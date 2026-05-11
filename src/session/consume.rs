use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use crate::error::Result;
use crate::git::git_worktree_root;
use crate::model::{AcknowledgementId, ReviewArtifactId, RevisionId, WorkUnitId};
use crate::session::body_artifact::load_body_artifact;
use crate::session::event::{
    AcknowledgementNextAction, EventType, ReviewArtifactAcknowledgedPayload,
    ReviewArtifactPublishedPayload, ReviewNoteImportedPayload, VerdictDecision, Writer,
};
use crate::session::state::SessionState;
use crate::sidecar::{
    ParsedReviewNotes, ReviewNoteEntry, ReviewNoteTarget, ReviewNotesFile, ReviewNotesSidecar,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewArtifact {
    pub id: ReviewArtifactId,
    pub work_unit_id: WorkUnitId,
    pub revision_id: RevisionId,
    pub decision: VerdictDecision,
    pub summary: Option<String>,
    pub replaces_review_artifact_ids: Vec<ReviewArtifactId>,
    pub reviewer: Writer,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Acknowledgement {
    pub id: AcknowledgementId,
    pub review_artifact_id: ReviewArtifactId,
    pub next_action: AcknowledgementNextAction,
    pub reason: Option<String>,
    pub acknowledger: Writer,
}

/// Replay a single durable `review_note_imported` event payload back into a `ReviewNoteEntry`.
///
/// Loads artifact-backed note bodies from `.shore/artifacts/notes/` when available. Falls back to
/// inline body if no artifact path is set.
#[allow(dead_code)]
pub(crate) fn replay_note_entry(
    payload: &ReviewNoteImportedPayload,
    shore_dir: &Path,
) -> Result<ReviewNoteEntry> {
    let target = payload
        .target
        .as_ref()
        .map(|imported_target| ReviewNoteTarget {
            side: imported_target.side,
            start_line: imported_target.start_line,
            end_line: imported_target.end_line,
        });

    // Load artifact body if path is set; otherwise use inline body
    let body = if let Some(artifact_path) = &payload.body_artifact_path {
        load_body_artifact(shore_dir, artifact_path)?
    } else {
        payload.body.clone()
    };

    Ok(ReviewNoteEntry {
        id: Some(payload.note_id.clone()),
        title: Some(payload.title.clone()),
        body,
        target,
        tags: payload.tags.clone(),
        confidence: payload.confidence.clone(),
        source: payload.external_source.clone(),
        author: payload.author.clone(),
        created_at: payload.created_at.clone(),
    })
}

/// Replay a collection of durable `review_note_imported` event payloads into a `ParsedReviewNotes`.
///
/// Groups notes by file path, deterministically ordered. The grouping is synthetic and does not
/// claim that the original transport file order is authoritative. Event order is not used for
/// file ordering.
#[allow(dead_code)]
pub(crate) fn parsed_review_notes_from_imports(
    payloads: &[ReviewNoteImportedPayload],
    shore_dir: &Path,
) -> Result<ParsedReviewNotes> {
    // Build a map of file_path -> (old_path, notes) to group notes by file
    let mut file_map: BTreeMap<String, (Option<String>, Vec<ReviewNoteEntry>)> = BTreeMap::new();

    for payload in payloads {
        let entry = replay_note_entry(payload, shore_dir)?;

        file_map
            .entry(payload.file_path.clone())
            .or_insert((payload.file_old_path.clone(), Vec::new()))
            .1
            .push(entry);
    }

    // Convert the map into an ordered list of ReviewNotesFile, with notes sorted by line range
    let files = file_map
        .into_iter()
        .map(|(path, (old_path, mut notes))| {
            // Sort notes by start_line for deterministic ordering within each file
            notes.sort_by_key(|note| {
                (
                    note.target.map(|t| t.start_line).unwrap_or(0),
                    note.id.clone().unwrap_or_default(),
                )
            });
            ReviewNotesFile {
                path,
                old_path,
                summary: None,
                notes,
            }
        })
        .collect();

    Ok(ParsedReviewNotes {
        sidecar: ReviewNotesSidecar {
            schema: Some("shore.review-notes".to_owned()),
            version: 1,
            summary: None,
            files,
        },
        diagnostics: Vec::new(),
    })
}

/// Load durable notes for a repository by replaying imported-note events.
///
/// Discovers `.shore/` at the worktree root. Returns `Ok(None)` if the store does not exist
/// (no durable state to load) or if no imported notes exist in the store.
///
/// This is a read-only operation and does not create directories or perform any mutations.
pub fn load_durable_notes_for_repo(repo: impl AsRef<Path>) -> Result<Option<ParsedReviewNotes>> {
    let repo_path = repo.as_ref();
    let worktree_root = git_worktree_root(repo_path)?;
    let shore_dir = worktree_root.join(".shore");

    // If .shore doesn't exist, there's no durable state to load
    if !shore_dir.exists() {
        return Ok(None);
    }

    // Read all events from the store
    let events = crate::session::read_events(&worktree_root)?;

    // Filter to ReviewNoteImported events and deserialize payloads
    let mut imported_payloads = Vec::new();
    for event in events {
        if event.event_type != EventType::ReviewNoteImported {
            continue;
        }

        let payload: ReviewNoteImportedPayload = serde_json::from_value(event.payload)?;
        imported_payloads.push(payload);
    }

    // Return None if no imported notes exist
    if imported_payloads.is_empty() {
        return Ok(None);
    }

    // Replay the imported notes into ParsedReviewNotes
    let parsed = parsed_review_notes_from_imports(&imported_payloads, &shore_dir)?;

    Ok(Some(parsed))
}

pub fn read_review_artifacts(repo: impl AsRef<Path>) -> Result<Vec<ReviewArtifact>> {
    let worktree_root = git_worktree_root(repo.as_ref())?;
    let shore_dir = worktree_root.join(".shore");
    if !shore_dir.exists() {
        return Ok(Vec::new());
    }

    let mut review_artifacts = Vec::new();
    for event in crate::session::read_events(&worktree_root)? {
        if event.event_type != EventType::ReviewArtifactPublished {
            continue;
        }
        let payload: ReviewArtifactPublishedPayload = serde_json::from_value(event.payload)?;
        let summary = if let Some(artifact_path) = &payload.summary_artifact_path {
            load_body_artifact(&shore_dir, artifact_path)?
        } else {
            payload.summary.clone()
        };
        review_artifacts.push(ReviewArtifact {
            id: payload.review_artifact_id,
            work_unit_id: payload.work_unit_id,
            revision_id: payload.revision_id,
            decision: payload.decision,
            summary,
            replaces_review_artifact_ids: payload.replaces_review_artifact_ids,
            reviewer: payload.reviewer,
        });
    }

    Ok(review_artifacts)
}

pub fn read_acknowledgements(repo: impl AsRef<Path>) -> Result<Vec<Acknowledgement>> {
    let worktree_root = git_worktree_root(repo.as_ref())?;
    let shore_dir = worktree_root.join(".shore");
    if !shore_dir.exists() {
        return Ok(Vec::new());
    }

    let mut acknowledgements = Vec::new();
    for event in crate::session::read_events(&worktree_root)? {
        if event.event_type != EventType::ReviewArtifactAcknowledged {
            continue;
        }
        let payload: ReviewArtifactAcknowledgedPayload = serde_json::from_value(event.payload)?;
        let reason = if let Some(artifact_path) = &payload.reason_artifact_path {
            load_body_artifact(&shore_dir, artifact_path)?
        } else {
            payload.reason.clone()
        };
        acknowledgements.push(Acknowledgement {
            id: payload.acknowledgement_id,
            review_artifact_id: payload.review_artifact_id,
            next_action: payload.next_action,
            reason,
            acknowledger: payload.acknowledger,
        });
    }

    Ok(acknowledgements)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CurrentVerdictView {
    Resolved {
        decision: VerdictDecision,
        review_artifact_id: ReviewArtifactId,
    },
    Ambiguous {
        review_artifact_ids: Vec<ReviewArtifactId>,
    },
    None,
}

pub fn current_verdict_view(
    artifacts: &[ReviewArtifact],
    current_revision_id: Option<&RevisionId>,
) -> CurrentVerdictView {
    let Some(revision_id) = current_revision_id else {
        return CurrentVerdictView::None;
    };

    // Mirrors StateReducer::finish unreplaced verdict selection.
    let replaced_ids = artifacts
        .iter()
        .flat_map(|artifact| artifact.replaces_review_artifact_ids.iter())
        .collect::<HashSet<_>>();
    let unreplaced = artifacts
        .iter()
        .filter(|artifact| {
            &artifact.revision_id == revision_id && !replaced_ids.contains(&artifact.id)
        })
        .collect::<Vec<_>>();

    match unreplaced.as_slice() {
        [] => CurrentVerdictView::None,
        [artifact] => CurrentVerdictView::Resolved {
            decision: artifact.decision,
            review_artifact_id: artifact.id.clone(),
        },
        artifacts => CurrentVerdictView::Ambiguous {
            review_artifact_ids: artifacts
                .iter()
                .map(|artifact| artifact.id.clone())
                .collect(),
        },
    }
}

pub fn load_or_rebuild_session_state(repo: impl AsRef<Path>) -> Result<Option<SessionState>> {
    let worktree_root = git_worktree_root(repo.as_ref())?;
    let shore_dir = worktree_root.join(".shore");
    if !shore_dir.exists() {
        return Ok(None);
    }

    let events = crate::session::read_events(&worktree_root)?;
    Ok(Some(SessionState::from_events(&events)?))
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;
    use crate::model::{ReviewId, Side};
    use crate::session::event::{
        EventTarget, ImportedNoteTarget, ReviewArtifactAcknowledgedPayload,
        ReviewArtifactPublishedPayload, ReviewInitializedPayload, RevisionPublishedPayload,
    };
    use crate::session::{EventType, ShoreEvent};
    use crate::storage::EventStore;

    fn test_payload_for(file_path: &str, note_id: &str) -> ReviewNoteImportedPayload {
        ReviewNoteImportedPayload {
            sidecar_source: crate::session::event::SidecarSource::ReviewNotes,
            note_id: note_id.to_owned(),
            file_path: file_path.to_owned(),
            file_old_path: None,
            target: Some(ImportedNoteTarget {
                side: Side::New,
                start_line: 2,
                end_line: 3,
            }),
            title: format!("Title for {}", note_id),
            body: Some(format!("Body for {}", note_id)),
            body_artifact_path: None,
            body_byte_size: None,
            tags: vec!["tag".to_owned()],
            confidence: Some("high".to_owned()),
            external_source: Some("tool".to_owned()),
            author: Some("reviewer".to_owned()),
            created_at: Some("2026-05-10T00:00:00Z".to_owned()),
            sidecar_content_hash: "sha256:abc".to_owned(),
        }
    }

    #[test]
    fn imported_note_payload_converts_to_review_note_entry() {
        let payload = ReviewNoteImportedPayload {
            sidecar_source: crate::session::event::SidecarSource::ReviewNotes,
            note_id: "note:123".to_owned(),
            file_path: "src/lib.rs".to_owned(),
            file_old_path: None,
            target: Some(ImportedNoteTarget {
                side: Side::New,
                start_line: 2,
                end_line: 3,
            }),
            title: "Durable title".to_owned(),
            body: Some("Durable body".to_owned()),
            body_artifact_path: None,
            body_byte_size: None,
            tags: vec!["tag".to_owned()],
            confidence: Some("high".to_owned()),
            external_source: Some("tool".to_owned()),
            author: Some("reviewer".to_owned()),
            created_at: Some("2026-05-10T00:00:00Z".to_owned()),
            sidecar_content_hash: "sha256:abc".to_owned(),
        };

        let entry = replay_note_entry(&payload, Path::new(".shore")).expect("entry builds");

        assert_eq!(entry.id.as_deref(), Some("note:123"));
        assert_eq!(entry.title.as_deref(), Some("Durable title"));
        assert_eq!(entry.body.as_deref(), Some("Durable body"));
        assert_eq!(entry.target.unwrap().start_line, 2);
    }

    #[test]
    fn imported_notes_group_by_file_without_using_event_order_as_file_order() {
        let events = vec![
            test_payload_for("b.rs", "note:b"),
            test_payload_for("a.rs", "note:a"),
        ];

        let parsed =
            parsed_review_notes_from_imports(&events, Path::new(".shore")).expect("parses");

        assert_eq!(parsed.sidecar.files.len(), 2);
        assert_eq!(
            parsed
                .sidecar
                .files
                .iter()
                .map(|f| f.notes.len())
                .sum::<usize>(),
            2
        );
        // Files should be in sorted order (a.rs before b.rs) despite event order being reversed
        assert_eq!(parsed.sidecar.files[0].path, "a.rs");
        assert_eq!(parsed.sidecar.files[1].path, "b.rs");
    }

    #[test]
    fn artifact_backed_note_body_is_loaded_from_artifact() {
        let shore_dir = tempfile::tempdir().expect("create shore dir");
        let artifact_path = "artifacts/notes/note-abc.json";
        let artifact_file = shore_dir.path().join(artifact_path);
        std::fs::create_dir_all(artifact_file.parent().expect("artifact parent"))
            .expect("create artifact dir");
        std::fs::write(
            &artifact_file,
            r#"{"schema":"shore.note-body","version":1,"body":"Artifact body"}"#,
        )
        .expect("write artifact");

        let payload = ReviewNoteImportedPayload {
            sidecar_source: crate::session::event::SidecarSource::ReviewNotes,
            note_id: "note:with-artifact".to_owned(),
            file_path: "src/lib.rs".to_owned(),
            file_old_path: None,
            target: Some(ImportedNoteTarget {
                side: Side::New,
                start_line: 5,
                end_line: 6,
            }),
            title: "Note with artifact body".to_owned(),
            body: None,
            body_artifact_path: Some(artifact_path.to_owned()),
            body_byte_size: Some(256),
            tags: vec![],
            confidence: None,
            external_source: None,
            author: None,
            created_at: None,
            sidecar_content_hash: "sha256:abc".to_owned(),
        };

        let entry = replay_note_entry(&payload, shore_dir.path()).expect("entry builds");

        assert_eq!(entry.body.as_deref(), Some("Artifact body"));
    }

    #[test]
    fn artifact_body_path_must_stay_under_artifacts_notes() {
        let payload = ReviewNoteImportedPayload {
            sidecar_source: crate::session::event::SidecarSource::ReviewNotes,
            note_id: "note:escape".to_owned(),
            file_path: "src/lib.rs".to_owned(),
            file_old_path: None,
            target: None,
            title: "Escaped".to_owned(),
            body: None,
            body_artifact_path: Some("../outside.json".to_owned()),
            body_byte_size: None,
            tags: vec![],
            confidence: None,
            external_source: None,
            author: None,
            created_at: None,
            sidecar_content_hash: "sha256:abc".to_owned(),
        };

        let error =
            replay_note_entry(&payload, Path::new(".shore")).expect_err("path should be rejected");
        assert!(error.to_string().contains("Invalid artifact path"));
    }

    #[test]
    fn artifact_body_schema_must_match() {
        let shore_dir = tempfile::tempdir().expect("create shore dir");
        let artifact_path = "artifacts/notes/note-abc.json";
        let artifact_file = shore_dir.path().join(artifact_path);
        std::fs::create_dir_all(artifact_file.parent().expect("artifact parent"))
            .expect("create artifact dir");
        std::fs::write(
            &artifact_file,
            r#"{"schema":"wrong.schema","version":2,"body":"Artifact body"}"#,
        )
        .expect("write artifact");

        let payload = ReviewNoteImportedPayload {
            sidecar_source: crate::session::event::SidecarSource::ReviewNotes,
            note_id: "note:wrong-schema".to_owned(),
            file_path: "src/lib.rs".to_owned(),
            file_old_path: None,
            target: None,
            title: "Wrong schema".to_owned(),
            body: None,
            body_artifact_path: Some(artifact_path.to_owned()),
            body_byte_size: None,
            tags: vec![],
            confidence: None,
            external_source: None,
            author: None,
            created_at: None,
            sidecar_content_hash: "sha256:abc".to_owned(),
        };

        let error =
            replay_note_entry(&payload, shore_dir.path()).expect_err("schema should be rejected");
        assert!(
            error
                .to_string()
                .contains("Unsupported note body artifact schema/version")
        );
    }

    #[test]
    fn read_review_artifacts_replays_published_events() {
        let repo = test_repo_with(vec![
            review_initialized(),
            revision_published("rev1"),
            review_artifact_published("a", VerdictDecision::Pass),
            review_artifact_published("b", VerdictDecision::RequestChanges),
        ]);

        let artifacts = read_review_artifacts(repo.path()).unwrap();
        assert_eq!(artifacts.len(), 2);
        let by_id: BTreeMap<_, _> = artifacts
            .iter()
            .map(|a| (a.id.as_str(), &a.decision))
            .collect();
        assert_eq!(by_id["review-artifact:sha256:a"], &VerdictDecision::Pass);
    }

    #[test]
    fn read_review_artifacts_is_order_independent() {
        let artifact_a = review_artifact_published("a", VerdictDecision::Pass);
        let artifact_b = review_artifact_published("b", VerdictDecision::RequestChanges);
        let repo_one = test_repo_with(vec![
            review_initialized(),
            revision_published("rev1"),
            artifact_a.clone(),
            artifact_b.clone(),
        ]);
        let repo_two = test_repo_with(vec![
            review_initialized(),
            revision_published("rev1"),
            artifact_b,
            artifact_a,
        ]);

        let mut first = read_review_artifacts(repo_one.path()).unwrap();
        let mut second = read_review_artifacts(repo_two.path()).unwrap();
        first.sort_by_key(|artifact| artifact.id.as_str().to_owned());
        second.sort_by_key(|artifact| artifact.id.as_str().to_owned());
        assert_eq!(first, second);
    }

    #[test]
    fn read_acknowledgements_replays_events() {
        let repo = test_repo_with(vec![
            review_initialized(),
            revision_published("rev1"),
            review_artifact_published("a", VerdictDecision::Pass),
            review_artifact_acknowledged("ack-a", "a", AcknowledgementNextAction::Accept),
        ]);

        let acknowledgements = read_acknowledgements(repo.path()).unwrap();
        assert_eq!(acknowledgements.len(), 1);
        assert_eq!(
            acknowledgements[0].review_artifact_id.as_str(),
            "review-artifact:sha256:a"
        );
        assert_eq!(
            acknowledgements[0].next_action,
            AcknowledgementNextAction::Accept
        );
    }

    #[test]
    fn read_review_artifacts_hydrates_externalized_summary() {
        let body = "summary-body".to_owned();
        let repo = test_repo_with_external_summary(&body);

        let artifacts = read_review_artifacts(repo.path()).unwrap();
        assert_eq!(artifacts[0].summary.as_deref(), Some(body.as_str()));
    }

    #[test]
    fn current_verdict_view_returns_resolved_when_single_unreplaced_verdict() {
        let artifacts = vec![sample_verdict_artifact(
            "rev-1",
            "review-artifact:1",
            VerdictDecision::Pass,
            &[],
        )];

        let view = current_verdict_view(&artifacts, Some(&RevisionId::new("rev-1")));

        assert!(matches!(
            view,
            CurrentVerdictView::Resolved {
                decision: VerdictDecision::Pass,
                ref review_artifact_id,
            } if review_artifact_id.as_str() == "review-artifact:1"
        ));
    }

    #[test]
    fn current_verdict_view_returns_ambiguous_when_two_unreplaced_verdicts() {
        let artifacts = vec![
            sample_verdict_artifact("rev-1", "review-artifact:1", VerdictDecision::Pass, &[]),
            sample_verdict_artifact(
                "rev-1",
                "review-artifact:2",
                VerdictDecision::RequestChanges,
                &[],
            ),
        ];

        let view = current_verdict_view(&artifacts, Some(&RevisionId::new("rev-1")));

        match view {
            CurrentVerdictView::Ambiguous {
                review_artifact_ids,
            } => {
                assert_eq!(review_artifact_ids.len(), 2);
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn current_verdict_view_returns_none_when_no_verdicts() {
        let view = current_verdict_view(&[], Some(&RevisionId::new("rev-1")));

        assert!(matches!(view, CurrentVerdictView::None));
    }

    #[test]
    fn current_verdict_view_excludes_replaced_artifacts_from_unreplaced_set() {
        let artifacts = vec![
            sample_verdict_artifact("rev-1", "review-artifact:1", VerdictDecision::Pass, &[]),
            sample_verdict_artifact(
                "rev-1",
                "review-artifact:2",
                VerdictDecision::RequestChanges,
                &["review-artifact:1"],
            ),
        ];

        let view = current_verdict_view(&artifacts, Some(&RevisionId::new("rev-1")));

        match view {
            CurrentVerdictView::Resolved {
                decision,
                ref review_artifact_id,
            } => {
                assert_eq!(decision, VerdictDecision::RequestChanges);
                assert_eq!(review_artifact_id.as_str(), "review-artifact:2");
            }
            other => panic!("expected Resolved on the replacing artifact, got {other:?}"),
        }
    }

    #[test]
    fn load_or_rebuild_session_state_returns_none_when_shore_dir_absent() {
        let repo = tempfile::tempdir().expect("create repo");
        Command::new("git")
            .args(["init"])
            .current_dir(repo.path())
            .output()
            .unwrap();

        let state = load_or_rebuild_session_state(repo.path()).unwrap();

        assert!(state.is_none());
    }

    #[test]
    fn load_or_rebuild_session_state_rebuilds_from_events_when_shore_dir_present() {
        let repo = test_repo_with(vec![review_initialized(), revision_published("rev1")]);

        let state = load_or_rebuild_session_state(repo.path()).unwrap();
        let state = state.expect("state should be present when .shore/ exists");

        assert_eq!(
            state
                .current_revision_id
                .as_ref()
                .map(|revision| revision.as_str()),
            Some("rev1"),
        );
    }

    fn test_repo_with(events: Vec<ShoreEvent>) -> tempfile::TempDir {
        let repo = tempfile::tempdir().expect("create repo");
        Command::new("git")
            .args(["init"])
            .current_dir(repo.path())
            .output()
            .unwrap();
        let shore_dir = repo.path().join(".shore");
        std::fs::create_dir_all(shore_dir.join("events")).unwrap();
        let store = EventStore::open(&shore_dir);
        for event in events {
            store.record_event_once(&event).unwrap();
        }
        repo
    }

    fn test_repo_with_external_summary(body: &str) -> tempfile::TempDir {
        let repo = test_repo_with(vec![
            review_initialized(),
            revision_published("rev1"),
            review_artifact_published_with_artifact("a", VerdictDecision::Pass, body),
        ]);
        let artifact_path = repo
            .path()
            .join(".shore/artifacts/notes/review-artifact-a.json");
        std::fs::create_dir_all(artifact_path.parent().unwrap()).unwrap();
        std::fs::write(
            artifact_path,
            format!(
                r#"{{"schema":"shore.note-body","version":1,"body":"{}"}}"#,
                body
            ),
        )
        .unwrap();
        repo
    }

    fn sample_verdict_artifact(
        revision_id: &str,
        artifact_id: &str,
        decision: VerdictDecision,
        replaces: &[&str],
    ) -> ReviewArtifact {
        ReviewArtifact {
            id: ReviewArtifactId::new(artifact_id),
            work_unit_id: WorkUnitId::new("work:default"),
            revision_id: RevisionId::new(revision_id),
            decision,
            summary: Some("summary".to_owned()),
            replaces_review_artifact_ids: replaces
                .iter()
                .map(|id| ReviewArtifactId::new(*id))
                .collect(),
            reviewer: Writer::shore_local_reviewer("0.1.0"),
        }
    }

    fn review_initialized() -> ShoreEvent {
        ShoreEvent::new(
            EventType::ReviewInitialized,
            "review_initialized:review:default:work:default",
            EventTarget::new(
                ReviewId::new("review:default"),
                WorkUnitId::new("work:default"),
            ),
            Writer::shore_local_author("0.1.0"),
            ReviewInitializedPayload {},
            "2026-05-10T00:00:00Z",
        )
        .unwrap()
    }

    fn revision_published(revision_id: &str) -> ShoreEvent {
        ShoreEvent::new(
            EventType::RevisionPublished,
            format!("revision_published:explicit:work:default:{revision_id}"),
            EventTarget::new(
                ReviewId::new("review:default"),
                WorkUnitId::new("work:default"),
            ),
            Writer::shore_local_author("0.1.0"),
            RevisionPublishedPayload {
                revision_id: RevisionId::new(revision_id),
                supersedes_revision_ids: vec![],
            },
            "2026-05-10T00:00:00Z",
        )
        .unwrap()
    }

    fn review_artifact_published(
        review_artifact_id_suffix: &str,
        decision: VerdictDecision,
    ) -> ShoreEvent {
        ShoreEvent::new(
            EventType::ReviewArtifactPublished,
            format!(
                "review_artifact_published:work:default:review-artifact:sha256:{review_artifact_id_suffix}"
            ),
            EventTarget::new(
                ReviewId::new("review:default"),
                WorkUnitId::new("work:default"),
            ),
            Writer::shore_local_reviewer("0.1.0"),
            ReviewArtifactPublishedPayload {
                review_artifact_id: ReviewArtifactId::new(format!(
                    "review-artifact:sha256:{review_artifact_id_suffix}"
                )),
                work_unit_id: WorkUnitId::new("work:default"),
                revision_id: RevisionId::new("rev1"),
                decision,
                summary: Some("summary".to_owned()),
                summary_artifact_path: None,
                summary_byte_size: Some(7),
                replaces_review_artifact_ids: vec![],
                reviewer: Writer::shore_local_reviewer("0.1.0"),
            },
            "2026-05-10T00:00:00Z",
        )
        .unwrap()
    }

    fn review_artifact_published_with_artifact(
        review_artifact_id_suffix: &str,
        decision: VerdictDecision,
        body: &str,
    ) -> ShoreEvent {
        ShoreEvent::new(
            EventType::ReviewArtifactPublished,
            format!(
                "review_artifact_published:work:default:review-artifact:sha256:{review_artifact_id_suffix}"
            ),
            EventTarget::new(
                ReviewId::new("review:default"),
                WorkUnitId::new("work:default"),
            ),
            Writer::shore_local_reviewer("0.1.0"),
            ReviewArtifactPublishedPayload {
                review_artifact_id: ReviewArtifactId::new(format!(
                    "review-artifact:sha256:{review_artifact_id_suffix}"
                )),
                work_unit_id: WorkUnitId::new("work:default"),
                revision_id: RevisionId::new("rev1"),
                decision,
                summary: None,
                summary_artifact_path: Some(format!(
                    "artifacts/notes/review-artifact-{review_artifact_id_suffix}.json"
                )),
                summary_byte_size: Some(body.len() as u64),
                replaces_review_artifact_ids: vec![],
                reviewer: Writer::shore_local_reviewer("0.1.0"),
            },
            "2026-05-10T00:00:00Z",
        )
        .unwrap()
    }

    fn review_artifact_acknowledged(
        acknowledgement_id_suffix: &str,
        review_artifact_id_suffix: &str,
        next_action: AcknowledgementNextAction,
    ) -> ShoreEvent {
        ShoreEvent::new(
            EventType::ReviewArtifactAcknowledged,
            format!(
                "review_artifact_acknowledged:review-artifact:sha256:{review_artifact_id_suffix}:ack:sha256:{acknowledgement_id_suffix}"
            ),
            EventTarget::new(
                ReviewId::new("review:default"),
                WorkUnitId::new("work:default"),
            ),
            Writer::shore_local_author("0.1.0"),
            ReviewArtifactAcknowledgedPayload {
                acknowledgement_id: AcknowledgementId::new(format!(
                    "ack:sha256:{acknowledgement_id_suffix}"
                )),
                review_artifact_id: ReviewArtifactId::new(format!(
                    "review-artifact:sha256:{review_artifact_id_suffix}"
                )),
                next_action,
                reason: Some("ack".to_owned()),
                reason_artifact_path: None,
                reason_byte_size: Some(3),
                acknowledger: Writer::shore_local_author("0.1.0"),
            },
            "2026-05-10T00:00:00Z",
        )
        .unwrap()
    }
}
