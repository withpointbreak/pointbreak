//! Pure filtering, ordering, and bidirectional windowing for the Change-aware Timeline.

use std::collections::BTreeMap;

use pointbreak::documents::{
    EventHistoryDocumentV1, EventHistoryEntryV1, EventHistoryOrderV1, EventHistorySummaryV1,
};
use pointbreak::session::{
    QueryDiagnosticCode, QuerySurface, SearchRecord, format_rfc3339_utc_millis, matches_query,
    parse_event_instant, parse_search_query_for,
};

use super::event_history_page::{
    Boundary, Order, Position, Request, TimelineKey, Traversal, issue_continuation,
    page_start_for_index,
};
use super::page_token::PageTokenSigner;

#[cfg(test)]
thread_local! {
    static SEARCH_RECORD_BUILD_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum ApplyError {
    Invalid(String),
    Stale,
}

/// Apply one already-authenticated request to an immutable Timeline generation.
///
/// The caller parses before loading the reader. This function performs no I/O:
/// it filters the typed entries, reverses the selected sequence when requested,
/// resolves the authenticated position, and emits adjacent-window tokens bound
/// to the same projection stamp.
pub(super) fn apply(
    mut document: EventHistoryDocumentV1,
    request: &Request,
    signer: &PageTokenSigner,
) -> Result<EventHistoryDocumentV1, ApplyError> {
    if request
        .continuation_projection_stamp()
        .is_some_and(|stamp| stamp != document.timeline_projection_stamp)
    {
        return Err(ApplyError::Stale);
    }

    let query = request.query();
    let parsed = parse_search_query_for(query.q().unwrap_or_default(), QuerySurface::Event);
    if let Some(fatal) = parsed.diagnostics.iter().find(|diagnostic| {
        matches!(
            diagnostic.code,
            QueryDiagnosticCode::UnsupportedQualifier | QueryDiagnosticCode::UnsupportedValue
        )
    }) {
        return Err(ApplyError::Invalid(fatal.message.clone()));
    }
    document.query_notices = parsed
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.clone())
        .collect();

    // Facets intentionally exclude only the URL's event-type page set. Every
    // other filter, including any type clause inside q, narrows their base.
    let facet_base = document
        .entries
        .iter()
        .filter(|entry| matches_non_type_filters(entry, request, &parsed.clauses))
        .cloned()
        .collect::<Vec<_>>();
    document.facets = facet_base
        .iter()
        .fold(BTreeMap::new(), |mut facets, entry| {
            *facets
                .entry(entry.event_type.as_str().to_owned())
                .or_insert(0) += 1;
            facets
        });

    let mut selected = facet_base
        .into_iter()
        .filter(|entry| query.types().is_empty() || query.types().contains(&entry.event_type))
        .collect::<Vec<_>>();
    if query.order() == Order::Desc {
        selected.reverse();
    }
    document.order = match query.order() {
        Order::Asc => EventHistoryOrderV1::Asc,
        Order::Desc => EventHistoryOrderV1::Desc,
    };
    document.match_count = selected.len();

    let (start, match_index) = match request.position() {
        Position::Initial => (0, None),
        Position::AtEventId(event_id) => {
            let index = selected
                .iter()
                .position(|entry| entry.event_id.as_str() == event_id)
                .ok_or_else(|| invalid("Timeline locator does not match this query"))?;
            (page_start_for_index(index, query.limit()), Some(index))
        }
        Position::Continuation {
            traversal,
            boundary,
        } => {
            let start = continuation_start(&selected, *traversal, boundary)?;
            if start % query.limit() != 0 {
                return Err(invalid("continuation boundary is not page-aligned"));
            }
            (start, None)
        }
    };
    if start > selected.len() {
        return Err(invalid("continuation boundary is outside this Timeline"));
    }
    let end = start.saturating_add(query.limit()).min(selected.len());

    document.previous = if start == 0 {
        None
    } else {
        let previous_start = start.saturating_sub(query.limit());
        let boundary = if previous_start == 0 {
            Boundary::SelectedOrderStart
        } else {
            Boundary::Key(key_for(&selected[previous_start - 1]))
        };
        Some(
            issue_continuation(
                query,
                &document.timeline_projection_stamp,
                Traversal::Previous,
                boundary,
                signer,
            )
            .map_err(page_error)?,
        )
    };
    document.next = if end < selected.len() {
        Some(
            issue_continuation(
                query,
                &document.timeline_projection_stamp,
                Traversal::Next,
                Boundary::Key(key_for(&selected[end - 1])),
                signer,
            )
            .map_err(page_error)?,
        )
    } else {
        None
    };
    document.offset = start;
    document.match_index = match_index;
    document.entries = selected[start..end].to_vec();
    Ok(document)
}

fn continuation_start(
    selected: &[EventHistoryEntryV1],
    traversal: Traversal,
    boundary: &Boundary,
) -> Result<usize, ApplyError> {
    match (traversal, boundary) {
        (Traversal::Previous, Boundary::SelectedOrderStart) => Ok(0),
        (Traversal::Next, Boundary::SelectedOrderStart) => {
            Err(invalid("next continuation cannot use the start sentinel"))
        }
        (_, Boundary::Key(key)) => selected
            .iter()
            .position(|entry| key_for(entry) == *key)
            .map(|index| index + 1)
            .ok_or_else(|| invalid("continuation boundary is absent from this Timeline")),
    }
}

fn matches_non_type_filters(
    entry: &EventHistoryEntryV1,
    request: &Request,
    clauses: &[pointbreak::session::QueryClause],
) -> bool {
    let query = request.query();
    if query.track().is_some_and(|track| {
        entry
            .track_id
            .as_ref()
            .is_none_or(|id| id.as_str() != track)
    }) {
        return false;
    }
    if query.change().is_some_and(|change| {
        !entry
            .change_ids
            .iter()
            .any(|candidate| candidate.as_str() == change)
    }) {
        return false;
    }
    if query.revision().is_some_and(|revision| {
        !entry.revision_refs.iter().any(|candidate| {
            candidate.revision_id.as_str() == revision.revision_id()
                && candidate.object_artifact_content_hash == revision.artifact_hash()
        })
    }) {
        return false;
    }
    clauses.is_empty() || matches_query(&search_record(entry), clauses)
}

fn search_record(entry: &EventHistoryEntryV1) -> SearchRecord {
    #[cfg(test)]
    SEARCH_RECORD_BUILD_COUNT.with(|count| count.set(count.get() + 1));

    let mut fields = BTreeMap::new();
    fields.insert("type".to_owned(), entry.event_type.as_str().to_owned());
    fields.insert(
        "track".to_owned(),
        token_set(entry.track_id.iter().map(|track| track.as_str())),
    );
    fields.insert(
        "actor".to_owned(),
        token_set(std::iter::once(entry.writer.actor_id.as_str())),
    );
    fields.insert(
        "revision".to_owned(),
        entry
            .revision_refs
            .iter()
            .map(|reference| reference.revision_id.as_str())
            .chain(
                entry
                    .unresolved_revision_ids
                    .iter()
                    .map(|revision| revision.as_str()),
            )
            .collect::<Vec<_>>()
            .join(" "),
    );
    fields.insert(
        "snapshot".to_owned(),
        entry
            .revision_refs
            .iter()
            .map(|reference| reference.object_artifact_content_hash.as_str())
            .collect::<Vec<_>>()
            .join(" "),
    );

    let (check, assessment, state, tags) = summary_search_fields(&entry.summary);
    fields.insert("check".to_owned(), check);
    fields.insert("assessment".to_owned(), assessment);
    fields.insert("is".to_owned(), token_set(state));
    fields.insert("tag".to_owned(), token_set(tags.iter().map(String::as_str)));
    fields.insert(
        "occurred_at".to_owned(),
        parse_event_instant(&entry.occurred_at)
            .map(format_rfc3339_utc_millis)
            .unwrap_or_default(),
    );
    SearchRecord {
        text: serde_json::to_string(entry)
            .expect("typed Timeline entry must serialize")
            .to_lowercase(),
        fields,
    }
}

#[cfg(test)]
fn reset_search_record_build_count() {
    SEARCH_RECORD_BUILD_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
fn search_record_build_count() -> usize {
    SEARCH_RECORD_BUILD_COUNT.with(std::cell::Cell::get)
}

fn summary_search_fields(
    summary: &EventHistorySummaryV1,
) -> (String, String, Option<&'static str>, Vec<String>) {
    let mut check = String::new();
    let mut assessment = String::new();
    let mut state = None;
    let mut tags = Vec::new();
    match summary {
        EventHistorySummaryV1::ReviewObservationRecorded(payload) => {
            tags = payload.tags.clone();
        }
        EventHistorySummaryV1::ReviewAssessmentRecorded(payload) => {
            assessment = wire(&payload.assessment);
        }
        EventHistorySummaryV1::InputRequestOpened(_) => state = Some("open"),
        EventHistorySummaryV1::InputRequestResponded(_) => state = Some("answered"),
        EventHistorySummaryV1::ValidationCheckRecorded(payload) => {
            check = wire(&payload.status);
        }
        EventHistorySummaryV1::ReviewInitialized
        | EventHistorySummaryV1::WorkObjectProposed { .. }
        | EventHistorySummaryV1::ReviewNoteImported
        | EventHistorySummaryV1::RevisionRefAssociated(_)
        | EventHistorySummaryV1::RevisionRefWithdrawn(_)
        | EventHistorySummaryV1::RevisionCommitAssociated(_)
        | EventHistorySummaryV1::RevisionCommitWithdrawn(_)
        | EventHistorySummaryV1::ChangeDeclared(_)
        | EventHistorySummaryV1::ChangeMembershipAsserted(_)
        | EventHistorySummaryV1::ChangeMembershipWithdrawn(_)
        | EventHistorySummaryV1::ChangeLinkAsserted(_)
        | EventHistorySummaryV1::ChangeRevisionRelationAsserted(_)
        | EventHistorySummaryV1::ChangeRevisionRelationWithdrawn(_)
        | EventHistorySummaryV1::RevisionRelationAttested(_)
        | EventHistorySummaryV1::ReviewFactPorted(_) => {}
    }
    (check, assessment, state, tags)
}

fn wire(value: &impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn token_set<'a>(tokens: impl IntoIterator<Item = &'a str>) -> String {
    let values = tokens
        .into_iter()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ");
    if values.is_empty() {
        String::new()
    } else {
        format!(" {values} ")
    }
}

fn key_for(entry: &EventHistoryEntryV1) -> TimelineKey {
    TimelineKey {
        occurred_at: entry.occurred_at.clone(),
        event_id: entry.event_id.as_str().to_owned(),
    }
}

fn page_error(error: super::event_history_page::PageError) -> ApplyError {
    match error {
        super::event_history_page::PageError::Invalid(message) => ApplyError::Invalid(message),
    }
}

fn invalid(message: &str) -> ApplyError {
    ApplyError::Invalid(message.to_owned())
}

#[cfg(test)]
mod tests {
    use pointbreak::crypto::EventVerificationStatus;
    use pointbreak::documents::{
        EventHistoryCompletionV1, EventHistoryDocumentV1, EventHistoryEntryV1, EventHistoryOrderV1,
        EventHistorySubjectV1, EventHistorySummaryV1, INSPECT_EVENT_HISTORY_SCHEMA,
    };
    use pointbreak::model::{ChangeId, JournalId, RevisionId, RevisionRefV1, TrackId};
    use pointbreak::session::AuthorityCursorV2;
    use pointbreak::session::event::{AssertionMode, EventType, Writer};

    use super::*;
    use crate::cli::inspect::event_history_page::{DEFAULT_LIMIT, parse_signed};

    fn hash(byte: char) -> String {
        format!("sha256:{}", byte.to_string().repeat(64))
    }

    fn signer() -> PageTokenSigner {
        PageTokenSigner::from_seed([17_u8; 32])
    }

    fn exact() -> RevisionRefV1 {
        RevisionRefV1::new(RevisionId::new("rev:sha256:exact"), hash('e')).unwrap()
    }

    fn document(count: usize) -> EventHistoryDocumentV1 {
        let exact = exact();
        let entries = (0..count)
            .map(|index| EventHistoryEntryV1 {
                event_id: pointbreak::model::EventId::new(format!("evt:sha256:{index:064x}")),
                event_type: if index % 2 == 0 {
                    EventType::ReviewInitialized
                } else {
                    EventType::ReviewObservationRecorded
                },
                occurred_at: format!("unix-ms:{}", 1_000 + index),
                payload_hash: hash('p'),
                journal_id: JournalId::new("journal:timeline-test"),
                track_id: Some(TrackId::new(if index < 4 {
                    "agent:alpha"
                } else {
                    "agent:beta"
                })),
                writer: Writer::shore_local("timeline-test"),
                verification_status: EventVerificationStatus::Unsigned,
                assertion_mode: AssertionMode::Advisory,
                signer: None,
                source_ref: None,
                ingest: None,
                subject: EventHistorySubjectV1::Journal {
                    journal_id: JournalId::new("journal:timeline-test"),
                },
                change_ids: vec![ChangeId::new(if index < 5 {
                    "change:sha256:one"
                } else {
                    "change:sha256:two"
                })],
                revision_refs: (index != 3).then(|| exact.clone()).into_iter().collect(),
                unresolved_revision_ids: (index == 3)
                    .then(|| exact.revision_id.clone())
                    .into_iter()
                    .collect(),
                summary: EventHistorySummaryV1::ReviewInitialized,
            })
            .collect::<Vec<_>>();
        EventHistoryDocumentV1 {
            schema: INSPECT_EVENT_HISTORY_SCHEMA.to_owned(),
            version: 1,
            authority_cursor: AuthorityCursorV2 {
                schema: "pointbreak.authority-cursor.v2".to_owned(),
                journal_record_count: count as u64,
                event_count: count as u64,
                journal_record_set_hash: hash('j'),
                event_set_hash: hash('v'),
                capability_set_hash: hash('c'),
            },
            source_change_projection_stamp: hash('s'),
            timeline_projection_stamp: hash('t'),
            order: EventHistoryOrderV1::Asc,
            event_count: count as u64,
            match_count: count,
            offset: 0,
            match_index: None,
            facets: std::collections::BTreeMap::new(),
            completion: EventHistoryCompletionV1::default(),
            diagnostics: Vec::new(),
            query_notices: Vec::new(),
            entries,
            previous: None,
            next: None,
        }
    }

    fn parse(query: &str) -> Request {
        parse_signed(Some(query), &signer()).unwrap()
    }

    fn ids(document: &EventHistoryDocumentV1) -> Vec<String> {
        document
            .entries
            .iter()
            .map(|entry| entry.event_id.as_str().to_owned())
            .collect()
    }

    fn submit_token(base_query: &str, token: &str) -> Request {
        parse(&format!("{base_query}&after={token}"))
    }

    #[test]
    fn bidirectional_pages_are_adjacent_without_overlap_in_both_orders() {
        for order in ["asc", "desc"] {
            let query = format!("limit=2&order={order}");
            let first = apply(document(5), &parse(&query), &signer()).unwrap();
            assert!(first.previous.is_none());
            assert!(first.next.is_some());

            let second = apply(
                document(5),
                &submit_token(&query, first.next.as_deref().unwrap()),
                &signer(),
            )
            .unwrap();
            let third = apply(
                document(5),
                &submit_token(&query, second.next.as_deref().unwrap()),
                &signer(),
            )
            .unwrap();
            let back = apply(
                document(5),
                &submit_token(&query, second.previous.as_deref().unwrap()),
                &signer(),
            )
            .unwrap();

            assert_eq!(ids(&back), ids(&first));
            let all = ids(&first)
                .into_iter()
                .chain(ids(&second))
                .chain(ids(&third))
                .collect::<Vec<_>>();
            assert_eq!(all.len(), 5);
            assert_eq!(
                all.iter().collect::<std::collections::BTreeSet<_>>().len(),
                5
            );
            assert!(third.next.is_none());
        }
    }

    #[test]
    fn at_is_page_aligned_and_can_have_both_neighbors() {
        let target = document(6).entries[2].event_id.as_str().to_owned();
        let page = apply(
            document(6),
            &parse(&format!("limit=2&order=asc&at={target}")),
            &signer(),
        )
        .unwrap();
        assert_eq!(page.offset, 2);
        assert_eq!(page.match_index, Some(2));
        assert_eq!(page.entries[0].event_id.as_str(), target);
        assert!(page.previous.is_some());
        assert!(page.next.is_some());
    }

    #[test]
    fn filters_are_exact_and_facets_exclude_only_the_type_page_set() {
        let exact = exact();
        let query = format!(
            "limit={DEFAULT_LIMIT}&order=asc&track=agent%3Aalpha&change=change%3Asha256%3Aone&revision={}&artifactHash={}&type=review_initialized",
            exact.revision_id.as_str(),
            exact.object_artifact_content_hash,
        );
        let page = apply(document(7), &parse(&query), &signer()).unwrap();
        assert_eq!(page.match_count, 2);
        assert!(page.entries.iter().all(|entry| {
            entry.event_type == EventType::ReviewInitialized
                && entry.revision_refs.contains(&exact)
                && entry
                    .change_ids
                    .contains(&ChangeId::new("change:sha256:one"))
        }));
        assert_eq!(page.facets.get("review_initialized"), Some(&2));
        assert_eq!(page.facets.get("review_observation_recorded"), Some(&1));
    }

    #[test]
    fn unresolved_revision_identity_never_matches_an_exact_filter() {
        let exact = exact();
        let query = format!(
            "order=asc&revision={}&artifactHash={}",
            exact.revision_id.as_str(),
            exact.object_artifact_content_hash,
        );
        let page = apply(document(4), &parse(&query), &signer()).unwrap();
        assert_eq!(page.match_count, 3);
        assert!(
            !page
                .entries
                .iter()
                .any(|entry| { entry.event_id == document(4).entries[3].event_id })
        );
    }

    #[test]
    fn valid_cursor_against_a_different_timeline_stamp_is_stale() {
        let query = "limit=2&order=asc";
        let first = apply(document(3), &parse(query), &signer()).unwrap();
        let request = submit_token(query, first.next.as_deref().unwrap());
        let mut changed = document(3);
        changed.timeline_projection_stamp = hash('n');
        assert_eq!(apply(changed, &request, &signer()), Err(ApplyError::Stale));
    }

    #[test]
    fn missing_locator_is_a_typed_invalid_query() {
        assert!(matches!(
            apply(
                document(3),
                &parse(&format!("at=evt:sha256:{}", "f".repeat(64))),
                &signer(),
            ),
            Err(ApplyError::Invalid(_))
        ));
    }

    #[test]
    fn empty_search_skips_lowercase_search_record_materialization() {
        reset_search_record_build_count();

        let page = apply(document(7), &parse("limit=2&order=asc"), &signer()).unwrap();

        assert_eq!(page.entries.len(), 2);
        assert_eq!(search_record_build_count(), 0);
    }
}
