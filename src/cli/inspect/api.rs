//! JSON payload builders for the inspector server.
//!
//! Each builder reuses a public `shoreline::session` projection so the
//! inspector reads the store through the same validated path as the
//! corresponding `shore review` command, rather than parsing raw `.shore/`
//! files. Errors are stringified so the server can surface them to the UI as
//! a JSON `error` body instead of crashing a connection thread.

use std::path::Path;

use serde::Serialize;
use shoreline::model::{ReviewEndpoint, ReviewUnitId, SnapshotId};
use shoreline::session::{
    ProjectionDiagnostic, ReviewHistoryEntry, ReviewHistoryOptions, ReviewUnitListEntry,
    ReviewUnitListOptions, ReviewUnitShowOptions, list_review_units, read_snapshot_artifact,
    review_history, show_review_unit,
};

use crate::cli::review::unit::unit_show_document;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryPayload {
    schema: &'static str,
    event_set_hash: String,
    event_count: usize,
    history_count: usize,
    entries: Vec<ReviewHistoryEntry>,
    diagnostics: Vec<ProjectionDiagnostic>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UnitsPayload {
    schema: &'static str,
    event_set_hash: String,
    event_count: usize,
    review_unit_count: usize,
    entries: Vec<UnitEntryDocument>,
    diagnostics: Vec<ProjectionDiagnostic>,
}

/// One `/api/units` entry: the full `ReviewUnitListEntry` flattened verbatim,
/// plus an additive, path-private `targetDisplay`. `#[serde(flatten)]` keeps
/// every existing field byte-present and unchanged.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UnitEntryDocument {
    #[serde(flatten)]
    entry: ReviewUnitListEntry,
    target_display: TargetDisplay,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FreshnessPayload {
    schema: &'static str,
    event_set_hash: String,
    event_count: usize,
    diagnostic_count: usize,
}

/// The literal floor label shown when no worktree basename can be derived.
const WORKING_TREE_FLOOR: &str = "working tree";
/// Length of the git-style short commit OID used for head labels (git's default).
const SHORT_OID_LEN: usize = 7;

/// Path-private display view-model for a ReviewUnit target.
///
/// Derived at read time from fields already present in a captured unit. The raw
/// worktree path never enters this block: only the final path component (a
/// basename) and a short commit OID are exposed, so the inspector can show a
/// meaningful worktree/head label without leaking absolute paths into its JSON.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TargetDisplay {
    /// `"working_tree"` for a Git working-tree target; otherwise the endpoint kind.
    kind: &'static str,
    /// Basename of the worktree root, or the `"working tree"` floor when none derives.
    label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    head: Option<HeadDisplay>,
    /// Always true: this block is built from path-free fields and never carries
    /// the raw worktree path.
    path_private: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HeadDisplay {
    commit_oid_short: String,
    /// Capture-time head label. The baseline label is the short commit OID; a
    /// branch label is a deferred follow-up.
    label: String,
    /// Reserved for a deferred live-branch probe; rendered as current/live,
    /// never as capture-time provenance.
    #[serde(skip_serializing_if = "Option::is_none")]
    live_branch: Option<String>,
}

/// Derive the path-private [`TargetDisplay`] for a captured unit from its target
/// and base endpoints.
///
/// Pure: reads only captured fields, never the filesystem, and never rewrites
/// identity.
fn derive_target_display(target: &ReviewEndpoint, base: &ReviewEndpoint) -> TargetDisplay {
    let (kind, label) = match target {
        ReviewEndpoint::GitWorkingTree { worktree_root } => {
            ("working_tree", basename_label(worktree_root))
        }
        ReviewEndpoint::GitCommit { .. } => ("git_commit", WORKING_TREE_FLOOR.to_owned()),
    };
    TargetDisplay {
        kind,
        label,
        head: head_display(base),
        // The raw worktree path is never copied into this block.
        path_private: true,
    }
}

/// Final non-empty path component of a worktree root, or the `"working tree"`
/// floor when the path is empty, the filesystem root, or not representable.
fn basename_label(worktree_root: &str) -> String {
    Path::new(worktree_root)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| WORKING_TREE_FLOOR.to_owned())
}

/// Git-style short commit OID: the first [`SHORT_OID_LEN`] characters, or the
/// whole oid when shorter. Returns `None` for an empty oid.
fn short_oid(commit_oid: &str) -> Option<String> {
    if commit_oid.is_empty() {
        return None;
    }
    Some(commit_oid.chars().take(SHORT_OID_LEN).collect())
}

/// Head label for a base endpoint: a short OID for a Git commit, else `None`.
fn head_display(base: &ReviewEndpoint) -> Option<HeadDisplay> {
    match base {
        ReviewEndpoint::GitCommit { commit_oid, .. } => {
            let short = short_oid(commit_oid)?;
            Some(HeadDisplay {
                label: short.clone(),
                commit_oid_short: short,
                live_branch: None,
            })
        }
        ReviewEndpoint::GitWorkingTree { .. } => None,
    }
}

/// Insert a derived `targetDisplay` into the `reviewUnit` object of a serialized
/// `/api/unit` document, leaving every existing field (including the verbatim
/// `target`) in place. A no-op if `reviewUnit` is not an object.
fn splice_target_display(
    document: &mut serde_json::Value,
    target_display: TargetDisplay,
) -> Result<(), String> {
    let value = serde_json::to_value(target_display).map_err(|error| error.to_string())?;
    if let Some(review_unit) = document
        .get_mut("reviewUnit")
        .and_then(|ru| ru.as_object_mut())
    {
        review_unit.insert("targetDisplay".to_owned(), value);
    }
    Ok(())
}

/// Wrap each list entry with its derived, path-private `targetDisplay`, leaving
/// every existing field on the entry untouched.
fn to_unit_entry_documents(entries: Vec<ReviewUnitListEntry>) -> Vec<UnitEntryDocument> {
    entries
        .into_iter()
        .map(|entry| {
            let target_display = derive_target_display(&entry.target, &entry.base);
            UnitEntryDocument {
                entry,
                target_display,
            }
        })
        .collect()
}

/// Full chronological event timeline with hydrated bodies.
pub(super) fn history_json(repo: &Path) -> Result<String, String> {
    let result = review_history(ReviewHistoryOptions::new(repo).with_include_body(true))
        .map_err(|error| error.to_string())?;
    let history_count = result.history_count();
    let payload = HistoryPayload {
        schema: "shore.inspect-history",
        event_set_hash: result.event_set_hash,
        event_count: result.event_count,
        history_count,
        entries: result.entries,
        diagnostics: result.diagnostics,
    };
    serde_json::to_string(&payload).map_err(|error| error.to_string())
}

/// Captured ReviewUnits with their base/target/snapshot identity.
pub(super) fn units_json(repo: &Path) -> Result<String, String> {
    let result =
        list_review_units(ReviewUnitListOptions::new(repo)).map_err(|error| error.to_string())?;
    let payload = UnitsPayload {
        schema: "shore.inspect-units",
        event_set_hash: result.event_set_hash,
        event_count: result.event_count,
        review_unit_count: result.review_unit_count,
        entries: to_unit_entry_documents(result.entries),
        diagnostics: result.diagnostics,
    };
    serde_json::to_string(&payload).map_err(|error| error.to_string())
}

/// The captured diff snapshot for one ReviewUnit, by snapshot id.
///
/// Reads the immutable snapshot artifact through the validated read path
/// (`read_snapshot_artifact` recomputes and checks the content hash), so the
/// inspector renders exactly the frozen diff that was reviewed.
pub(super) fn snapshot_json(repo: &Path, snapshot_id: &str) -> Result<String, String> {
    if snapshot_id.is_empty() {
        return Err("missing snapshot id".to_owned());
    }
    let artifact = read_snapshot_artifact(repo, &SnapshotId::new(snapshot_id.to_owned())).map_err(
        |error| {
            // Keep the full error (which may include the internal artifact path)
            // in the server trace, but return a path-free message to the client.
            tracing::debug!(error = %error, snapshot = snapshot_id, "inspect_snapshot_read_failed");
            format!("snapshot not found or unreadable: {snapshot_id}")
        },
    )?;
    serde_json::to_string(&artifact).map_err(|error| error.to_string())
}

/// The full composite projection for one ReviewUnit.
///
/// Reuses the exact `shore.review-unit` document the `shore review unit show`
/// command builds (`unit_show_document`), so the inspector renders the same
/// authoritative composite — current-assessment status, duplicate-collapsed
/// facts, supersession, adapter notes, and projection rows — rather than
/// re-deriving it client-side.
pub(super) fn unit_json(repo: &Path, review_unit_id: &str) -> Result<String, String> {
    if review_unit_id.is_empty() {
        return Err("missing review unit id".to_owned());
    }
    let result = show_review_unit(
        ReviewUnitShowOptions::new(repo)
            .with_review_unit_id(ReviewUnitId::new(review_unit_id.to_owned()))
            .with_include_body(true),
    )
    .map_err(|error| {
        tracing::debug!(error = %error, review_unit = review_unit_id, "inspect_unit_read_failed");
        format!("review unit not found or unreadable: {review_unit_id}")
    })?;
    // Thread the typed endpoints out before `unit_show_document` consumes `result`,
    // then splice the additive `targetDisplay` into the serialized document.
    //
    // Known limitation: `/api/unit` goes through `show_review_unit`, which resolves
    // the worktree-local store (unlike the store-aware `/api/units` path). So this
    // only enriches units already readable from the current repo; clicking into a
    // linked-only unit still does not resolve. Migrating the single-unit read path
    // to the store-aware resolver is a separate, deferred follow-up.
    let target_display =
        derive_target_display(&result.review_unit.target, &result.review_unit.base);
    let document = unit_show_document(result);
    let mut value = serde_json::to_value(&document).map_err(|error| error.to_string())?;
    splice_target_display(&mut value, target_display)?;
    serde_json::to_string(&value).map_err(|error| error.to_string())
}

/// Cheap freshness probe for client-side auto-refresh polling.
///
/// Computes `eventSetHash` from the live event set (without hydrating bodies)
/// so the UI can detect store changes and re-fetch only when something moved.
pub(super) fn freshness_json(repo: &Path) -> Result<String, String> {
    let result = review_history(ReviewHistoryOptions::new(repo).with_include_body(false))
        .map_err(|error| error.to_string())?;
    let payload = FreshnessPayload {
        schema: "shore.inspect-freshness",
        event_set_hash: result.event_set_hash,
        event_count: result.event_count,
        diagnostic_count: result.diagnostics.len(),
    };
    serde_json::to_string(&payload).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use shoreline::model::{
        ReviewEndpoint, ReviewUnitSource, RevisionId, SessionId, WorktreeCaptureMode,
    };
    use shoreline::session::event::ReviewUnitCapturedPayload;

    use super::*;

    fn working_tree(root: &str) -> ReviewEndpoint {
        ReviewEndpoint::GitWorkingTree {
            worktree_root: root.to_owned(),
        }
    }

    fn commit(oid: &str) -> ReviewEndpoint {
        ReviewEndpoint::GitCommit {
            commit_oid: oid.to_owned(),
            tree_oid: "tree-oid".to_owned(),
        }
    }

    #[test]
    fn derives_basename_label_and_short_head_from_captured_fields() {
        let target = working_tree("/Users/x/worktrees/boardwalk/plan-0006");
        let base = commit("545b0eb81463aaaaaaaaaaaaaaaaaaaaaaaaaaaa");

        let display = derive_target_display(&target, &base);

        assert_eq!(display.kind, "working_tree");
        assert_eq!(display.label, "plan-0006");
        let head = display
            .head
            .as_ref()
            .expect("head derived from base commit");
        assert_eq!(head.commit_oid_short, "545b0eb");
        assert_eq!(head.label, "545b0eb");
        assert!(head.live_branch.is_none());
        assert!(display.path_private);
    }

    #[test]
    fn floors_empty_or_root_worktree_root_to_working_tree() {
        assert_eq!(
            derive_target_display(&working_tree("/"), &commit("abc1234")).label,
            "working tree"
        );
        assert_eq!(
            derive_target_display(&working_tree(""), &commit("abc1234")).label,
            "working tree"
        );
    }

    #[test]
    fn empty_commit_oid_yields_no_head() {
        let display = derive_target_display(&working_tree("/repo/wt"), &commit(""));
        assert!(display.head.is_none());
    }

    #[test]
    fn serialized_block_is_camel_case_and_path_private() {
        let display = derive_target_display(
            &working_tree("/Users/x/worktrees/boardwalk/plan-0006"),
            &commit("545b0eb81463aaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        );
        let json = serde_json::to_string(&display).unwrap();

        assert!(json.contains("\"pathPrivate\":true"));
        assert!(json.contains("\"commitOidShort\":\"545b0eb\""));
        assert!(json.contains("\"label\":\"plan-0006\""));
        // No raw absolute path and no worktreeRoot key leak into the display block.
        assert!(!json.contains("/Users"));
        assert!(!json.contains("worktreeRoot"));
    }

    fn entry(worktree: &str, commit: &str) -> ReviewUnitListEntry {
        ReviewUnitListEntry {
            review_unit_id: ReviewUnitId::new("review-unit:sha256:abc"),
            session_id: SessionId::new("session:default"),
            captured_at: "2026-05-13T10:00:00Z".to_owned(),
            revision_id: RevisionId::new("rev:sha256:abc"),
            snapshot_id: SnapshotId::new("snap:sha256:abc"),
            source: ReviewUnitSource::GitWorktree {
                mode: WorktreeCaptureMode::CombinedHeadToWorkingTree,
                include_untracked: true,
            },
            base: ReviewEndpoint::GitCommit {
                commit_oid: commit.to_owned(),
                tree_oid: "tree-oid".to_owned(),
            },
            target: ReviewEndpoint::GitWorkingTree {
                worktree_root: worktree.to_owned(),
            },
            snapshot_artifact_content_hash: "sha256:artifact:abc".to_owned(),
        }
    }

    #[test]
    fn units_document_splices_target_display_additively() {
        let entries = vec![entry(
            "/Users/x/worktrees/boardwalk/plan-0006",
            "545b0eb81463aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )];

        let docs = to_unit_entry_documents(entries);
        let json = serde_json::to_value(&docs[0]).unwrap();

        // The derived, path-private targetDisplay is spliced in...
        assert_eq!(json["targetDisplay"]["label"], "plan-0006");
        assert_eq!(json["targetDisplay"]["head"]["commitOidShort"], "545b0eb");
        assert_eq!(json["targetDisplay"]["pathPrivate"], true);

        // ...and every prior field is still byte-present and unchanged (additive).
        assert_eq!(
            json["target"]["worktreeRoot"],
            "/Users/x/worktrees/boardwalk/plan-0006"
        );
        assert_eq!(json["target"]["kind"], "git_working_tree");
        assert_eq!(
            json["base"]["commitOid"],
            "545b0eb81463aaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert!(json["source"].is_object());
        assert_eq!(json["snapshotArtifactContentHash"], "sha256:artifact:abc");
        assert_eq!(json["reviewUnitId"], "review-unit:sha256:abc");
        assert_eq!(json["capturedAt"], "2026-05-13T10:00:00Z");
        assert_eq!(json["revisionId"], "rev:sha256:abc");
        assert_eq!(json["snapshotId"], "snap:sha256:abc");
    }

    #[test]
    fn splice_target_display_adds_block_without_dropping_target_fields() {
        // Mirrors the /api/unit document shape: reviewUnit carries the verbatim target.
        let mut document = serde_json::json!({
            "reviewUnit": {
                "id": "review-unit:sha256:abc",
                "target": {
                    "kind": "git_working_tree",
                    "worktreeRoot": "/Users/x/worktrees/boardwalk/plan-0006"
                },
                "base": { "kind": "git_commit", "commitOid": "545b0eb81463", "treeOid": "t" }
            }
        });
        let display = derive_target_display(
            &working_tree("/Users/x/worktrees/boardwalk/plan-0006"),
            &commit("545b0eb81463aaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        );

        splice_target_display(&mut document, display).unwrap();

        assert_eq!(
            document["reviewUnit"]["targetDisplay"]["label"],
            "plan-0006"
        );
        assert_eq!(
            document["reviewUnit"]["targetDisplay"]["head"]["commitOidShort"],
            "545b0eb"
        );
        // Additive: the raw target endpoint is untouched.
        assert_eq!(
            document["reviewUnit"]["target"]["worktreeRoot"],
            "/Users/x/worktrees/boardwalk/plan-0006"
        );
        assert_eq!(document["reviewUnit"]["target"]["kind"], "git_working_tree");
    }

    #[test]
    fn legacy_worktree_root_payload_derives_basename_without_touching_identity() {
        // A payload that only ever carried `worktreeRoot`. Deriving the display
        // must be a pure read: it must not rewrite the ReviewUnit identity and
        // must not leak the raw path into the derived block.
        let review_unit_id = ReviewUnitId::new("review-unit:sha256:legacy");
        let payload = ReviewUnitCapturedPayload {
            review_unit_id: review_unit_id.clone(),
            source: ReviewUnitSource::GitWorktree {
                mode: WorktreeCaptureMode::CombinedHeadToWorkingTree,
                include_untracked: true,
            },
            base: ReviewEndpoint::GitCommit {
                commit_oid: "0123456789abcdef0123456789abcdef01234567".to_owned(),
                tree_oid: "tree-oid".to_owned(),
            },
            target: ReviewEndpoint::GitWorkingTree {
                worktree_root: "/repo/legacy-wt".to_owned(),
            },
            revision_id: RevisionId::new("rev:sha256:legacy"),
            snapshot_id: SnapshotId::new("snap:sha256:legacy"),
            snapshot_artifact_content_hash: "sha256:artifact:legacy".to_owned(),
        };

        let display = derive_target_display(&payload.target, &payload.base);
        let json = serde_json::to_string(&display).unwrap();

        assert_eq!(display.label, "legacy-wt");
        assert!(display.path_private);
        assert_eq!(display.head.as_ref().unwrap().commit_oid_short, "0123456");
        // No raw path leaks into the derived block.
        assert!(!json.contains("/repo"));
        // Derivation never rewrote identity (no event/file written).
        assert_eq!(payload.review_unit_id, review_unit_id);
    }
}
