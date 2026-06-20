//! Read-time inversion of the commit-range projection: `commit_oid → {review_unit_ids}`.
//!
//! Under the shared store two worktrees that capture the same range mint distinct
//! `review_unit_id`s (the identity fold folds the per-worktree `source_repo_namespace`,
//! `fingerprint.rs`), but their current commit sets converge on the same OID(s). This
//! view recovers that convergence so a read surface can present co-grouped units as one.
//! It is a pure projection — no git, no store reads, no re-identification. Reachability
//! (merged/live/orphaned) is a separate read-time enrichment and never enters this view.
//! The identity fold is deliberately left untouched; a floating capture (no current
//! commit OID) has no grouping key and stays un-grouped.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::error::Result;
use crate::model::ReviewUnitId;
use crate::session::event::ShoreEvent;
use crate::session::projection::commit_range::ReviewUnitCommitRangeProjection;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitOidGroupingProjection {
    /// commit OID → the set of review units whose CURRENT commit set contains it.
    /// Only OIDs claimed by at least one unit appear; an OID claimed by exactly
    /// one unit is still recorded (a singleton group), so callers get a uniform
    /// "what units claim this commit?" answer.
    pub groups: BTreeMap<String, BTreeSet<ReviewUnitId>>,
}

impl CommitOidGroupingProjection {
    pub fn from_events(events: &[ShoreEvent]) -> Result<Self> {
        let commit_range = ReviewUnitCommitRangeProjection::from_events(events)?;
        let mut groups: BTreeMap<String, BTreeSet<ReviewUnitId>> = BTreeMap::new();
        for (review_unit_id, view) in &commit_range.units {
            for current in &view.current_commits {
                // Distinct ids sharing one OID is the designed cross-worktree outcome,
                // not a bug to dedup away at identity time — the identity fold stays put.
                groups
                    .entry(current.commit_oid.clone())
                    .or_default()
                    .insert(review_unit_id.clone());
            }
        }
        Ok(Self { groups })
    }

    /// The review units whose current commit set includes `commit_oid`, or `None`
    /// when no unit currently claims it (withdrawn, or never associated).
    pub fn group_for(&self, commit_oid: &str) -> Option<&BTreeSet<ReviewUnitId>> {
        self.groups.get(commit_oid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        CommitRangeCaptureMode, ReviewEndpoint, ReviewTargetRef, ReviewUnitId, ReviewUnitSource,
        RevisionId, SessionId, SnapshotId, WorktreeCaptureMode,
    };
    use crate::session::event::{
        EventTarget, EventType, ReviewUnitCapturedPayload, ReviewUnitCommitAssociatedPayload,
        ReviewUnitCommitWithdrawnPayload, ShoreEvent, Writer, build_commit_association_id,
        build_commit_withdrawal_id,
    };

    fn envelope(unit: &ReviewUnitId) -> EventTarget {
        EventTarget::for_review_unit(
            SessionId::new("session:default"),
            unit.clone(),
            RevisionId::new("rev:git:sha256:def"),
            SnapshotId::new("snap:git:sha256:ghi"),
        )
    }

    fn capture_for(
        unit: &ReviewUnitId,
        target: ReviewEndpoint,
        source: ReviewUnitSource,
    ) -> ShoreEvent {
        ShoreEvent::new(
            EventType::ReviewUnitCaptured,
            format!("review_unit_captured:{}", unit.as_str()),
            envelope(unit),
            Writer::shore_local("test"),
            ReviewUnitCapturedPayload {
                review_unit_id: unit.clone(),
                source,
                base: ReviewEndpoint::GitCommit {
                    commit_oid: "base".to_owned(),
                    tree_oid: "base-tree".to_owned(),
                },
                target,
                revision_id: RevisionId::new("rev:git:sha256:def"),
                snapshot_id: SnapshotId::new("snap:git:sha256:ghi"),
                snapshot_artifact_content_hash: "sha256:artifact".to_owned(),
            },
            "2026-06-19T00:00:00Z",
        )
        .unwrap()
    }

    fn worktree_capture_for(unit: &ReviewUnitId) -> ShoreEvent {
        capture_for(
            unit,
            ReviewEndpoint::GitWorkingTree {
                worktree_root: "/repo".to_owned(),
            },
            ReviewUnitSource::GitWorktree {
                mode: WorktreeCaptureMode::CombinedHeadToWorkingTree,
                include_untracked: true,
            },
        )
    }

    fn commit_range_capture_for(
        unit: &ReviewUnitId,
        commit_oid: &str,
        tree_oid: &str,
    ) -> ShoreEvent {
        capture_for(
            unit,
            ReviewEndpoint::GitCommit {
                commit_oid: commit_oid.to_owned(),
                tree_oid: tree_oid.to_owned(),
            },
            ReviewUnitSource::GitCommitRange {
                mode: CommitRangeCaptureMode::BaseTreeToTargetTree,
            },
        )
    }

    fn commit_associated_for(unit: &ReviewUnitId, commit_oid: &str) -> ShoreEvent {
        let cid = build_commit_association_id(unit, commit_oid).unwrap();
        ShoreEvent::new(
            EventType::ReviewUnitCommitAssociated,
            ReviewUnitCommitAssociatedPayload::idempotency_key(unit, commit_oid),
            envelope(unit),
            Writer::shore_local("test"),
            ReviewUnitCommitAssociatedPayload {
                commit_association_id: cid,
                target: ReviewTargetRef::ReviewUnit {
                    review_unit_id: unit.clone(),
                },
                commit: ReviewEndpoint::GitCommit {
                    commit_oid: commit_oid.to_owned(),
                    tree_oid: format!("{commit_oid}-tree"),
                },
            },
            "2026-06-19T00:00:01Z",
        )
        .unwrap()
    }

    fn commit_withdrawn_for(unit: &ReviewUnitId, commit_oid: &str) -> ShoreEvent {
        let cid = build_commit_association_id(unit, commit_oid).unwrap();
        let wid = build_commit_withdrawal_id(unit, &cid).unwrap();
        ShoreEvent::new(
            EventType::ReviewUnitCommitWithdrawn,
            ReviewUnitCommitWithdrawnPayload::idempotency_key(&cid),
            envelope(unit),
            Writer::shore_local("test"),
            ReviewUnitCommitWithdrawnPayload {
                commit_withdrawal_id: wid,
                target: ReviewTargetRef::ReviewUnit {
                    review_unit_id: unit.clone(),
                },
                commit_association_id: cid,
            },
            "2026-06-19T00:00:02Z",
        )
        .unwrap()
    }

    #[test]
    fn two_units_sharing_a_commit_oid_group_together() {
        // Two distinct review_unit_ids whose current sets both contain "oidShared"
        // collapse into one grouping key. (Models the cross-worktree same-range case:
        // two units, one shared OID — no re-ID.)
        let unit_a = ReviewUnitId::new("review-unit:sha256:a");
        let unit_b = ReviewUnitId::new("review-unit:sha256:b");
        let events = [
            worktree_capture_for(&unit_a),
            commit_associated_for(&unit_a, "oidShared"),
            worktree_capture_for(&unit_b),
            commit_associated_for(&unit_b, "oidShared"),
        ];

        let grouping = CommitOidGroupingProjection::from_events(&events).unwrap();
        let group = grouping
            .group_for("oidShared")
            .expect("oidShared is grouped");

        assert_eq!(group.len(), 2);
        assert!(group.contains(&unit_a));
        assert!(group.contains(&unit_b));
    }

    #[test]
    fn capture_target_seed_groups_without_an_association_event() {
        // The primary cross-worktree case: a commit-range capture is born anchored at its
        // target commit (source = CaptureTarget). Its OID groups with NO association event.
        let unit = ReviewUnitId::new("review-unit:sha256:seed");
        let events = [commit_range_capture_for(&unit, "oidSeed", "oidSeed-tree")];

        let grouping = CommitOidGroupingProjection::from_events(&events).unwrap();
        let group = grouping.group_for("oidSeed").expect("seed OID is grouped");

        assert_eq!(group.len(), 1);
        assert!(group.contains(&unit));
    }

    #[test]
    fn a_floating_unit_stays_ungrouped() {
        // A worktree capture with no commit anchor contributes no grouping key.
        let unit = ReviewUnitId::new("review-unit:sha256:floating");
        let events = [worktree_capture_for(&unit)];

        let grouping = CommitOidGroupingProjection::from_events(&events).unwrap();

        assert!(grouping.groups.is_empty());
        // No key resolves to the floating unit.
        assert!(
            grouping
                .groups
                .values()
                .all(|members| !members.contains(&unit))
        );
    }

    #[test]
    fn a_withdrawn_commit_drops_from_its_group() {
        // associate then withdraw the SAME oid: the OID leaves the unit's current set,
        // so the grouping key disappears (no member, and the key is dropped entirely
        // since the inversion only walks `current_commits`).
        let unit = ReviewUnitId::new("review-unit:sha256:withdrawn");
        let events = [
            worktree_capture_for(&unit),
            commit_associated_for(&unit, "oidGone"),
            commit_withdrawn_for(&unit, "oidGone"),
        ];

        let grouping = CommitOidGroupingProjection::from_events(&events).unwrap();

        assert!(grouping.group_for("oidGone").is_none());
    }
}
