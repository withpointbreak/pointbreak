use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::{Result, ShoreError};
use crate::model::{ReviewUnitId, ReviewUnitLineageBasisV1, ReviewUnitLineageId};
use crate::session::event::{
    EventTarget, EventType, ReviewUnitCapturedPayload, ReviewUnitLineageDeclaredPayload,
    ReviewUnitLineageRoundRecordedPayload, ShoreEvent,
};
use crate::session::projection::lineage::ReviewUnitLineageProjection;
use crate::session::state::{ProjectionDiagnostic, SessionState};
use crate::session::store::resolution::resolve_write_validation_store;
use crate::session::store_init::{ShoreStorePaths, prepare_shore_writer};
use crate::session::workflow::write_store::fact_batch_only_diagnostics;
use crate::session::{EventStore, EventWriteOutcome, current_timestamp, writer_from_options};
use crate::storage::{Durability, LocalStorage};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LineageAttachOptions {
    repo: PathBuf,
    lineage_id: ReviewUnitLineageId,
    review_unit_id: Option<ReviewUnitId>,
    predecessor_review_unit_id: Option<ReviewUnitId>,
    change_id: Option<String>,
}

impl LineageAttachOptions {
    pub fn new(repo: impl AsRef<Path>, lineage_id: ReviewUnitLineageId) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            lineage_id,
            review_unit_id: None,
            predecessor_review_unit_id: None,
            change_id: None,
        }
    }

    pub fn with_review_unit_id(mut self, review_unit_id: ReviewUnitId) -> Self {
        self.review_unit_id = Some(review_unit_id);
        self
    }

    pub fn with_predecessor_review_unit_id(mut self, review_unit_id: ReviewUnitId) -> Self {
        self.predecessor_review_unit_id = Some(review_unit_id);
        self
    }

    pub fn with_change_id(mut self, change_id: impl Into<String>) -> Self {
        self.change_id = Some(change_id.into());
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LineageAttachResult {
    pub lineage_id: ReviewUnitLineageId,
    pub head_review_unit_id: Option<ReviewUnitId>,
    pub events_created: usize,
    pub events_existing: usize,
    pub events_created_by_type: BTreeMap<String, usize>,
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

pub fn attach_review_unit_to_lineage(options: LineageAttachOptions) -> Result<LineageAttachResult> {
    let paths = ShoreStorePaths::resolve(&options.repo)?;
    let worktree_root = paths.worktree_root();
    let shore_dir = paths.shore_dir();
    let storage = LocalStorage::new(shore_dir);
    prepare_shore_writer(&paths, &storage)?;

    let review_unit_id =
        options
            .review_unit_id
            .ok_or_else(|| ShoreError::WorkflowInputInvalid {
                reason: "review unit is required".to_owned(),
            })?;
    // Pre-write validation/derivation reads resolve the writer-visible union:
    // the attached unit and its predecessor may exist only in the linked store.
    let validation_store = resolve_write_validation_store(&options.repo)?;
    let validation_events = validation_store.validation_events()?;
    let capture = stored_capture_payload(&validation_events, &review_unit_id)?;
    if let Some(predecessor) = options.predecessor_review_unit_id.as_ref() {
        stored_capture_payload(&validation_events, predecessor)?;
    }
    // The session id is baked into both lineage events; compute it once from the
    // union rather than re-reading per event.
    let session_id = capture_session_id(&validation_events, &review_unit_id)?;

    let lineage_id = options.lineage_id.clone();
    let basis = ReviewUnitLineageBasisV1::from_capture_parts(&capture.source, &capture.base)?;
    let writer = writer_from_options(worktree_root, None);
    let occurred_at = current_timestamp();
    let event_store = EventStore::open(shore_dir);
    let mut recorder = LineageRecorder::default();
    let declaration = ShoreEvent::new(
        EventType::ReviewUnitLineageDeclared,
        ReviewUnitLineageDeclaredPayload::idempotency_key(&lineage_id),
        EventTarget::for_review_unit_lineage(session_id.clone(), lineage_id.clone()),
        writer.clone(),
        ReviewUnitLineageDeclaredPayload {
            lineage_id: lineage_id.clone(),
            basis,
        },
        occurred_at.clone(),
    )?;
    recorder.record(&event_store, declaration)?;

    let round_id = crate::model::ReviewUnitLineageRoundId::from_lineage_review_unit(
        &lineage_id,
        &review_unit_id,
    )?;
    let round = ShoreEvent::new(
        EventType::ReviewUnitLineageRoundRecorded,
        ReviewUnitLineageRoundRecordedPayload::idempotency_key(&lineage_id, &review_unit_id),
        EventTarget::for_review_unit_lineage(session_id, lineage_id.clone()),
        writer,
        ReviewUnitLineageRoundRecordedPayload {
            lineage_id: lineage_id.clone(),
            round_id,
            review_unit_id: review_unit_id.clone(),
            predecessor_review_unit_id: options.predecessor_review_unit_id,
            change_id: options.change_id,
        },
        occurred_at,
    )?;
    recorder.record(&event_store, round)?;

    // The single-writer state.json projects the LOCAL store's events.
    let events_after = event_store.list_events()?;
    let state = SessionState::from_events(&events_after)?;
    storage.write_json_atomic(&paths.state_path(), &state, Durability::Projection)?;

    // The reported head and fork diagnostics must reflect the writer-visible
    // lineage: a cross-worktree predecessor's prior rounds live in the linked
    // store. Re-resolve the store AFTER the writes so its local-only difference
    // now includes the two freshly written lineage events; the union then spans
    // the linked rounds plus the new local ones.
    let projection_store = resolve_write_validation_store(&options.repo)?;
    let projection_events = projection_store.validation_events()?;
    let projection = ReviewUnitLineageProjection::from_events(&projection_events)?;
    let lineage = projection.lineage(&lineage_id).ok_or_else(|| {
        ShoreError::Message(format!(
            "lineage projection missing lineage {} after attach",
            lineage_id.as_str()
        ))
    })?;

    let mut result = LineageAttachResult {
        lineage_id,
        head_review_unit_id: lineage.head_review_unit_id.clone(),
        events_created: recorder.events_created,
        events_existing: recorder.events_existing,
        events_created_by_type: recorder.events_created_by_type,
        diagnostics: lineage.diagnostics.clone(),
    };
    result
        .diagnostics
        .extend(fact_batch_only_diagnostics(&validation_store));
    Ok(result)
}

fn stored_capture_payload(
    events: &[ShoreEvent],
    review_unit_id: &ReviewUnitId,
) -> Result<ReviewUnitCapturedPayload> {
    events
        .iter()
        .filter(|event| event.event_type == EventType::ReviewUnitCaptured)
        .find_map(|event| {
            let payload: ReviewUnitCapturedPayload =
                serde_json::from_value(event.payload.clone()).ok()?;
            (payload.review_unit_id == *review_unit_id).then_some(payload)
        })
        .ok_or_else(|| {
            ShoreError::Message(format!("unknown review unit: {}", review_unit_id.as_str()))
        })
}

fn capture_session_id(
    events: &[ShoreEvent],
    review_unit_id: &ReviewUnitId,
) -> Result<crate::model::SessionId> {
    events
        .iter()
        .filter(|event| event.event_type == EventType::ReviewUnitCaptured)
        .find_map(|event| {
            let payload: ReviewUnitCapturedPayload =
                serde_json::from_value(event.payload.clone()).ok()?;
            (payload.review_unit_id == *review_unit_id).then_some(event.target.session_id.clone())
        })
        .ok_or_else(|| {
            ShoreError::Message(format!("unknown review unit: {}", review_unit_id.as_str()))
        })
}

#[derive(Default)]
struct LineageRecorder {
    events_created: usize,
    events_existing: usize,
    events_created_by_type: BTreeMap<String, usize>,
}

impl LineageRecorder {
    fn record(&mut self, event_store: &EventStore, event: ShoreEvent) -> Result<()> {
        match event_store.record_event_once(&event)? {
            EventWriteOutcome::Created => {
                self.events_created += 1;
                *self
                    .events_created_by_type
                    .entry(event.event_type.as_str().to_owned())
                    .or_default() += 1;
            }
            EventWriteOutcome::Existing | EventWriteOutcome::ExistingDivergentSignature => {
                self.events_existing += 1;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use crate::model::{ReviewUnitId, ReviewUnitLineageId};
    use crate::session::{
        CaptureOptions, LineageAttachOptions, attach_review_unit_to_lineage,
        capture_worktree_review,
    };

    #[test]
    fn attach_review_unit_to_lineage_writes_declaration_and_first_round() {
        let repo = modified_repo();
        let capture = capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();
        let lineage_id = review_unit_lineage_id();

        let result = attach_review_unit_to_lineage(
            LineageAttachOptions::new(repo.path(), lineage_id.clone())
                .with_review_unit_id(capture.review_unit_id.clone()),
        )
        .unwrap();

        assert_eq!(result.lineage_id, lineage_id);
        assert_eq!(result.head_review_unit_id, Some(capture.review_unit_id));
        assert_eq!(
            result.events_created_by_type["review_unit_lineage_declared"],
            1
        );
        assert_eq!(
            result.events_created_by_type["review_unit_lineage_round_recorded"],
            1
        );
    }

    #[test]
    fn attach_rejects_unknown_review_unit() {
        let repo = modified_repo();
        let error = attach_review_unit_to_lineage(
            LineageAttachOptions::new(repo.path(), review_unit_lineage_id())
                .with_review_unit_id(ReviewUnitId::new("review-unit:sha256:missing")),
        )
        .unwrap_err();

        assert!(error.to_string().contains("unknown review unit"));
    }

    #[test]
    fn attaching_same_review_unit_to_same_lineage_is_idempotent() {
        let repo = modified_repo();
        let capture = capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();
        let options = LineageAttachOptions::new(repo.path(), review_unit_lineage_id())
            .with_review_unit_id(capture.review_unit_id);

        let first = attach_review_unit_to_lineage(options.clone()).unwrap();
        let second = attach_review_unit_to_lineage(options).unwrap();

        assert_eq!(first.events_created, 2);
        assert_eq!(second.events_created, 0);
        assert_eq!(second.events_existing, 2);
    }

    #[test]
    fn attaching_fork_reports_lineage_diagnostics_in_write_result() {
        let repo = modified_repo();
        let first = capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();
        let lineage_id = review_unit_lineage_id();
        attach_review_unit_to_lineage(
            LineageAttachOptions::new(repo.path(), lineage_id.clone())
                .with_review_unit_id(first.review_unit_id.clone()),
        )
        .unwrap();

        repo.write("src/lib.rs", "pub fn value() -> u32 { 3 }\n");
        let second = capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();
        attach_review_unit_to_lineage(
            LineageAttachOptions::new(repo.path(), lineage_id.clone())
                .with_review_unit_id(second.review_unit_id)
                .with_predecessor_review_unit_id(first.review_unit_id.clone()),
        )
        .unwrap();

        repo.write("src/lib.rs", "pub fn value() -> u32 { 4 }\n");
        let third = capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();
        let result = attach_review_unit_to_lineage(
            LineageAttachOptions::new(repo.path(), lineage_id)
                .with_review_unit_id(third.review_unit_id)
                .with_predecessor_review_unit_id(first.review_unit_id),
        )
        .unwrap();

        assert!(result.head_review_unit_id.is_none());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "lineage_forked_successor")
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "lineage_multiple_heads")
        );
    }

    #[test]
    fn same_lineage_review_unit_with_different_predecessor_conflicts() {
        let repo = modified_repo();
        let first = capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();
        repo.write("src/lib.rs", "pub fn value() -> u32 { 3 }\n");
        let second = capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();
        let lineage_id = review_unit_lineage_id();

        attach_review_unit_to_lineage(
            LineageAttachOptions::new(repo.path(), lineage_id.clone())
                .with_review_unit_id(second.review_unit_id.clone())
                .with_predecessor_review_unit_id(first.review_unit_id.clone()),
        )
        .unwrap();

        let error = attach_review_unit_to_lineage(
            LineageAttachOptions::new(repo.path(), lineage_id)
                .with_review_unit_id(second.review_unit_id),
        )
        .unwrap_err();

        assert!(error.to_string().contains("event conflict"));
    }

    fn review_unit_lineage_id() -> ReviewUnitLineageId {
        ReviewUnitLineageId::new("review-unit-lineage:random:test")
    }

    fn modified_repo() -> TestRepo {
        let repo = TestRepo::new();
        repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
        repo.commit_all("base");
        repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
        repo
    }

    struct TestRepo {
        root: tempfile::TempDir,
    }

    impl TestRepo {
        fn new() -> Self {
            let root = tempfile::tempdir().expect("create temp git repository directory");
            let repo = Self { root };

            repo.git(["init"]);
            repo.git(["config", "user.name", "Shore Tests"]);
            repo.git(["config", "user.email", "shore-tests@example.com"]);
            repo.git(["config", "commit.gpgsign", "false"]);

            repo
        }

        fn path(&self) -> &Path {
            self.root.path()
        }

        fn write(&self, path: impl AsRef<Path>, contents: impl AsRef<[u8]>) {
            let path = self.root.path().join(path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create parent directories");
            }
            fs::write(path, contents).expect("write test repository file");
        }

        fn commit_all(&self, message: &str) {
            self.git(["add", "--all"]);
            self.git(["commit", "-m", message]);
        }

        fn git<I, S>(&self, args: I)
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let args = args
                .into_iter()
                .map(|arg| arg.as_ref().to_owned())
                .collect::<Vec<_>>();
            let output = Command::new("git")
                .args(&args)
                .current_dir(self.root.path())
                .output()
                .unwrap_or_else(|error| panic!("run git {:?}: {error}", args));

            assert!(
                output.status.success(),
                "git {:?} failed\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
                args,
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
