//! Presentation ordering keys for Change list surfaces.
//!
//! `ChangeOrderingV1` is composition-time input, attached to the Change
//! document facade beside the presentation projection. It is never persisted
//! and never read by Change semantics; it only decides where a Change sits in
//! a list.
//!
//! Two keys, two sources, one rule each:
//!
//! - `activity`: the newest contributing event instant per Change, where
//!   "contributing" is exactly the Timeline's own attribution
//!   (`EventHistoryEntry.changeIds`). The newest Timeline row for `change:X`
//!   is the row that sets X's position in the Changes list. Ties between
//!   equal instants with different spellings break on the winning event id
//!   so the fold is permutation-invariant.
//! - `attention_wait`: tier rank plus the OLDEST `observed_at` over the
//!   attention items anchored on the Change's member Revisions, reusing the
//!   attention projection's own tier and instant semantics.
//!
//! Instant ordering goes through `compare_event_instants` only.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::model::{ChangeId, EventId};
use crate::session::event::ShoreEvent;
use crate::session::{
    AttentionItem, AttentionWaitKeyV1, ChangeDocumentProjectionV1, ChangeProjection,
    attention_from_events, attention_tier_rank, compare_event_instants,
    project_selected_event_history_without_trust,
};

/// Composition-time ordering keys for one Change document generation.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeOrderingV1 {
    /// Newest contributing event instant per Change, raw and unparsed so an
    /// unreadable legacy value survives to the wire intact.
    pub activity: BTreeMap<ChangeId, String>,
    /// Wait-time key per Change with at least one anchored attention item.
    pub attention_wait: BTreeMap<ChangeId, AttentionWaitKeyV1>,
    /// The semantic generation this ordering was computed for.
    pub source_projection_stamp: String,
}

/// One Timeline entry's attribution, the only input the activity fold reads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeActivityContributionV1<'a> {
    pub change_ids: &'a [ChangeId],
    pub occurred_at: &'a str,
    pub event_id: &'a EventId,
}

/// Fold the newest contributing instant per Change. Deterministic under any
/// input order: a candidate replaces the current winner only when it is
/// strictly newer under `compare_event_instants`, or equal and carries the
/// greater event id.
pub fn fold_change_activity<'a>(
    contributions: impl IntoIterator<Item = ChangeActivityContributionV1<'a>>,
) -> BTreeMap<ChangeId, String> {
    // Track (instant, event id) so ties break deterministically; emit only
    // the raw instant.
    let mut best: BTreeMap<ChangeId, (&'a str, &'a EventId)> = BTreeMap::new();
    for contribution in contributions {
        for change_id in contribution.change_ids {
            let candidate = (contribution.occurred_at, contribution.event_id);
            match best.get_mut(change_id) {
                Some(current) => {
                    let ordering = compare_event_instants(candidate.0, current.0)
                        .then_with(|| candidate.1.as_str().cmp(current.1.as_str()));
                    if ordering.is_gt() {
                        *current = candidate;
                    }
                }
                None => {
                    best.insert(change_id.clone(), candidate);
                }
            }
        }
    }
    best.into_iter()
        .map(|(change_id, (instant, _))| (change_id, instant.to_owned()))
        .collect()
}

/// Tier rank then oldest anchored `observed_at` per Change, over the items
/// whose anchoring Revision is one of the Change's members. Thread-scoped
/// items (no anchoring Revision) contribute to no Change.
pub fn attention_wait_keys(
    items: &[AttentionItem],
    semantic: &ChangeProjection,
) -> BTreeMap<ChangeId, AttentionWaitKeyV1> {
    let mut keys: BTreeMap<ChangeId, AttentionWaitKeyV1> = BTreeMap::new();
    for item in items {
        let Some(revision_id) = &item.revision_id else {
            continue;
        };
        let candidate = AttentionWaitKeyV1 {
            tier_rank: attention_tier_rank(item.tier),
            oldest_observed_at: item.observed_at.clone(),
        };
        for view in semantic.changes.values() {
            if !view.members.contains(revision_id) {
                continue;
            }
            match keys.get_mut(&view.change_id) {
                Some(current) => {
                    // Tier first, then the OLDEST instant under the shared
                    // comparator: longest-waiting-first means oldest-first.
                    let ordering = candidate.tier_rank.cmp(&current.tier_rank).then_with(|| {
                        compare_event_instants(
                            &candidate.oldest_observed_at,
                            &current.oldest_observed_at,
                        )
                    });
                    if ordering.is_lt() {
                        *current = candidate.clone();
                    }
                }
                None => {
                    keys.insert(view.change_id.clone(), candidate.clone());
                }
            }
        }
    }
    keys
}

/// The authoritative lane: both keys from the validated event generation,
/// attribution taken from the Timeline projection itself.
pub(crate) fn change_ordering_projection(
    semantic: &ChangeProjection,
    provenance: &ChangeDocumentProjectionV1,
    events: &[ShoreEvent],
) -> Result<ChangeOrderingV1> {
    // Attribution is the Timeline's own: the same projection that fills each
    // entry's `changeIds` supplies the activity contributions, so the two
    // surfaces cannot disagree about which events belong to a Change.
    let (drafts, _diagnostics) = project_selected_event_history_without_trust(events, provenance)?;
    let activity = fold_change_activity(drafts.iter().map(|draft| {
        let entry = draft.entry();
        ChangeActivityContributionV1 {
            change_ids: &entry.change_ids,
            occurred_at: &entry.occurred_at,
            event_id: &entry.event_id,
        }
    }));
    let attention = attention_from_events(events, None)?;
    Ok(ChangeOrderingV1 {
        activity,
        attention_wait: attention_wait_keys(&attention.items, semantic),
        source_projection_stamp: provenance.projection_stamp.clone(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::model::RevisionId;
    use crate::session::{
        AttentionDetail, AttentionFreshness, AttentionFreshnessState, AttentionTier,
        ChangeLifecycleV1, ChangeTopologyV1, ChangeView,
    };

    fn change(id: &str) -> ChangeId {
        ChangeId::new(id)
    }

    fn contribution<'a>(
        change_ids: &'a [ChangeId],
        occurred_at: &'a str,
        event_id: &'a EventId,
    ) -> ChangeActivityContributionV1<'a> {
        ChangeActivityContributionV1 {
            change_ids,
            occurred_at,
            event_id,
        }
    }

    #[test]
    fn activity_is_the_newest_contributing_event_instant() {
        let one = [change("change:sha256:one")];
        let two = [change("change:sha256:two")];
        let (a, b, c) = (
            EventId::new("ev:a"),
            EventId::new("ev:b"),
            EventId::new("ev:c"),
        );
        let activity = fold_change_activity([
            contribution(&one, "2026-09-12T18:04:00.000Z", &a),
            contribution(&one, "2026-09-12T19:20:00.000Z", &b),
            contribution(&two, "2026-09-12T18:31:00.000Z", &c),
        ]);
        assert_eq!(
            activity.get(&one[0]).map(String::as_str),
            Some("2026-09-12T19:20:00.000Z")
        );
        assert_eq!(
            activity.get(&two[0]).map(String::as_str),
            Some("2026-09-12T18:31:00.000Z")
        );
    }

    #[test]
    fn newest_is_chosen_by_the_shared_instant_comparator_not_lexically() {
        let one = [change("change:sha256:one")];
        let (a, b) = (EventId::new("ev:a"), EventId::new("ev:b"));
        // unix-ms:1789000000000 is 2026-09-10; a lexical max would pick it.
        let activity = fold_change_activity([
            contribution(&one, "unix-ms:1789000000000", &a),
            contribution(&one, "2026-09-12T18:04:00.000Z", &b),
        ]);
        assert_eq!(
            activity.get(&one[0]).map(String::as_str),
            Some("2026-09-12T18:04:00.000Z")
        );
    }

    #[test]
    fn an_unparseable_instant_never_displaces_a_parseable_one() {
        let one = [change("change:sha256:one")];
        let (a, b) = (EventId::new("ev:a"), EventId::new("ev:b"));
        let activity = fold_change_activity([
            contribution(&one, "2026-09-12T18:04:00.000Z", &a),
            contribution(&one, "not-an-instant", &b),
        ]);
        assert_eq!(
            activity.get(&one[0]).map(String::as_str),
            Some("2026-09-12T18:04:00.000Z")
        );
    }

    #[test]
    fn an_event_contributing_to_several_changes_updates_each() {
        let both = [change("change:sha256:one"), change("change:sha256:two")];
        let a = EventId::new("ev:a");
        let activity = fold_change_activity([contribution(&both, "2026-09-12T20:00:00.000Z", &a)]);
        assert_eq!(activity.len(), 2);
    }

    #[test]
    fn a_change_with_no_contributing_event_has_no_entry() {
        assert!(fold_change_activity(std::iter::empty()).is_empty());
    }

    #[test]
    fn equal_instants_with_different_spellings_fold_order_independently() {
        let one = [change("change:sha256:one")];
        let (a, b) = (EventId::new("ev:a"), EventId::new("ev:b"));
        let forward = fold_change_activity([
            contribution(&one, "unix-ms:0", &b),
            contribution(&one, "1970-01-01T00:00:00.000Z", &a),
        ]);
        let reversed = fold_change_activity([
            contribution(&one, "1970-01-01T00:00:00.000Z", &a),
            contribution(&one, "unix-ms:0", &b),
        ]);
        assert_eq!(forward, reversed);
        // The greater event id (ev:b) wins the tie, whichever arrives first.
        assert_eq!(forward.get(&one[0]).map(String::as_str), Some("unix-ms:0"));
    }

    fn view(change_id: &str, members: &[&str]) -> ChangeView {
        let members = members
            .iter()
            .map(|id| RevisionId::new(*id))
            .collect::<BTreeSet<_>>();
        ChangeView {
            change_id: change(change_id),
            members: members.clone(),
            current_revisions: members,
            supersedes: BTreeSet::new(),
            topology: ChangeTopologyV1::Initial,
            lifecycle: ChangeLifecycleV1::InProgress,
            qualified_current_revisions: BTreeSet::new(),
            operative_obligations: BTreeSet::new(),
            diagnostics: Vec::new(),
        }
    }

    fn item(
        id: &str,
        tier: AttentionTier,
        revision_id: Option<&str>,
        observed_at: &str,
    ) -> AttentionItem {
        AttentionItem {
            id: id.to_owned(),
            tier,
            revision_id: revision_id.map(RevisionId::new),
            freshness: AttentionFreshness {
                state: AttentionFreshnessState::Current,
                superseded_by: Vec::new(),
            },
            observed_at: observed_at.to_owned(),
            detail: AttentionDetail::CompetingHeads {
                head_revision_ids: Vec::new(),
                thread_revision_count: 0,
            },
        }
    }

    #[test]
    fn attention_wait_takes_tier_then_oldest_anchored_item_per_change() {
        let semantic = ChangeProjection {
            changes: [
                (
                    change("change:sha256:one"),
                    view("change:sha256:one", &["rev:sha256:a", "rev:sha256:b"]),
                ),
                (
                    change("change:sha256:two"),
                    view("change:sha256:two", &["rev:sha256:c"]),
                ),
                (
                    change("change:sha256:idle"),
                    view("change:sha256:idle", &["rev:sha256:d"]),
                ),
            ]
            .into(),
            links: Vec::new(),
        };
        let items = [
            item(
                "s:1",
                AttentionTier::Secondary,
                Some("rev:sha256:a"),
                "2026-08-01T00:00:00.000Z",
            ),
            item(
                "p:1",
                AttentionTier::Primary,
                Some("rev:sha256:b"),
                "2026-09-02T00:00:00.000Z",
            ),
            item(
                "p:2",
                AttentionTier::Primary,
                Some("rev:sha256:a"),
                "unix-ms:1756700000000",
            ),
            item(
                "p:3",
                AttentionTier::Primary,
                Some("rev:sha256:c"),
                "2026-09-05T00:00:00.000Z",
            ),
            item(
                "thread",
                AttentionTier::Primary,
                None,
                "2026-01-01T00:00:00.000Z",
            ),
            item(
                "orphan",
                AttentionTier::Primary,
                Some("rev:sha256:zz"),
                "2026-01-01T00:00:00.000Z",
            ),
        ];
        let keys = attention_wait_keys(&items, &semantic);
        // Primary outranks the older secondary; within primary the oldest
        // instant wins under the shared comparator, not lexically.
        assert_eq!(
            keys.get(&change("change:sha256:one")),
            Some(&AttentionWaitKeyV1 {
                tier_rank: 0,
                oldest_observed_at: "unix-ms:1756700000000".to_owned(),
            })
        );
        assert_eq!(
            keys.get(&change("change:sha256:two")),
            Some(&AttentionWaitKeyV1 {
                tier_rank: 0,
                oldest_observed_at: "2026-09-05T00:00:00.000Z".to_owned(),
            })
        );
        assert!(!keys.contains_key(&change("change:sha256:idle")));
        assert_eq!(keys.len(), 2);
    }
}
