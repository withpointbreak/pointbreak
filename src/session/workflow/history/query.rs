use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use super::cursor::{HistoryCursor, cmp_key, next_cursor_for};
use super::projection::{BaseEntry, BaseHistoryProjection};
use super::search::{
    QueryClause, QueryDiagnostic, QuerySurface, entry_actor, entry_track, event_type_wire,
    matches_query, parse_search_query_for, tag_completion_key,
};
use super::summary::{ReviewHistoryEntry, ReviewHistorySummary};
use crate::model::{EventId, ReviewTargetRef, RevisionId};
use crate::session::ProjectionDiagnostic;

/// The display order of the queried history page. The base is stored ascending
/// `(occurred_at, event_id)`; `Desc` reverses the filtered set before windowing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HistoryOrder {
    #[default]
    Asc,
    Desc,
}

/// The query applied to the base projection: full-text `q`, the exact `track` and
/// `snapshot` params, the enabled event-type page set (`None` => all types), and the
/// display order. Pure data — `apply_history_query` runs it over a cached base.
#[derive(Clone, Debug, Default)]
pub struct HistoryQuery {
    pub q: String,
    pub track: Option<String>,
    pub snapshot: Option<String>,
    /// An exact, already-resolved revision identity. Resolution never happens
    /// inside the query engine.
    pub revision: Option<RevisionId>,
    /// An already-resolved revision set. `Some(empty)` matches nothing while
    /// `None` leaves the revision dimension unconstrained.
    pub revisions: Option<BTreeSet<RevisionId>>,
    pub types: Option<BTreeSet<String>>,
    pub order: HistoryOrder,
}

/// The query-path window spec. Precedence `after` › `at` › `offset`; a bare
/// `limit` is the first page. The inspector uses `at`/`offset` for random access,
/// while the CLI uses the forward-only opaque cursor.
#[derive(Clone, Debug, Default)]
pub struct HistoryPage {
    pub limit: Option<usize>,
    /// Start strictly after this ascending stream key. Callers must reject
    /// `HistoryOrder::Desc` before the engine because the ascending partition
    /// predicate is not partitioned over a reversed slice.
    pub after: Option<HistoryCursor>,
    pub offset: Option<usize>,
    pub at: Option<EventId>,
}

/// Distinct values across the whole base projection — store-wide vocabulary,
/// independent of the live q/track/snapshot/types query (unlike `facets`,
/// which narrows with it). Computing this under the live query would
/// self-defeat completion: typing `track:cod` would filter out every record
/// carrying the very value being completed, and a query whose clauses
/// jointly match nothing (a committed clause plus a partially-typed second
/// one) would report an empty vocabulary altogether. Values are derived from
/// the raw DOMAIN fields (`entry_track`/`entry_actor`/observation tags),
/// never from the space-wrapped search-record encoding — a fallback Git-name
/// actor id legally contains spaces, and splitting the encoded set would
/// fragment it into junk completions that also diverge from the cold
/// default-page path's raw-envelope reads. `tag` carries first-colon KEYS
/// only (e.g. "issue", not "issue:191") — the useful completion vocabulary;
/// the full string still matches via the existing set-membership `tag`
/// field, this struct is additive autocomplete input, not a matching change.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DistinctValues {
    pub track: Vec<String>,
    pub actor: Vec<String>,
    pub tag: Vec<String>,
}

/// The result of `apply_history_query`: the windowed page plus the facet counts,
/// the full filtered size (`match_count`), the window start (`offset`), the located
/// index for an `at` request (`match_index`), and the FULL-set identity (never the
/// filtered set — plan 0092 INV-5).
pub struct QueriedHistory {
    pub entries: Vec<ReviewHistoryEntry>,
    pub next_cursor: Option<HistoryCursor>,
    pub facets: BTreeMap<String, usize>,
    pub match_count: usize,
    pub offset: usize,
    pub match_index: Option<usize>,
    pub event_set_hash: String,
    pub event_count: usize,
    pub diagnostics: Vec<ProjectionDiagnostic>,
    /// Parse diagnostics for the applied `q` (deprecation hints on a 200) — a
    /// sibling of the store-integrity `diagnostics`, never mixed in.
    pub query_notices: Vec<QueryDiagnostic>,
    /// Store-wide completion vocabulary (see [`DistinctValues`]) — always the
    /// unfiltered base's values, never the matched set's.
    pub distinct_values: DistinctValues,
}

/// Run the query over the cached base projection, purely (no I/O): filter (q +
/// track + object + types page set) → facets (excluding the types page set, INV-3)
/// → order → window (`at` › `offset`, INV-2). Identity is always the full replayed
/// set (INV-2).
pub fn apply_history_query(
    base: &BaseHistoryProjection,
    query: &HistoryQuery,
    page: &HistoryPage,
) -> QueriedHistory {
    debug_assert!(
        !(matches!(query.order, HistoryOrder::Desc) && page.after.is_some()),
        "history cursor windowing requires ascending order"
    );
    let span = tracing::debug_span!(
        "shore.history.apply_query",
        base_entry_count = base.entries.len(),
        q_empty = query.q.is_empty()
    );
    let _guard = span.enter();

    let parsed = {
        let span = tracing::debug_span!("shore.history.query.parse_search_query");
        let _guard = span.enter();
        parse_search_query_for(&query.q, QuerySurface::Event)
    };
    let clauses = parsed.clauses;
    // The facet predicate is q + track + snapshot, EXCLUDING the `types` page filter
    // (INV-3). The page predicate additionally applies the `types` set.
    let facet_match = |entry: &BaseEntry| {
        track_snapshot_match(entry, query)
            && revision_dims_match(entry, query)
            && matches_query(&entry.record, &clauses)
    };

    let facets = {
        let span = tracing::debug_span!("shore.history.query.facets");
        let _guard = span.enter();
        let mut facets: BTreeMap<String, usize> = BTreeMap::new();
        for entry in base.entries.iter().filter(|&entry| facet_match(entry)) {
            *facets.entry(event_type_wire(&entry.entry)).or_default() += 1;
        }
        facets
    };

    // Iterates `base.entries` directly, never `.filter(facet_match)` — the
    // completion vocabulary is store-wide by contract (see `DistinctValues`).
    // Domain values, not record tokens: the space-wrapped set encoding cannot
    // carry a space-bearing id losslessly, and the cold default-page path
    // reads raw envelope/payload values — the two must agree byte-for-byte.
    let distinct_values = {
        let span = tracing::debug_span!("shore.history.query.distinct_values");
        let _guard = span.enter();
        let mut track = BTreeSet::new();
        let mut actor = BTreeSet::new();
        let mut tag = BTreeSet::new();
        for entry in &base.entries {
            let track_value = entry_track(&entry.entry);
            if !track_value.is_empty() {
                track.insert(track_value.to_lowercase());
            }
            let actor_value = entry_actor(&entry.entry);
            if !actor_value.is_empty() {
                actor.insert(actor_value.to_lowercase());
            }
            if let ReviewHistorySummary::ReviewObservationRecorded { tags, .. } =
                &entry.entry.summary
            {
                for full in tags {
                    if let Some(key) = tag_completion_key(full) {
                        tag.insert(key);
                    }
                }
            }
        }
        DistinctValues {
            track: track.into_iter().collect(),
            actor: actor.into_iter().collect(),
            tag: tag.into_iter().collect(),
        }
    };

    let mut filtered: Vec<&BaseEntry> = {
        let span = tracing::debug_span!("shore.history.query.filter_entries");
        let _guard = span.enter();
        base.entries
            .iter()
            .filter(|&entry| passes_page_filter(entry, query, &clauses))
            .collect()
    };
    // The base is ascending `(occurred_at, event_id)`; `Desc` reverses the ordered
    // filtered set so windowing runs in display order (INV-2).
    if matches!(query.order, HistoryOrder::Desc) {
        let span = tracing::debug_span!("shore.history.query.reverse_filtered");
        let _guard = span.enter();
        filtered.reverse();
    }
    let match_count = filtered.len();

    let (range, match_index) = {
        let span = tracing::debug_span!("shore.history.query.resolve_window");
        let _guard = span.enter();
        resolve_window(&filtered, page)
    };
    let next_cursor = if matches!(query.order, HistoryOrder::Asc) {
        let keys: Vec<HistoryCursor> = filtered
            .iter()
            .map(|entry| HistoryCursor {
                occurred_at: entry.entry.occurred_at.clone(),
                event_id: entry.entry.event_id.clone(),
            })
            .collect();
        next_cursor_for(&keys, &range)
    } else {
        None
    };
    let entries = {
        let span = tracing::debug_span!(
            "shore.history.query.clone_window_entries",
            window_count = range.len()
        );
        let _guard = span.enter();
        filtered[range.clone()]
            .iter()
            .map(|entry| entry.entry.clone())
            .collect()
    };

    QueriedHistory {
        entries,
        next_cursor,
        facets,
        match_count,
        offset: range.start,
        match_index,
        event_set_hash: base.event_set_hash.clone(),
        event_count: base.event_count,
        diagnostics: base.diagnostics.clone(),
        query_notices: parsed.diagnostics,
        distinct_values,
    }
}

/// The `track=` and `snapshot=` params. The record's `track` field is a
/// space-wrapped token set, so the track param matches by whole-token membership
/// (aligned with the `track:` grammar kind); `snapshot` stays raw equality. An
/// absent param does not constrain. The `snapshot` field value is sourced from
/// the shared `object_id` document field (renamed grammar key, #334).
fn track_snapshot_match(entry: &BaseEntry, query: &HistoryQuery) -> bool {
    if let Some(track) = &query.track
        && !entry
            .record
            .field("track")
            .unwrap_or("")
            .contains(&format!(" {} ", track.to_lowercase()))
    {
        return false;
    }
    if let Some(snapshot) = &query.snapshot
        && entry.record.field("snapshot") != Some(snapshot.as_str())
    {
        return false;
    }
    true
}

/// The page's per-entry filter: exact params, parsed `q` clauses, and the
/// enabled event-type set. Facets intentionally use their separate predicate.
fn passes_page_filter(entry: &BaseEntry, query: &HistoryQuery, clauses: &[QueryClause]) -> bool {
    track_snapshot_match(entry, query)
        && revision_dims_match(entry, query)
        && matches_query(&entry.record, clauses)
        && type_set_match(entry, query.types.as_ref())
}

/// The exact typed revision dimensions shared by page filtering and facets.
fn revision_dims_match(entry: &BaseEntry, query: &HistoryQuery) -> bool {
    let revision_id = entry.entry.subject.as_ref().map(review_target_revision_id);
    if let Some(expected) = query.revision.as_ref()
        && revision_id != Some(expected)
    {
        return false;
    }
    match &query.revisions {
        None => true,
        Some(revisions) => revision_id.is_some_and(|revision_id| revisions.contains(revision_id)),
    }
}

fn review_target_revision_id(target: &ReviewTargetRef) -> &RevisionId {
    match target {
        ReviewTargetRef::Revision { revision_id }
        | ReviewTargetRef::File { revision_id, .. }
        | ReviewTargetRef::Range { revision_id, .. }
        | ReviewTargetRef::Observation { revision_id, .. }
        | ReviewTargetRef::InputRequest { revision_id, .. }
        | ReviewTargetRef::Assessment { revision_id, .. }
        | ReviewTargetRef::Event { revision_id, .. } => revision_id,
    }
}

/// Count filtered entries strictly newer than `since` in the ascending
/// `(occurred_at, event_id)` key space.
///
/// Unlike `HistoryPage.at`, this positions by key rather than requiring the
/// anchor event to remain in the filtered set.
pub fn count_new_since(
    base: &BaseHistoryProjection,
    query: &HistoryQuery,
    since: &HistoryCursor,
) -> usize {
    let parsed = parse_search_query_for(&query.q, QuerySurface::Event);
    let filtered: Vec<&BaseEntry> = base
        .entries
        .iter()
        .filter(|entry| passes_page_filter(entry, query, &parsed.clauses))
        .collect();
    let seen = filtered.partition_point(|entry| {
        cmp_key(&entry.entry.occurred_at, entry.entry.event_id.as_str())
            <= cmp_key(&since.occurred_at, since.event_id.as_str())
    });
    filtered.len() - seen
}

/// The `types` page filter (the enabled event-type set): `None` => all types;
/// `Some(set)` keeps only entries whose event-type wire id is in the set.
fn type_set_match(entry: &BaseEntry, types: Option<&BTreeSet<String>>) -> bool {
    match types {
        None => true,
        Some(set) => set.contains(&event_type_wire(&entry.entry)),
    }
}

/// The exclusive window end for `start` under `limit` (`None` => to `len`),
/// saturating so an attacker-supplied huge `limit` cannot overflow. `Some(0)`
/// yields an empty `start..start` — never a divide-by-zero (F6).
fn page_end(start: usize, limit: Option<usize>, len: usize) -> usize {
    match limit {
        Some(limit) => start.saturating_add(limit).min(len),
        None => len,
    }
}

/// Resolve the window over the filtered + display-ordered set. Precedence
/// `after` › `at` › `offset` (a bare `limit` is the first page). Returns the
/// index range and, for an `at` request, the located index (`match_index`).
fn resolve_window(filtered: &[&BaseEntry], page: &HistoryPage) -> (Range<usize>, Option<usize>) {
    let len = filtered.len();
    if let Some(after) = &page.after {
        let start = filtered.partition_point(|entry| {
            cmp_key(&entry.entry.occurred_at, entry.entry.event_id.as_str())
                <= cmp_key(&after.occurred_at, after.event_id.as_str())
        });
        return (start..page_end(start, page.limit, len), None);
    }
    if let Some(at) = &page.at {
        let Some(index) = filtered
            .iter()
            .position(|entry| &entry.entry.event_id == at)
        else {
            // The located event is filtered out — an empty page, no index (INV-2).
            return (0..0, None);
        };
        // Page-align the window on the located index. Division by the limit happens
        // only in the `limit > 0` arm, so a zero/absent limit never divides (F6).
        let range = match page.limit {
            Some(0) => 0..0,
            None => 0..len,
            Some(limit) => {
                let start = (index / limit) * limit;
                start..page_end(start, Some(limit), len)
            }
        };
        return (range, Some(index));
    }
    let start = page.offset.unwrap_or(0).min(len);
    (start..page_end(start, page.limit, len), None)
}

#[cfg(test)]
mod tests {
    use super::super::search::{EventRecordExtras, SearchRecord, build_haystack};
    use super::super::summary::ReviewHistorySummary;
    use super::*;
    use crate::model::{ObservationId, ReviewTargetRef, RevisionId};
    use crate::session::event::{EventType, Writer};

    fn entry(
        occurred_at: &str,
        id: &str,
        event_type: EventType,
        title: &str,
        track: &str,
        revision: &str,
    ) -> ReviewHistoryEntry {
        let summary = match event_type {
            EventType::ReviewAssessmentRecorded => ReviewHistorySummary::ReviewAssessmentRecorded {
                assessment_id: crate::model::AssessmentId::new("assess:sha256:x"),
                target: ReviewTargetRef::Revision {
                    revision_id: RevisionId::new(revision),
                },
                assessment: crate::session::event::ReviewAssessment::Accepted,
                summary: Some(title.to_owned()),
                summary_content_type: Default::default(),
                summary_byte_size: None,
                summary_content_hash: None,
                summary_content_state: Default::default(),
                replaces: vec![],
                related_observations: vec![],
                related_input_requests: vec![],
            },
            _ => ReviewHistorySummary::ReviewObservationRecorded {
                observation_id: ObservationId::new("obs:sha256:x"),
                target: ReviewTargetRef::Revision {
                    revision_id: RevisionId::new(revision),
                },
                title: title.to_owned(),
                body: None,
                body_content_type: Default::default(),
                body_byte_size: None,
                body_content_hash: None,
                body_content_state: Default::default(),
                tags: vec![],
                confidence: None,
                supersedes: vec![],
                responds_to: vec![],
            },
        };
        ReviewHistoryEntry {
            event_id: EventId::new(id),
            event_type,
            occurred_at: occurred_at.to_owned(),
            payload_hash: "sha256:x".to_owned(),
            journal_id: crate::model::JournalId::new("journal:default"),
            track_id: Some(crate::model::TrackId::new(track)),
            subject: Some(ReviewTargetRef::Revision {
                revision_id: RevisionId::new(revision),
            }),
            writer: Writer::shore_local("test"),
            verification_status: None,
            endorsements: vec![],
            principal: None,
            summary,
        }
    }

    fn base_from(entries: Vec<(ReviewHistoryEntry, &str)>) -> BaseHistoryProjection {
        let count = entries.len();
        let base_entries = entries
            .into_iter()
            .map(|(entry, object)| {
                let record =
                    SearchRecord::from_entry(&entry, object, &EventRecordExtras::default());
                BaseEntry { entry, record }
            })
            .collect();
        BaseHistoryProjection {
            entries: base_entries,
            event_set_hash: "sha256:test".to_owned(),
            event_count: count,
            diagnostics: Vec::new(),
        }
    }

    /// `n` ascending observation entries on one track/revision.
    fn base_of(n: usize) -> BaseHistoryProjection {
        let entries = (1..=n)
            .map(|i| {
                (
                    entry(
                        &format!("2026-05-13T10:00:{i:02}Z"),
                        &format!("evt:sha256:{i:02}"),
                        EventType::ReviewObservationRecorded,
                        &format!("entry {i}"),
                        "agent:codex",
                        "rev:sha256:one",
                    ),
                    "obj:sha256:one",
                )
            })
            .collect();
        base_from(entries)
    }

    fn base_with_titles(titles: &[&str]) -> BaseHistoryProjection {
        let entries = titles
            .iter()
            .enumerate()
            .map(|(i, title)| {
                (
                    entry(
                        &format!("2026-05-13T10:00:{:02}Z", i + 1),
                        &format!("evt:sha256:{:02}", i + 1),
                        EventType::ReviewObservationRecorded,
                        title,
                        "agent:codex",
                        "rev:sha256:one",
                    ),
                    "obj:sha256:one",
                )
            })
            .collect();
        base_from(entries)
    }

    /// Two observations and one assessment, ascending.
    fn mixed_base() -> BaseHistoryProjection {
        base_from(vec![
            (
                entry(
                    "2026-05-13T10:00:01Z",
                    "evt:sha256:01",
                    EventType::ReviewObservationRecorded,
                    "first observation",
                    "agent:codex",
                    "rev:sha256:one",
                ),
                "obj:sha256:one",
            ),
            (
                entry(
                    "2026-05-13T10:00:02Z",
                    "evt:sha256:02",
                    EventType::ReviewObservationRecorded,
                    "second observation",
                    "agent:codex",
                    "rev:sha256:one",
                ),
                "obj:sha256:one",
            ),
            (
                entry(
                    "2026-05-13T10:00:03Z",
                    "evt:sha256:03",
                    EventType::ReviewAssessmentRecorded,
                    "an assessment",
                    "human:kevin",
                    "rev:sha256:one",
                ),
                "obj:sha256:one",
            ),
        ])
    }

    /// Observation entries titled and tagged per `specs`, ascending.
    fn base_with_tags(specs: &[(&str, &[&str])]) -> BaseHistoryProjection {
        let entries = specs
            .iter()
            .enumerate()
            .map(|(i, (title, tags))| {
                let mut e = entry(
                    &format!("2026-05-13T10:00:{:02}Z", i + 1),
                    &format!("evt:sha256:{:02}", i + 1),
                    EventType::ReviewObservationRecorded,
                    title,
                    "agent:codex",
                    "rev:sha256:one",
                );
                if let ReviewHistorySummary::ReviewObservationRecorded {
                    tags: entry_tags, ..
                } = &mut e.summary
                {
                    *entry_tags = tags.iter().map(|tag| (*tag).to_owned()).collect();
                }
                (e, "obj:sha256:one")
            })
            .collect();
        base_from(entries)
    }

    fn page(limit: Option<usize>) -> HistoryPage {
        HistoryPage {
            limit,
            after: None,
            offset: None,
            at: None,
        }
    }

    fn offset_page(limit: usize, offset: usize) -> HistoryPage {
        HistoryPage {
            limit: Some(limit),
            after: None,
            offset: Some(offset),
            at: None,
        }
    }

    fn cursor_for(entry: &ReviewHistoryEntry) -> HistoryCursor {
        HistoryCursor {
            occurred_at: entry.occurred_at.clone(),
            event_id: entry.event_id.clone(),
        }
    }

    #[test]
    fn track_param_matches_an_explicit_track_by_whole_token() {
        let base = base_of(3); // three entries on track agent:codex
        let q = HistoryQuery {
            track: Some("agent:codex".to_owned()),
            ..Default::default()
        };
        let out = apply_history_query(&base, &q, &HistoryPage::default());
        assert_eq!(out.match_count, 3);
    }

    #[test]
    fn track_param_matches_explicit_tracks_only_not_the_writer_actor() {
        // An actor-only entry no longer answers a ?track=<actor-id> scope: the record
        // track field is the explicit track only now.
        let mut e = entry(
            "2026-05-13T10:00:01Z",
            "evt:sha256:01",
            EventType::ReviewObservationRecorded,
            "obs",
            "agent:codex",
            "rev:sha256:one",
        );
        e.track_id = None; // writer actor becomes the only lane
        let actor = e.writer.actor_id.as_str().to_owned();
        let base = base_from(vec![(e, "obj:sha256:one")]);
        let q = HistoryQuery {
            track: Some(actor),
            ..Default::default()
        };
        let out = apply_history_query(&base, &q, &HistoryPage::default());
        assert_eq!(out.match_count, 0);
    }

    #[test]
    fn empty_query_unwindowed_equals_base_order_and_full_identity() {
        let base = base_of(5);
        let out = apply_history_query(&base, &HistoryQuery::default(), &HistoryPage::default());
        assert_eq!(out.entries.len(), 5);
        assert_eq!(out.match_count, 5);
        assert_eq!(out.offset, 0);
        assert_eq!(out.event_count, base.event_count);
        assert_eq!(out.event_set_hash, base.event_set_hash);
    }

    #[test]
    fn count_new_since_counts_entries_strictly_newer_than_the_anchor() {
        let base = base_of(5);
        let anchor = &base.entries[2].entry;
        let since = HistoryCursor {
            occurred_at: anchor.occurred_at.clone(),
            event_id: anchor.event_id.clone(),
        };

        assert_eq!(count_new_since(&base, &HistoryQuery::default(), &since), 2);
    }

    #[test]
    fn count_new_since_is_filter_aware() {
        let base = mixed_base();
        let anchor = &base.entries[0].entry;
        let since = HistoryCursor {
            occurred_at: anchor.occurred_at.clone(),
            event_id: anchor.event_id.clone(),
        };
        let query = HistoryQuery {
            q: "type:observation".to_owned(),
            ..Default::default()
        };

        assert_eq!(count_new_since(&base, &query, &since), 1);
        assert_eq!(count_new_since(&base, &HistoryQuery::default(), &since), 2);
    }

    #[test]
    fn count_new_since_survives_an_anchor_absent_from_the_filtered_set() {
        let base = base_of(5);
        let since = HistoryCursor {
            occurred_at: "2026-05-13T10:00:03.500Z".to_owned(),
            event_id: EventId::new("evt:sha256:absent"),
        };

        assert_eq!(count_new_since(&base, &HistoryQuery::default(), &since), 2);
    }

    #[test]
    fn revision_dimension_is_exact_typed_identity() {
        let base = base_from(vec![
            (
                entry(
                    "2026-05-13T10:00:01Z",
                    "evt:sha256:01",
                    EventType::ReviewObservationRecorded,
                    "first revision",
                    "agent:codex",
                    "rev:sha256:one",
                ),
                "obj:sha256:one",
            ),
            (
                entry(
                    "2026-05-13T10:00:02Z",
                    "evt:sha256:02",
                    EventType::ReviewObservationRecorded,
                    "second revision",
                    "agent:codex",
                    "rev:sha256:two",
                ),
                "obj:sha256:two",
            ),
        ]);
        let query = HistoryQuery {
            revision: Some(RevisionId::new("rev:sha256:two")),
            ..Default::default()
        };

        let out = apply_history_query(&base, &query, &HistoryPage::default());

        assert_eq!(out.match_count, 1);
        assert_eq!(
            out.entries[0].subject,
            Some(ReviewTargetRef::Revision {
                revision_id: RevisionId::new("rev:sha256:two"),
            })
        );
    }

    #[test]
    fn revisions_set_is_membership_and_empty_means_match_nothing() {
        let base = base_of(3);
        let empty = HistoryQuery {
            revisions: Some(BTreeSet::new()),
            ..Default::default()
        };
        assert_eq!(
            apply_history_query(&base, &empty, &HistoryPage::default()).match_count,
            0
        );

        let matching = HistoryQuery {
            revisions: Some([RevisionId::new("rev:sha256:one")].into_iter().collect()),
            ..Default::default()
        };
        assert_eq!(
            apply_history_query(&base, &matching, &HistoryPage::default()).match_count,
            3
        );
        assert_eq!(
            apply_history_query(&base, &HistoryQuery::default(), &HistoryPage::default())
                .match_count,
            3
        );
    }

    #[test]
    fn facets_honor_revision_dimensions_and_distinct_values_stay_independent() {
        let base = base_from(vec![
            (
                entry(
                    "2026-05-13T10:00:01Z",
                    "evt:sha256:01",
                    EventType::ReviewObservationRecorded,
                    "first observation",
                    "agent:codex",
                    "rev:sha256:one",
                ),
                "obj:sha256:one",
            ),
            (
                entry(
                    "2026-05-13T10:00:02Z",
                    "evt:sha256:02",
                    EventType::ReviewObservationRecorded,
                    "second observation",
                    "agent:other",
                    "rev:sha256:two",
                ),
                "obj:sha256:two",
            ),
            (
                entry(
                    "2026-05-13T10:00:03Z",
                    "evt:sha256:03",
                    EventType::ReviewAssessmentRecorded,
                    "second assessment",
                    "human:kevin",
                    "rev:sha256:two",
                ),
                "obj:sha256:two",
            ),
        ]);
        let baseline =
            apply_history_query(&base, &HistoryQuery::default(), &HistoryPage::default());
        let query = HistoryQuery {
            revisions: Some([RevisionId::new("rev:sha256:two")].into_iter().collect()),
            ..Default::default()
        };

        let out = apply_history_query(&base, &query, &HistoryPage::default());

        assert_eq!(out.match_count, 2);
        assert_eq!(out.facets.get("review_observation_recorded"), Some(&1));
        assert_eq!(out.facets.get("review_assessment_recorded"), Some(&1));
        assert_eq!(out.distinct_values, baseline.distinct_values);
    }

    #[test]
    fn count_new_since_inherits_revision_dimensions() {
        let base = base_from(vec![
            (
                entry(
                    "2026-05-13T10:00:01Z",
                    "evt:sha256:01",
                    EventType::ReviewObservationRecorded,
                    "anchor",
                    "agent:codex",
                    "rev:sha256:one",
                ),
                "obj:sha256:one",
            ),
            (
                entry(
                    "2026-05-13T10:00:02Z",
                    "evt:sha256:02",
                    EventType::ReviewObservationRecorded,
                    "matching newer",
                    "agent:codex",
                    "rev:sha256:two",
                ),
                "obj:sha256:two",
            ),
            (
                entry(
                    "2026-05-13T10:00:03Z",
                    "evt:sha256:03",
                    EventType::ReviewAssessmentRecorded,
                    "non-matching newer",
                    "human:kevin",
                    "rev:sha256:one",
                ),
                "obj:sha256:one",
            ),
        ]);
        let anchor = &base.entries[0].entry;
        let since = HistoryCursor {
            occurred_at: anchor.occurred_at.clone(),
            event_id: anchor.event_id.clone(),
        };
        let query = HistoryQuery {
            revisions: Some([RevisionId::new("rev:sha256:two")].into_iter().collect()),
            ..Default::default()
        };

        assert_eq!(count_new_since(&base, &query, &since), 1);
        assert_eq!(count_new_since(&base, &HistoryQuery::default(), &since), 2);
    }

    #[test]
    fn q_filters_page_and_match_count_over_filtered_set() {
        let base = base_with_titles(&["pinned alpha", "other", "pinned beta"]);
        let q = HistoryQuery {
            q: "pinned".into(),
            ..Default::default()
        };
        let out = apply_history_query(&base, &q, &HistoryPage::default());
        assert_eq!(out.match_count, 2);
        assert!(
            out.entries
                .iter()
                .all(|e| build_haystack(e).contains("pinned"))
        );
    }

    #[test]
    fn facets_exclude_the_types_page_filter_but_honor_q() {
        let base = mixed_base();
        let mut types = BTreeSet::new();
        types.insert("review_observation_recorded".to_owned());
        let q = HistoryQuery {
            types: Some(types),
            ..Default::default()
        };
        let out = apply_history_query(&base, &q, &HistoryPage::default());
        assert_eq!(out.entries.len(), 2);
        assert_eq!(out.match_count, 2);
        assert_eq!(out.facets.get("review_observation_recorded"), Some(&2));
        assert_eq!(out.facets.get("review_assessment_recorded"), Some(&1));
    }

    #[test]
    fn type_clause_in_q_affects_facets_unlike_the_types_param() {
        let base = mixed_base();
        let q = HistoryQuery {
            q: "type:observation".into(),
            ..Default::default()
        };
        let out = apply_history_query(&base, &q, &HistoryPage::default());
        assert_eq!(out.facets.get("review_assessment_recorded"), None);
        assert_eq!(out.facets.get("review_observation_recorded"), Some(&2));
    }

    #[test]
    fn distinct_values_are_independent_of_q_track_snapshot_and_types() {
        // mixed_base(): two observations on "agent:codex", one assessment on
        // "human:kevin". The unfiltered baseline and a second query that narrows
        // q, track, snapshot, AND the types page set all at once — together
        // matching nothing — must report the IDENTICAL distinct values: none of
        // those params may narrow the completion vocabulary.
        let base = mixed_base();
        let baseline =
            apply_history_query(&base, &HistoryQuery::default(), &HistoryPage::default());

        let mut types = BTreeSet::new();
        types.insert("review_observation_recorded".to_owned());
        let narrow = HistoryQuery {
            q: "pinned".into(),
            track: Some("agent:codex".into()),
            snapshot: Some("obj:sha256:one".into()),
            types: Some(types),
            ..Default::default()
        };
        let out = apply_history_query(&base, &narrow, &HistoryPage::default());
        assert_eq!(
            out.match_count, 0,
            "sanity check: the narrowed query matches nothing"
        );
        assert_eq!(out.distinct_values, baseline.distinct_values);
    }

    #[test]
    fn distinct_values_survive_a_query_whose_clauses_jointly_match_no_records() {
        // A committed clause (track:agent:codex) plus a partially-typed second
        // qualifier (tag:co, matching no complete tag on these tag-less entries)
        // together match ZERO records. If distinct values were still scoped to
        // the matched set, this would surface an EMPTY vocabulary — filtering the
        // very value a reader is mid-typing out of its own suggestion list.
        let base = mixed_base();
        let q = HistoryQuery {
            q: "track:agent:codex tag:co".into(),
            ..Default::default()
        };
        let out = apply_history_query(&base, &q, &HistoryPage::default());
        assert_eq!(
            out.match_count, 0,
            "sanity check: the committed query matches nothing"
        );
        assert!(
            out.distinct_values
                .track
                .contains(&"agent:codex".to_owned())
        );
        assert!(
            out.distinct_values
                .track
                .contains(&"human:kevin".to_owned())
        );
    }

    #[test]
    fn distinct_values_keep_whitespace_bearing_actor_and_tag_values_whole() {
        // Fallback Git-name actor ids legally contain spaces, and tags are free
        // strings. The completion vocabulary must carry the whole domain value:
        // fragmenting on the encoded set's internal spaces would offer junk
        // completions and diverge from the cold default-page path, which reads
        // the raw envelope/payload values.
        let mut e = entry(
            "2026-05-13T10:00:01Z",
            "evt:sha256:01",
            EventType::ReviewObservationRecorded,
            "obs",
            "agent:codex",
            "rev:sha256:one",
        );
        e.writer.actor_id = crate::model::ActorId::new("actor:git-name:Kevin Swiber");
        if let ReviewHistorySummary::ReviewObservationRecorded { tags, .. } = &mut e.summary {
            *tags = vec!["needs follow up".to_owned()];
        }
        let base = base_from(vec![(e, "obj:sha256:one")]);
        let out = apply_history_query(&base, &HistoryQuery::default(), &HistoryPage::default());
        assert_eq!(
            out.distinct_values.actor,
            vec!["actor:git-name:kevin swiber".to_owned()],
            "the whole lowercased actor id, never its space-split fragments"
        );
        assert_eq!(
            out.distinct_values.tag,
            vec!["needs follow up".to_owned()],
            "the whole lowercased tag, never its space-split fragments"
        );
    }

    #[test]
    fn distinct_tag_values_are_first_colon_keys_not_full_strings() {
        let base = base_with_tags(&[("issue:191", &["issue:191"]), ("bare", &["correctness"])]);
        let out = apply_history_query(&base, &HistoryQuery::default(), &HistoryPage::default());
        assert!(out.distinct_values.tag.contains(&"issue".to_owned()));
        assert!(out.distinct_values.tag.contains(&"correctness".to_owned()));
        assert!(
            !out.distinct_values.tag.contains(&"issue:191".to_owned()),
            "the full tag string is not a distinct VALUE — only its first-colon key is (the useful \
             completion vocabulary); `tag:issue:191` still MATCHES via the set-membership field, \
             this is only about what's offered as an autocomplete suggestion"
        );
    }

    #[test]
    fn order_desc_reverses_and_window_pages_in_display_order() {
        let base = base_of(5);
        let q = HistoryQuery {
            order: HistoryOrder::Desc,
            ..Default::default()
        };
        let out = apply_history_query(&base, &q, &page(Some(2)));
        assert!(out.entries[0].occurred_at > out.entries[1].occurred_at);
        assert_eq!(out.offset, 0);
    }

    #[test]
    fn offset_paging_continues_the_filtered_set_without_overlap() {
        let base = base_of(5);
        let p1 = apply_history_query(&base, &HistoryQuery::default(), &page(Some(2)));
        let p2 = apply_history_query(&base, &HistoryQuery::default(), &offset_page(2, 2));
        assert_eq!(p1.entries.len(), 2);
        assert_eq!(p2.offset, 2);
        assert_ne!(
            p1.entries.last().unwrap().event_id,
            p2.entries.first().unwrap().event_id
        );
    }

    #[test]
    fn desc_offset_paging_continues_toward_older_entries() {
        let base = base_of(5);
        let q = HistoryQuery {
            order: HistoryOrder::Desc,
            ..Default::default()
        };
        let p1 = apply_history_query(&base, &q, &page(Some(2))); // the two newest
        let p2 = apply_history_query(&base, &q, &offset_page(2, 2)); // the next two, older
        assert!(p2.entries.first().unwrap().occurred_at < p1.entries.last().unwrap().occurred_at);
        assert_eq!(p2.offset, 2);
    }

    #[test]
    fn offset_windows_the_filtered_set() {
        let base = base_of(5);
        let out = apply_history_query(&base, &HistoryQuery::default(), &offset_page(2, 1));
        assert_eq!(out.offset, 1);
        assert_eq!(out.entries.len(), 2);
        assert_eq!(out.match_count, 5);
    }

    #[test]
    fn no_window_takes_all_without_a_cursor() {
        let base = base_of(5);

        let out = apply_history_query(&base, &HistoryQuery::default(), &HistoryPage::default());

        assert_eq!(out.entries.len(), 5);
        assert!(out.next_cursor.is_none());
    }

    #[test]
    fn limit_takes_prefix_and_emits_next_cursor() {
        let base = base_of(5);

        let out = apply_history_query(&base, &HistoryQuery::default(), &page(Some(2)));

        assert_eq!(out.entries.len(), 2);
        assert_eq!(out.next_cursor, Some(cursor_for(&base.entries[1].entry)));
    }

    #[test]
    fn after_skips_through_and_past_the_cursor_key() {
        let base = base_of(5);
        let page = HistoryPage {
            limit: Some(2),
            after: Some(cursor_for(&base.entries[1].entry)),
            ..Default::default()
        };

        let out = apply_history_query(&base, &HistoryQuery::default(), &page);

        assert_eq!(out.offset, 2);
        assert_eq!(out.entries.len(), 2);
        assert_eq!(out.entries[0].event_id, base.entries[2].entry.event_id);
        assert_eq!(out.next_cursor, Some(cursor_for(&base.entries[3].entry)));
    }

    #[test]
    fn after_takes_precedence_over_at_and_offset() {
        let base = base_of(5);
        let page = HistoryPage {
            limit: Some(1),
            after: Some(cursor_for(&base.entries[1].entry)),
            offset: Some(4),
            at: Some(base.entries[4].entry.event_id.clone()),
        };

        let out = apply_history_query(&base, &HistoryQuery::default(), &page);

        assert_eq!(out.offset, 2);
        assert_eq!(out.match_index, None);
        assert_eq!(out.entries[0].event_id, base.entries[2].entry.event_id);
    }

    #[test]
    #[should_panic(expected = "history cursor windowing requires ascending order")]
    fn after_under_desc_is_a_stated_precondition() {
        let base = base_of(2);
        let query = HistoryQuery {
            order: HistoryOrder::Desc,
            ..Default::default()
        };
        let page = HistoryPage {
            after: Some(cursor_for(&base.entries[0].entry)),
            ..Default::default()
        };

        let _ = apply_history_query(&base, &query, &page);
    }

    #[test]
    fn last_page_emits_no_next_cursor() {
        let base = base_of(3);

        let out = apply_history_query(&base, &HistoryQuery::default(), &page(Some(10)));

        assert_eq!(out.entries.len(), 3);
        assert!(out.next_cursor.is_none());
    }

    #[test]
    fn cursor_past_end_is_empty() {
        let base = base_of(2);
        let page = HistoryPage {
            limit: Some(5),
            after: Some(cursor_for(&base.entries[1].entry)),
            ..Default::default()
        };

        let out = apply_history_query(&base, &HistoryQuery::default(), &page);

        assert_eq!(out.offset, 2);
        assert!(out.entries.is_empty());
        assert!(out.next_cursor.is_none());
    }

    #[test]
    fn limit_zero_is_an_empty_window() {
        let base = base_of(3);

        let out = apply_history_query(&base, &HistoryQuery::default(), &page(Some(0)));

        assert!(out.entries.is_empty());
        assert!(out.next_cursor.is_none());
    }

    #[test]
    fn huge_limit_after_cursor_does_not_overflow() {
        let base = base_of(3);
        let page = HistoryPage {
            limit: Some(usize::MAX),
            after: Some(cursor_for(&base.entries[1].entry)),
            ..Default::default()
        };

        let out = apply_history_query(&base, &HistoryQuery::default(), &page);

        assert_eq!(out.entries.len(), 1);
        assert_eq!(out.entries[0].event_id, base.entries[2].entry.event_id);
        assert!(out.next_cursor.is_none());
    }

    #[test]
    fn at_locates_the_page_containing_an_event_and_sets_match_index() {
        let base = base_of(10);
        let target = base.entries[7].entry.event_id.clone();
        let out = apply_history_query(
            &base,
            &HistoryQuery::default(),
            &HistoryPage {
                limit: Some(3),
                after: None,
                offset: None,
                at: Some(target.clone()),
            },
        );
        assert_eq!(out.offset, 6);
        assert_eq!(out.match_index, Some(7));
        assert!(out.entries.iter().any(|e| e.event_id == target));
    }

    #[test]
    fn at_absent_from_filtered_set_returns_empty_with_no_match_index() {
        let base = base_with_titles(&["alpha", "beta"]);
        let q = HistoryQuery {
            q: "alpha".into(),
            ..Default::default()
        };
        let missing = base.entries[1].entry.event_id.clone();
        let out = apply_history_query(
            &base,
            &q,
            &HistoryPage {
                limit: Some(5),
                after: None,
                offset: None,
                at: Some(missing),
            },
        );
        assert!(out.match_index.is_none());
    }

    #[test]
    fn at_with_zero_limit_is_empty_and_does_not_panic() {
        let base = base_of(5);
        let target = base.entries[3].entry.event_id.clone();
        let out = apply_history_query(
            &base,
            &HistoryQuery::default(),
            &HistoryPage {
                limit: Some(0),
                after: None,
                offset: None,
                at: Some(target),
            },
        );
        assert!(out.entries.is_empty());
        assert_eq!(out.match_index, Some(3));
    }
}
