//! The supersession views item-level attention reads.
//!
//! Replacement authority is an exact, Change-scoped relation claim (ADR-0042);
//! a proposal-borne `supersedes` list survives only as historical migration
//! input. Once a store holds any Change claim, attention therefore derives
//! freshness from effective Change relations alone, so no sequence of relation
//! or membership withdrawals can revive a historical edge. A store that holds
//! no Change claim reads its proposal-borne edges exactly as before.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::RevisionId;
use crate::session::projection::change::{ChangeProjection, ChangeTopologyV1, ChangeView};
use crate::session::projection::supersession::{SUPERSESSION_CYCLE_CODE, SupersessionView};

/// The two graphs the attention collectors read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AttentionSupersession {
    /// Replacement authority: freshness, heads, thread heads, the suppression
    /// rules, and `--revision` scoping.
    pub(crate) replacement: SupersessionView,
    /// The graph competing-heads items are read from. It carries no edges once
    /// the store holds a Change claim: divergence inside a Change surfaces
    /// through the Change lifecycle, and two Changes that independently replace
    /// a shared Revision do not compete.
    pub(crate) competition: SupersessionView,
}

/// A Change whose current set can be trusted: an incomplete or cyclic Change
/// says nothing reliable about which member is current.
fn is_usable(view: &ChangeView) -> bool {
    !matches!(
        view.topology,
        ChangeTopologyV1::Incomplete | ChangeTopologyV1::CycleConflicted
    )
}

/// Builds the attention views from the proposal-borne view and the store-wide
/// Change projection.
///
/// A Change relation `(successor, predecessor)` supersedes the predecessor only
/// when the Change is usable, both endpoints are active members and known
/// Revisions, and the predecessor is non-current in every Change that holds it
/// (an unusable Change counts as holding it current). A Revision that is still
/// a live candidate anywhere keeps asking for judgment.
pub(crate) fn change_aware_supersession(
    legacy: &SupersessionView,
    changes: &ChangeProjection,
) -> AttentionSupersession {
    if changes.changes.is_empty() {
        return AttentionSupersession {
            replacement: legacy.clone(),
            competition: legacy.clone(),
        };
    }

    let known: BTreeSet<&RevisionId> = legacy.components.iter().flatten().collect();
    let mut holding: BTreeMap<&RevisionId, Vec<&ChangeView>> = BTreeMap::new();
    for view in changes.changes.values() {
        for member in &view.members {
            holding.entry(member).or_default().push(view);
        }
    }
    let current_somewhere = |revision: &RevisionId| {
        holding.get(revision).is_some_and(|views| {
            views
                .iter()
                .any(|view| !is_usable(view) || view.current_revisions.contains(revision))
        })
    };

    let mut edges: BTreeMap<&RevisionId, BTreeSet<&RevisionId>> = known
        .iter()
        .map(|revision| (*revision, BTreeSet::new()))
        .collect();
    for view in changes.changes.values().filter(|view| is_usable(view)) {
        for (successor, predecessor) in &view.supersedes {
            let admitted = view.members.contains(successor)
                && view.members.contains(predecessor)
                && known.contains(successor)
                && known.contains(predecessor)
                && !current_somewhere(predecessor);
            if admitted {
                edges.entry(successor).or_default().insert(predecessor);
            }
        }
    }

    let mut replacement = SupersessionView::from_edges(
        edges
            .into_iter()
            .map(|(revision, targets)| (revision.clone(), targets.into_iter().cloned().collect())),
    );
    // Every admitted edge comes from a usable, and therefore acyclic, Change, so
    // a loop here is only ever two valid histories crossing (one Change reads
    // a -> b -> c, another b -> a -> c): it is not a conflicted Change, and a
    // genuinely cyclic Change contributed nothing above. It cannot make a thread
    // headless either, because a Revision is superseded only when every Change
    // holding it has moved on, and each of those Changes ends at a Revision it
    // holds current, which nothing supersedes. The cycle guard exists to refuse
    // suppression when heads are unreliable; here they are reliable, so the
    // union must not raise it or report a cycle nobody can repair.
    replacement.cycle_revisions.clear();
    replacement
        .diagnostics
        .retain(|diagnostic| diagnostic.code != SUPERSESSION_CYCLE_CODE);

    AttentionSupersession {
        replacement,
        competition: SupersessionView::from_edges(
            known
                .into_iter()
                .map(|revision| (revision.clone(), Vec::new())),
        ),
    }
}

/// Every Revision connected to `seed` through relation edges of usable Changes,
/// followed in both directions across the whole store. Thread heads are
/// component-wide, so a scoped attention fold needs the facts of this whole set;
/// the result always contains the seed and deliberately over-includes.
pub(crate) fn change_connected_revisions(
    seed: &BTreeSet<RevisionId>,
    changes: &ChangeProjection,
) -> BTreeSet<RevisionId> {
    let mut neighbors: BTreeMap<&RevisionId, BTreeSet<&RevisionId>> = BTreeMap::new();
    for view in changes.changes.values().filter(|view| is_usable(view)) {
        for (successor, predecessor) in &view.supersedes {
            neighbors.entry(successor).or_default().insert(predecessor);
            neighbors.entry(predecessor).or_default().insert(successor);
        }
    }
    let mut connected = seed.clone();
    let mut frontier: Vec<RevisionId> = seed.iter().cloned().collect();
    while let Some(revision) = frontier.pop() {
        for neighbor in neighbors.get(&revision).into_iter().flatten() {
            if connected.insert((*neighbor).clone()) {
                frontier.push((*neighbor).clone());
            }
        }
    }
    connected
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ChangeId;
    use crate::session::projection::change::ChangeLifecycleV1;

    fn rev(suffix: &str) -> RevisionId {
        RevisionId::new(format!("rev:sha256:{suffix}"))
    }

    fn roots(suffixes: &[&str]) -> SupersessionView {
        SupersessionView::from_edges(suffixes.iter().map(|suffix| (rev(suffix), Vec::new())))
    }

    /// A usable Change: `members` plus `(successor, predecessor)` edges, with the
    /// current set derived as "members no edge replaces".
    fn change(id: &str, members: &[&str], edges: &[(&str, &str)]) -> ChangeView {
        change_with_topology(id, members, edges, ChangeTopologyV1::Replacement)
    }

    fn change_with_topology(
        id: &str,
        members: &[&str],
        edges: &[(&str, &str)],
        topology: ChangeTopologyV1,
    ) -> ChangeView {
        let members: BTreeSet<RevisionId> = members.iter().map(|suffix| rev(suffix)).collect();
        let supersedes: BTreeSet<(RevisionId, RevisionId)> = edges
            .iter()
            .map(|(successor, predecessor)| (rev(successor), rev(predecessor)))
            .collect();
        let replaced: BTreeSet<&RevisionId> = supersedes
            .iter()
            .map(|(_, predecessor)| predecessor)
            .collect();
        let current_revisions = members
            .iter()
            .filter(|member| !replaced.contains(member))
            .cloned()
            .collect();
        ChangeView {
            change_id: ChangeId::new(format!("change:sha256:{id}")),
            members,
            current_revisions,
            supersedes,
            topology,
            lifecycle: ChangeLifecycleV1::InProgress,
            qualified_current_revisions: BTreeSet::new(),
            operative_obligations: BTreeSet::new(),
            diagnostics: Vec::new(),
        }
    }

    fn projection(views: Vec<ChangeView>) -> ChangeProjection {
        ChangeProjection {
            changes: views
                .into_iter()
                .map(|view| (view.change_id.clone(), view))
                .collect(),
            links: Vec::new(),
        }
    }

    fn set(suffixes: &[&str]) -> BTreeSet<RevisionId> {
        suffixes.iter().map(|suffix| rev(suffix)).collect()
    }

    fn has_competing_heads(view: &SupersessionView) -> bool {
        view.components
            .iter()
            .any(|component| component.intersection(&view.heads).count() > 1)
    }

    #[test]
    fn replacement_claim_supersedes_the_predecessor() {
        let views = change_aware_supersession(
            &roots(&["a", "b"]),
            &projection(vec![change("x", &["a", "b"], &[("b", "a")])]),
        );
        assert_eq!(views.replacement.heads, set(&["b"]));
        assert_eq!(
            views.replacement.stale_by_superseding_revision(&rev("a")),
            set(&["b"])
        );
        assert_eq!(views.replacement.components, vec![set(&["a", "b"])]);
        assert_eq!(views.competition.heads, set(&["a", "b"]));
        assert_eq!(views.competition.components.len(), 2);
    }

    #[test]
    fn a_revision_current_in_any_change_keeps_its_head() {
        let views = change_aware_supersession(
            &roots(&["a", "b"]),
            &projection(vec![
                change("x", &["a"], &[]),
                change("y", &["a", "b"], &[("b", "a")]),
            ]),
        );
        assert_eq!(views.replacement.heads, set(&["a", "b"]));
    }

    #[test]
    fn an_unusable_holding_change_counts_as_current() {
        for topology in [
            ChangeTopologyV1::Incomplete,
            ChangeTopologyV1::CycleConflicted,
        ] {
            let views = change_aware_supersession(
                &roots(&["a", "b"]),
                &projection(vec![
                    change_with_topology("x", &["a"], &[], topology),
                    change("y", &["a", "b"], &[("b", "a")]),
                ]),
            );
            assert_eq!(views.replacement.heads, set(&["a", "b"]), "{topology:?}");
        }
    }

    #[test]
    fn unusable_changes_contribute_no_edges() {
        let legacy = SupersessionView::from_edges([(rev("a"), vec![]), (rev("b"), vec![rev("a")])]);
        for topology in [
            ChangeTopologyV1::Incomplete,
            ChangeTopologyV1::CycleConflicted,
        ] {
            let views = change_aware_supersession(
                &legacy,
                &projection(vec![change_with_topology(
                    "x",
                    &["a", "b"],
                    &[("b", "a")],
                    topology,
                )]),
            );
            assert_eq!(views.replacement.heads, set(&["a", "b"]), "{topology:?}");
            assert!(views.replacement.diagnostics.is_empty());
        }
    }

    #[test]
    fn proposal_borne_edges_lose_authority_once_any_change_claim_exists() {
        let legacy = SupersessionView::from_edges([
            (rev("a"), vec![]),
            (rev("b"), vec![rev("a")]),
            (rev("z"), vec![]),
        ]);
        let views = change_aware_supersession(&legacy, &projection(vec![change("x", &["z"], &[])]));
        assert_eq!(views.replacement.heads, set(&["a", "b", "z"]));
        assert_eq!(views.competition.heads, set(&["a", "b", "z"]));
    }

    #[test]
    fn withdrawals_cannot_revive_a_proposal_borne_edge() {
        // The shape left after withdrawing a migrated relation and then the
        // predecessor's last membership: the Change still exists, without `a`.
        let legacy = SupersessionView::from_edges([(rev("a"), vec![]), (rev("b"), vec![rev("a")])]);
        let views = change_aware_supersession(&legacy, &projection(vec![change("x", &["b"], &[])]));
        assert_eq!(views.replacement.heads, set(&["a", "b"]));
        assert_eq!(views.competition.heads, set(&["a", "b"]));
    }

    #[test]
    fn divergent_replacement_names_every_successor() {
        let views = change_aware_supersession(
            &roots(&["a", "b", "c"]),
            &projection(vec![change_with_topology(
                "x",
                &["a", "b", "c"],
                &[("b", "a"), ("c", "a")],
                ChangeTopologyV1::ReplacementDivergent,
            )]),
        );
        assert_eq!(
            views.replacement.stale_by_superseding_revision(&rev("a")),
            set(&["b", "c"])
        );
        assert!(!has_competing_heads(&views.competition));
    }

    #[test]
    fn two_changes_replacing_a_shared_revision_do_not_compete() {
        let views = change_aware_supersession(
            &roots(&["a", "b", "c"]),
            &projection(vec![
                change("x", &["a", "b"], &[("b", "a")]),
                change("y", &["a", "c"], &[("c", "a")]),
            ]),
        );
        assert_eq!(
            views.replacement.stale_by_superseding_revision(&rev("a")),
            set(&["b", "c"])
        );
        assert_eq!(views.replacement.heads_for(&rev("a")), set(&["b", "c"]));
        assert!(!has_competing_heads(&views.competition));
        assert_eq!(views.competition.components.len(), 3);
    }

    #[test]
    fn consolidation_supersedes_every_predecessor() {
        let views = change_aware_supersession(
            &roots(&["a", "b", "c"]),
            &projection(vec![change_with_topology(
                "x",
                &["a", "b", "c"],
                &[("c", "a"), ("c", "b")],
                ChangeTopologyV1::Consolidation,
            )]),
        );
        assert_eq!(views.replacement.heads, set(&["c"]));
    }

    #[test]
    fn edges_naming_unknown_revisions_are_dropped_without_diagnostics() {
        let legacy = roots(&["a"]);
        let views = change_aware_supersession(
            &legacy,
            &projection(vec![change("x", &["a", "ghost"], &[("ghost", "a")])]),
        );
        assert_eq!(views.replacement, legacy);
        assert!(views.replacement.diagnostics.is_empty());
    }

    #[test]
    fn a_legacy_fork_with_a_change_replacing_one_head_names_no_superseded_head() {
        let legacy = SupersessionView::from_edges([
            (rev("a"), vec![]),
            (rev("b"), vec![rev("a")]),
            (rev("c"), vec![rev("a")]),
            (rev("d"), vec![]),
        ]);
        let views = change_aware_supersession(
            &legacy,
            &projection(vec![change("x", &["b", "d"], &[("d", "b")])]),
        );
        assert!(!has_competing_heads(&views.competition));
        assert_eq!(
            views.replacement.stale_by_superseding_revision(&rev("b")),
            set(&["d"])
        );
        assert_eq!(views.replacement.heads, set(&["a", "c", "d"]));
    }

    #[test]
    fn crossed_change_histories_do_not_read_as_a_cycle() {
        // Each Change is acyclic and ends at `c`; only their union loops a <-> b.
        let views = change_aware_supersession(
            &roots(&["a", "b", "c"]),
            &projection(vec![
                change("x", &["a", "b", "c"], &[("b", "a"), ("c", "b")]),
                change("y", &["a", "b", "c"], &[("a", "b"), ("c", "a")]),
            ]),
        );
        assert_eq!(views.replacement.heads, set(&["c"]));
        assert_eq!(
            views.replacement.stale_by_superseding_revision(&rev("a")),
            set(&["b", "c"])
        );
        assert_eq!(
            views.replacement.stale_by_superseding_revision(&rev("b")),
            set(&["a", "c"])
        );
        assert_eq!(views.replacement.heads_for(&rev("a")), set(&["c"]));
        assert!(views.replacement.cycle_revisions.is_empty());
        assert!(views.replacement.diagnostics.is_empty());
    }

    #[test]
    fn a_store_without_change_claims_returns_the_legacy_view_twice() {
        let legacy = SupersessionView::from_edges([
            (rev("a"), vec![]),
            (rev("b"), vec![rev("a")]),
            (rev("c"), vec![rev("a"), rev("ghost")]),
        ]);
        assert!(!legacy.diagnostics.is_empty());
        let views = change_aware_supersession(&legacy, &ChangeProjection::default());
        assert_eq!(views.replacement, legacy);
        assert_eq!(views.competition, legacy);
        assert!(has_competing_heads(&views.competition));
    }

    #[test]
    fn result_is_deterministic_under_change_iteration_order() {
        let legacy = roots(&["a", "b"]);
        let forward = change_aware_supersession(
            &legacy,
            &projection(vec![
                change("x", &["a", "b"], &[("b", "a")]),
                change("y", &["a", "b"], &[("b", "a")]),
            ]),
        );
        let reverse = change_aware_supersession(
            &legacy,
            &projection(vec![
                change("y", &["a", "b"], &[("b", "a")]),
                change("x", &["a", "b"], &[("b", "a")]),
            ]),
        );
        assert_eq!(forward, reverse);
        assert_eq!(forward.replacement.heads, set(&["b"]));
    }

    #[test]
    fn change_connected_revisions_follow_usable_relations_in_both_directions() {
        let changes = projection(vec![
            change("x", &["a", "b"], &[("b", "a")]),
            change("y", &["a", "c"], &[("c", "a")]),
            change("z", &["c", "d"], &[("d", "c")]),
            change("unrelated", &["p", "q"], &[("q", "p")]),
            change_with_topology(
                "broken",
                &["a", "e"],
                &[("e", "a")],
                ChangeTopologyV1::Incomplete,
            ),
        ]);
        assert_eq!(
            change_connected_revisions(&set(&["a"]), &changes),
            set(&["a", "b", "c", "d"])
        );
        assert_eq!(
            change_connected_revisions(&set(&["d"]), &changes),
            set(&["a", "b", "c", "d"])
        );
        assert_eq!(
            change_connected_revisions(&set(&["lonely"]), &changes),
            set(&["lonely"])
        );
    }
}
