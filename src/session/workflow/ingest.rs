use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::{Result, ShoreError};
use crate::session::event::ShoreEvent;
use crate::session::state::{ProjectionDiagnostic, SessionState};
use crate::session::store_init::{ShoreStorePaths, prepare_shore_writer};
use crate::session::{EventStore, EventWriteOutcome, is_valid_actor_id};
use crate::storage::{Durability, LocalStorage};

/// Options for ingesting one or more pre-formed events into a repo's `.shore`
/// store — for example events produced on another machine and forwarded over a
/// network, or merged from another clone.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestEventsOptions {
    repo: PathBuf,
    events: Vec<ShoreEvent>,
}

impl IngestEventsOptions {
    pub fn new(repo: impl AsRef<Path>, events: Vec<ShoreEvent>) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            events,
        }
    }
}

/// Options for ingesting a single pre-formed event. Thin convenience over
/// [`IngestEventsOptions`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportEventOptions {
    repo: PathBuf,
    event: ShoreEvent,
}

impl ImportEventOptions {
    pub fn new(repo: impl AsRef<Path>, event: ShoreEvent) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            event,
        }
    }
}

/// The outcome of an ingest: how many events were newly written vs. already
/// present (idempotent re-ingest), a per-type breakdown of the newly written
/// events, and the projection diagnostics after the rebuild.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestEventsResult {
    pub events_created: usize,
    pub events_existing: usize,
    pub events_created_by_type: BTreeMap<String, usize>,
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

/// Ingest a single pre-formed event. See [`ingest_events`].
pub fn import_event(options: ImportEventOptions) -> Result<IngestEventsResult> {
    ingest_events(IngestEventsOptions::new(options.repo, vec![options.event]))
}

/// Ingest pre-formed events into the repo's durable store, preserving the
/// store's append-only, content-addressed, idempotent, and conflict semantics.
///
/// Each event is recorded through the same primitive the domain write workflows
/// use, so a re-ingest of an already-present event is a no-op (`events_existing`)
/// and an event that collides with a different payload under the same
/// idempotency key is rejected. Malformed envelopes (bad `eventId`/`payloadHash`
/// /schema) are rejected, and the writer attribution is validated up front: an
/// event whose `writer.actor_id` is not a well-formed `actor:` id is rejected
/// before anything is written, so the whole batch is atomic on attribution.
///
/// After recording, the projection (`state.json`) is rebuilt once from the full
/// event log. If a write fails partway through a batch (e.g. a conflict), the
/// events already written remain durable and the projection is still rebuilt to
/// match what is on disk before the error is returned — re-ingesting the batch
/// is safe.
pub fn ingest_events(options: IngestEventsOptions) -> Result<IngestEventsResult> {
    let paths = ShoreStorePaths::resolve(&options.repo)?;
    let shore_dir = paths.shore_dir();
    let storage = LocalStorage::new(shore_dir);
    prepare_shore_writer(&paths, &storage)?;

    // Reject malformed attribution before any write so the batch is atomic on
    // attribution: a bad actor id can never partially corrupt the log.
    for event in &options.events {
        if !is_valid_actor_id(event.writer.actor_id.as_str()) {
            return Err(ShoreError::InvalidEvent {
                message: format!(
                    "ingested event {} has a malformed writer actor id: {}",
                    event.event_id.as_str(),
                    event.writer.actor_id.as_str()
                ),
            });
        }
    }

    let event_store = EventStore::open(shore_dir);
    let mut events_created = 0usize;
    let mut events_existing = 0usize;
    let mut events_created_by_type: BTreeMap<String, usize> = BTreeMap::new();
    let mut write_error = None;

    for event in &options.events {
        match event_store.record_event_once(event) {
            Ok(EventWriteOutcome::Created) => {
                events_created += 1;
                *events_created_by_type
                    .entry(event.event_type.as_str().to_owned())
                    .or_default() += 1;
            }
            Ok(EventWriteOutcome::Existing) => events_existing += 1,
            Err(err) => {
                write_error = Some(err);
                break;
            }
        }
    }

    // Rebuild the projection from whatever is durably on disk — even on a
    // partial-batch failure — so state.json never drifts from the event log.
    let events = event_store.list_events()?;
    let state = SessionState::from_events(&events)?;
    storage.write_json_atomic(&paths.state_path(), &state, Durability::Projection)?;

    if let Some(err) = write_error {
        return Err(err);
    }

    Ok(IngestEventsResult {
        events_created,
        events_existing,
        events_created_by_type,
        diagnostics: state.diagnostics,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::*;
    use crate::model::ActorId;
    use crate::session::event::{EventType, InputRequestReasonCode, InputRequestResponseOutcome};
    use crate::session::{
        CaptureOptions, InputRequestListOptions, InputRequestOpenOptions,
        InputRequestRespondOptions, InputRequestStatus, InputRequestStatusFilter,
        capture_worktree_review, list_input_requests, open_input_request, respond_input_request,
    };

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

        fn write(&self, path: &str, contents: &str) {
            let path = self.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }

        fn commit_all(&self, message: &str) {
            self.git(["add", "."]);
            self.git(["commit", "-m", message]);
        }

        fn git<I, S>(&self, args: I)
        where
            I: IntoIterator<Item = S>,
            S: AsRef<std::ffi::OsStr>,
        {
            let output = Command::new("git")
                .args(args)
                .current_dir(self.path())
                .output()
                .expect("run git command");
            assert!(output.status.success(), "git failed");
        }
    }

    fn modified_repo() -> TestRepo {
        let repo = TestRepo::new();
        repo.write("src/lib.rs", "pub fn value() -> u32 {\n    1\n}\n");
        repo.commit_all("base");
        repo.write("src/lib.rs", "pub fn value() -> u32 {\n    2\n}\n");
        repo
    }

    /// Build an origin store with a captured review unit + one responded input
    /// request, returning its full event log.
    fn origin_events() -> (TestRepo, Vec<ShoreEvent>) {
        let repo = modified_repo();
        capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();
        let opened = open_input_request(
            InputRequestOpenOptions::new(repo.path())
                .with_track("human:kevin")
                .with_title("Need approval")
                .with_reason_code(InputRequestReasonCode::ManualDecisionRequired),
        )
        .unwrap();
        respond_input_request(
            InputRequestRespondOptions::new(repo.path(), opened.input_request_id.clone())
                .with_outcome(InputRequestResponseOutcome::Approved)
                .with_actor_id(ActorId::new("actor:agent:remote-reviewer")),
        )
        .unwrap();
        let events = EventStore::open(repo.path().join(".shore"))
            .list_events()
            .unwrap();
        (repo, events)
    }

    fn dest_repo() -> TestRepo {
        // The destination only needs a valid repo root to host its own .shore.
        modified_repo()
    }

    fn on_disk_state(repo: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(repo.join(".shore/state.json")).unwrap())
            .unwrap()
    }

    fn replayed_state(repo: &Path) -> serde_json::Value {
        let events = EventStore::open(repo.join(".shore")).list_events().unwrap();
        serde_json::to_value(SessionState::from_events(&events).unwrap()).unwrap()
    }

    #[test]
    fn ingest_events_reconstructs_projection_and_is_idempotent() {
        let (_origin, events) = origin_events();
        let total = events.len();
        assert!(
            total >= 3,
            "expected captured + opened + responded, got {total}"
        );
        let dest = dest_repo();

        let first = ingest_events(IngestEventsOptions::new(dest.path(), events.clone())).unwrap();
        assert_eq!(first.events_created, total);
        assert_eq!(first.events_existing, 0);

        // The forwarded responded input request is visible in the destination.
        let listed = list_input_requests(
            InputRequestListOptions::new(dest.path()).with_status(InputRequestStatusFilter::All),
        )
        .unwrap();
        assert_eq!(listed.input_requests.len(), 1);
        assert_eq!(
            listed.input_requests[0].status,
            InputRequestStatus::Responded
        );
        // The forwarded actor attribution is preserved through ingest.
        assert_eq!(
            listed.input_requests[0].responses[0]
                .writer
                .actor_id
                .as_str(),
            "actor:agent:remote-reviewer"
        );

        // Projection equals a full replay, and re-ingest is a no-op.
        assert_eq!(on_disk_state(dest.path()), replayed_state(dest.path()));
        let second = ingest_events(IngestEventsOptions::new(dest.path(), events)).unwrap();
        assert_eq!(second.events_created, 0);
        assert_eq!(second.events_existing, total);
    }

    #[test]
    fn import_event_records_a_single_event() {
        let (_origin, events) = origin_events();
        let captured = events
            .iter()
            .find(|event| event.event_type == EventType::ReviewUnitCaptured)
            .unwrap()
            .clone();
        let dest = dest_repo();

        let result = import_event(ImportEventOptions::new(dest.path(), captured.clone())).unwrap();
        assert_eq!(result.events_created, 1);
        assert_eq!(result.events_created_by_type["review_unit_captured"], 1);

        let again = import_event(ImportEventOptions::new(dest.path(), captured)).unwrap();
        assert_eq!(again.events_created, 0);
        assert_eq!(again.events_existing, 1);
    }

    #[test]
    fn ingest_rejects_malformed_writer_actor_id() {
        let (_origin, events) = origin_events();
        let mut bad = events[0].clone();
        bad.writer.actor_id = ActorId::new("not-an-actor-id");
        let dest = dest_repo();

        let error = import_event(ImportEventOptions::new(dest.path(), bad)).unwrap_err();
        assert!(
            error.to_string().contains("malformed writer actor id"),
            "unexpected error: {error}"
        );
        // Nothing was written (attribution is validated before any write).
        assert!(
            !dest.path().join(".shore/events").exists() || {
                EventStore::open(dest.path().join(".shore"))
                    .list_events()
                    .unwrap()
                    .is_empty()
            }
        );
    }

    #[test]
    fn ingest_conflict_mid_batch_keeps_projection_consistent_with_disk() {
        let (_origin, events) = origin_events();
        let opened = events
            .iter()
            .find(|event| event.event_type == EventType::InputRequestOpened)
            .unwrap()
            .clone();
        // A conflicting event: same idempotency key (and eventId) but a different
        // payload under a recomputed, self-consistent payload hash.
        let mut conflict = opened.clone();
        let mut payload = conflict.payload.clone();
        payload["title"] = serde_json::json!("a different title");
        conflict.payload = payload;
        conflict.payload_hash =
            crate::canonical_hash::sha256_json_prefixed(&conflict.payload).unwrap();

        let captured = events
            .iter()
            .find(|event| event.event_type == EventType::ReviewUnitCaptured)
            .unwrap()
            .clone();

        let dest = dest_repo();
        let batch = vec![captured, opened, conflict];
        let error = ingest_events(IngestEventsOptions::new(dest.path(), batch)).unwrap_err();
        assert!(
            error.to_string().contains("conflict"),
            "unexpected error: {error}"
        );

        // The good events are durable and the projection matches the event log on disk.
        assert_eq!(on_disk_state(dest.path()), replayed_state(dest.path()));
    }
}
