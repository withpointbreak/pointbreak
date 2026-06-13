use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::{Result, ShoreError};
use crate::session::event::{IngestVia, ShoreEvent, stamp_ingest_provenance};
use crate::session::state::{ProjectionDiagnostic, SessionState};
use crate::session::store_init::{ShoreStorePaths, prepare_shore_writer};
use crate::session::{
    EventStore, EventVerificationPolicy, EventWriteOutcome, IngestEventVerification, TrustSet,
    current_timestamp, is_valid_actor_id, verify_events_for_ingest,
};
use crate::storage::{Durability, LocalStorage};

const DIVERGENT_SIGNATURE_EXISTING_EVENT_CODE: &str = "divergent_signature_existing_event";

/// Options for ingesting one or more pre-formed events into a repo's `.shore`
/// store — for example events produced on another machine and forwarded over a
/// network, or merged from another clone.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestEventsOptions {
    repo: PathBuf,
    events: Vec<ShoreEvent>,
    verification_policy: EventVerificationPolicy,
    trust_set: TrustSet,
}

impl IngestEventsOptions {
    pub fn new(repo: impl AsRef<Path>, events: Vec<ShoreEvent>) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            events,
            verification_policy: EventVerificationPolicy::advisory(),
            trust_set: TrustSet::default(),
        }
    }

    pub fn with_verification_policy(mut self, policy: EventVerificationPolicy) -> Self {
        self.verification_policy = policy;
        self
    }

    pub fn with_trust_set(mut self, trust_set: TrustSet) -> Self {
        self.trust_set = trust_set;
        self
    }
}

/// Options for ingesting a single pre-formed event. Thin convenience over
/// [`IngestEventsOptions`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportEventOptions {
    repo: PathBuf,
    event: ShoreEvent,
    verification_policy: EventVerificationPolicy,
    trust_set: TrustSet,
}

impl ImportEventOptions {
    pub fn new(repo: impl AsRef<Path>, event: ShoreEvent) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            event,
            verification_policy: EventVerificationPolicy::advisory(),
            trust_set: TrustSet::default(),
        }
    }

    pub fn with_verification_policy(mut self, policy: EventVerificationPolicy) -> Self {
        self.verification_policy = policy;
        self
    }

    pub fn with_trust_set(mut self, trust_set: TrustSet) -> Self {
        self.trust_set = trust_set;
        self
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
    pub verification: Vec<IngestEventVerification>,
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

/// Ingest a single pre-formed event. See [`ingest_events`].
pub fn import_event(options: ImportEventOptions) -> Result<IngestEventsResult> {
    ingest_events(
        IngestEventsOptions::new(options.repo, vec![options.event])
            .with_verification_policy(options.verification_policy)
            .with_trust_set(options.trust_set),
    )
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

    let verification = verify_events_for_ingest(
        &options.events,
        options.verification_policy,
        &options.trust_set,
    )?;

    let stamped = stamp_ingest_provenance(
        &options.events,
        IngestVia::IngestEvents,
        &current_timestamp(),
    );

    let event_store = EventStore::open(shore_dir);
    let mut events_created = 0usize;
    let mut events_existing = 0usize;
    let mut events_created_by_type: BTreeMap<String, usize> = BTreeMap::new();
    let mut ingest_diagnostics = Vec::new();
    let mut write_error = None;

    for event in &stamped {
        match event_store.record_event_once(event) {
            Ok(EventWriteOutcome::Created) => {
                events_created += 1;
                *events_created_by_type
                    .entry(event.event_type.as_str().to_owned())
                    .or_default() += 1;
            }
            Ok(EventWriteOutcome::Existing) => events_existing += 1,
            Ok(EventWriteOutcome::ExistingDivergentSignature) => {
                events_existing += 1;
                ingest_diagnostics.push(ProjectionDiagnostic {
                    code: DIVERGENT_SIGNATURE_EXISTING_EVENT_CODE.to_owned(),
                    message: format!(
                        "ingested event {} matched existing idempotency key {} and payload hash, but signer or signature differed; kept the first stored event",
                        event.event_id.as_str(),
                        event.idempotency_key.as_str()
                    ),
                });
            }
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
    let mut diagnostics = state.diagnostics;
    diagnostics.extend(ingest_diagnostics);

    if let Some(err) = write_error {
        return Err(err);
    }

    Ok(IngestEventsResult {
        events_created,
        events_existing,
        events_created_by_type,
        verification,
        diagnostics,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use serde_json::json;

    use super::*;
    use crate::canonical_hash::sha256_json_prefixed;
    use crate::crypto::{EventVerificationStatus, SignerId};
    use crate::model::{
        ActorId, InputRequestId, InputRequestResponseId, SessionId, TargetRef, TaskTargetRef,
        WorkObjectId,
    };
    use crate::session::event::{
        AssertionMode, EventSignature, EventType, IngestProvenance, IngestVia,
        InputRequestReasonCode, InputRequestResponseOutcome,
    };
    use crate::session::projection::task::{
        AgentResumptionProjection, AgentResumptionState, ResumptionBindingPolicy,
        agent_resumption_from_events,
    };
    use crate::session::projection::test_support::{
        reader_actor, task_attempt_event, task_input_request_event_with_target, user_response_event,
    };
    use crate::session::signing::test_support::{DeterministicSigner, trust_for_actor};
    use crate::session::{
        CaptureOptions, EventVerificationPolicy, InputRequestListOptions, InputRequestOpenOptions,
        InputRequestRespondOptions, InputRequestStatus, InputRequestStatusFilter, TrustSet,
        capture_worktree_review, list_input_requests, open_input_request, respond_input_request,
        verify_event_signature,
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

    fn signed_captured_event() -> (ShoreEvent, DeterministicSigner, ActorId) {
        let repo = modified_repo();
        let signer = DeterministicSigner::fixture();
        let actor = ActorId::new("actor:git-email:alice@example.com");
        capture_worktree_review(
            CaptureOptions::new(repo.path())
                .with_actor_id(actor.clone())
                .sign_with(signer.clone()),
        )
        .unwrap();
        let event = EventStore::open(repo.path().join(".shore"))
            .list_events()
            .unwrap()
            .into_iter()
            .find(|event| event.event_type == EventType::ReviewUnitCaptured)
            .unwrap();

        (event, signer, actor)
    }

    fn invalid_signed_event() -> (ShoreEvent, TrustSet) {
        let (mut event, signer, actor) = signed_captured_event();
        event.payload["tamperedAfterSigning"] = json!(true);
        event.payload_hash = sha256_json_prefixed(&event.payload).unwrap();
        let trust = trust_for_actor(&actor, &signer);

        (event, trust)
    }

    fn unsigned_event() -> ShoreEvent {
        let (_origin, events) = origin_events();
        events
            .into_iter()
            .find(|event| event.event_type == EventType::ReviewUnitCaptured)
            .unwrap()
    }

    fn stored_event_count(repo: &Path) -> usize {
        let events_dir = repo.join(".shore/events");
        if !events_dir.exists() {
            return 0;
        }

        EventStore::open(repo.join(".shore"))
            .list_events()
            .unwrap()
            .len()
    }

    fn signed_replay_event(signature: &str) -> ShoreEvent {
        let mut event = unsigned_event();
        event.signer = Some(
            SignerId::parse("did:key:z6MkehRgf7yJbgaGfYsdoAsKdBPE3dj2CYhowQdcjqSJgvVd").unwrap(),
        );
        event.signature = Some(EventSignature::new_ed25519_v1(signature).unwrap());
        event
    }

    #[test]
    fn advisory_ingest_accepts_invalid_signature_and_reports_status() {
        let (event, trust) = invalid_signed_event();
        let dest = dest_repo();

        let result = ingest_events(
            IngestEventsOptions::new(dest.path(), vec![event.clone()])
                .with_verification_policy(EventVerificationPolicy::advisory())
                .with_trust_set(trust),
        )
        .unwrap();

        assert_eq!(result.events_created, 1);
        assert_eq!(result.verification.len(), 1);
        assert_eq!(result.verification[0].event_id, event.event_id);
        assert_eq!(
            result.verification[0].status,
            EventVerificationStatus::Invalid
        );

        let (untrusted, _signer, _actor) = signed_captured_event();
        let untrusted_dest = dest_repo();
        let untrusted_result = ingest_events(
            IngestEventsOptions::new(untrusted_dest.path(), vec![untrusted])
                .with_verification_policy(EventVerificationPolicy::advisory()),
        )
        .unwrap();
        assert_eq!(untrusted_result.events_created, 1);
        assert_eq!(
            untrusted_result.verification[0].status,
            EventVerificationStatus::UntrustedKey
        );

        let unsigned = unsigned_event();
        let unsigned_dest = dest_repo();
        let unsigned_result = ingest_events(
            IngestEventsOptions::new(unsigned_dest.path(), vec![unsigned])
                .with_verification_policy(EventVerificationPolicy::advisory()),
        )
        .unwrap();
        assert_eq!(unsigned_result.events_created, 1);
        assert_eq!(
            unsigned_result.verification[0].status,
            EventVerificationStatus::Unsigned
        );
    }

    #[test]
    fn integrity_strict_rejects_invalid_but_accepts_unsigned() {
        let (invalid, trust) = invalid_signed_event();
        let rejected_dest = dest_repo();

        let error = ingest_events(
            IngestEventsOptions::new(rejected_dest.path(), vec![invalid])
                .with_verification_policy(EventVerificationPolicy::integrity_strict())
                .with_trust_set(trust),
        )
        .unwrap_err();

        assert!(
            error.to_string().contains("invalid"),
            "unexpected error: {error}"
        );
        assert_eq!(stored_event_count(rejected_dest.path()), 0);

        let unsigned = unsigned_event();
        let accepted_dest = dest_repo();
        let result = ingest_events(
            IngestEventsOptions::new(accepted_dest.path(), vec![unsigned])
                .with_verification_policy(EventVerificationPolicy::integrity_strict()),
        )
        .unwrap();

        assert_eq!(result.events_created, 1);
        assert_eq!(
            result.verification[0].status,
            EventVerificationStatus::Unsigned
        );
    }

    #[test]
    fn trusted_strict_rejects_untrusted_and_unsigned_unless_allowed() {
        let (untrusted, _signer, _actor) = signed_captured_event();
        let untrusted_dest = dest_repo();

        let untrusted_error = ingest_events(
            IngestEventsOptions::new(untrusted_dest.path(), vec![untrusted])
                .with_verification_policy(EventVerificationPolicy::trusted_strict())
                .with_trust_set(TrustSet::default()),
        )
        .unwrap_err();

        assert!(
            untrusted_error.to_string().contains("untrusted_key"),
            "unexpected error: {untrusted_error}"
        );
        assert_eq!(stored_event_count(untrusted_dest.path()), 0);

        let unsigned = unsigned_event();
        let unsigned_dest = dest_repo();
        let unsigned_error = ingest_events(
            IngestEventsOptions::new(unsigned_dest.path(), vec![unsigned.clone()])
                .with_verification_policy(EventVerificationPolicy::trusted_strict()),
        )
        .unwrap_err();

        assert!(
            unsigned_error.to_string().contains("unsigned"),
            "unexpected error: {unsigned_error}"
        );
        assert_eq!(stored_event_count(unsigned_dest.path()), 0);

        let allowed_unsigned_dest = dest_repo();
        let result = ingest_events(
            IngestEventsOptions::new(allowed_unsigned_dest.path(), vec![unsigned])
                .with_verification_policy(
                    EventVerificationPolicy::trusted_strict().with_allow_unsigned(true),
                ),
        )
        .unwrap();

        assert_eq!(result.events_created, 1);
        assert_eq!(
            result.verification[0].status,
            EventVerificationStatus::Unsigned
        );
    }

    #[test]
    fn ingest_reports_divergent_signature_existing_event_diagnostic() {
        let first = signed_replay_event(
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
        );
        let mut second = first.clone();
        second.signature = Some(
            EventSignature::new_ed25519_v1(
                "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
            )
            .unwrap(),
        );
        let dest = dest_repo();

        let first_result =
            ingest_events(IngestEventsOptions::new(dest.path(), vec![first.clone()])).unwrap();
        assert_eq!(first_result.events_created, 1);

        let second_result =
            ingest_events(IngestEventsOptions::new(dest.path(), vec![second.clone()])).unwrap();

        assert_eq!(second_result.events_created, 0);
        assert_eq!(second_result.events_existing, 1);
        assert!(second_result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "divergent_signature_existing_event"
                && diagnostic.message.contains(second.event_id.as_str())
                && diagnostic.message.contains(second.idempotency_key.as_str())
        }));
        let mut stored = EventStore::open(dest.path().join(".shore"))
            .list_events()
            .unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(
            stored[0].ingest.as_ref().unwrap().via,
            IngestVia::IngestEvents
        );
        stored[0].ingest = None;
        assert_eq!(stored, vec![first]);
    }

    #[test]
    fn ingest_treats_timestamp_only_unsigned_replay_as_existing_without_signature_diagnostic() {
        let first = unsigned_event();
        let mut second = first.clone();
        second.occurred_at = "2026-06-04T00:00:00Z".to_owned();
        let dest = dest_repo();

        let first_result =
            ingest_events(IngestEventsOptions::new(dest.path(), vec![first.clone()])).unwrap();
        assert_eq!(first_result.events_created, 1);

        let second_result =
            ingest_events(IngestEventsOptions::new(dest.path(), vec![second])).unwrap();

        assert_eq!(second_result.events_created, 0);
        assert_eq!(second_result.events_existing, 1);
        assert!(
            second_result
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "divergent_signature_existing_event")
        );
        let mut stored = EventStore::open(dest.path().join(".shore"))
            .list_events()
            .unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(
            stored[0].ingest.as_ref().unwrap().via,
            IngestVia::IngestEvents
        );
        stored[0].ingest = None;
        assert_eq!(stored, vec![first]);
    }

    #[test]
    fn ingest_stamps_ingest_provenance_on_every_written_event() {
        let (_origin, events) = origin_events();
        let dest = dest_repo();

        ingest_events(IngestEventsOptions::new(dest.path(), events.clone())).unwrap();

        let stored = EventStore::open(dest.path().join(".shore"))
            .list_events()
            .unwrap();
        assert_eq!(stored.len(), events.len());
        for event in &stored {
            let stamp = event
                .ingest
                .as_ref()
                .expect("every ingested event is stamped");
            assert_eq!(stamp.via, IngestVia::IngestEvents);
            assert!(stamp.received_at.starts_with("unix-ms:"));
        }
    }

    #[test]
    fn ingest_overwrites_inbound_ingest_stamp_with_local_stamp() {
        // A stamp in arriving bytes is some other store's bookkeeping; only the
        // local importer's stamp means anything here (ADR-0009).
        let mut event = unsigned_event();
        event.ingest = Some(IngestProvenance {
            via: IngestVia::BundleApply,
            received_at: "unix-ms:1".to_owned(),
        });
        let dest = dest_repo();

        import_event(ImportEventOptions::new(dest.path(), event)).unwrap();

        let stored = EventStore::open(dest.path().join(".shore"))
            .list_events()
            .unwrap();
        let stamp = stored[0].ingest.as_ref().unwrap();
        assert_eq!(stamp.via, IngestVia::IngestEvents);
        assert_ne!(stamp.received_at, "unix-ms:1");
    }

    #[test]
    fn reingest_of_locally_authored_event_leaves_stored_event_unstamped() {
        // Author locally, then ingest the store's own events back into it:
        // Existing outcome, first-stored-wins, the stored files stay unstamped —
        // a locally authored event can never acquire a stamp after the fact.
        let (origin, events) = origin_events();

        let result =
            ingest_events(IngestEventsOptions::new(origin.path(), events.clone())).unwrap();
        assert_eq!(result.events_created, 0);
        assert_eq!(result.events_existing, events.len());

        let stored = EventStore::open(origin.path().join(".shore"))
            .list_events()
            .unwrap();
        assert!(stored.iter().all(|event| event.ingest.is_none()));
    }

    #[test]
    fn reingest_of_ingested_event_keeps_first_stamp() {
        // Ingest twice into the same destination; the stored stamp does not
        // change on the second pass — an ingested event can never lose (or
        // churn) its stamp.
        let (_origin, events) = origin_events();
        let dest = dest_repo();

        ingest_events(IngestEventsOptions::new(dest.path(), events.clone())).unwrap();
        let first_stamps: Vec<_> = EventStore::open(dest.path().join(".shore"))
            .list_events()
            .unwrap()
            .into_iter()
            .map(|event| event.ingest)
            .collect();
        assert!(first_stamps.iter().all(Option::is_some));

        let second = ingest_events(IngestEventsOptions::new(dest.path(), events)).unwrap();
        assert_eq!(second.events_created, 0);

        let second_stamps: Vec<_> = EventStore::open(dest.path().join(".shore"))
            .list_events()
            .unwrap()
            .into_iter()
            .map(|event| event.ingest)
            .collect();
        assert_eq!(second_stamps, first_stamps);
    }

    #[test]
    fn stamped_signed_event_still_verifies_valid_after_ingest() {
        let (event, signer, actor) = signed_captured_event();
        let trust = trust_for_actor(&actor, &signer);
        let dest = dest_repo();

        ingest_events(
            IngestEventsOptions::new(dest.path(), vec![event]).with_trust_set(trust.clone()),
        )
        .unwrap();

        let stored = EventStore::open(dest.path().join(".shore"))
            .list_events()
            .unwrap();
        assert!(stored[0].ingest.is_some());
        assert_eq!(
            verify_event_signature(&stored[0], &trust).unwrap(),
            EventVerificationStatus::Valid
        );
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

    // -- end-to-end: ingest/bundle -> resumption binding (ADR-0009) ----------
    //
    // The relay consequence is implicit throughout: a relay that strips
    // signatures converts a bindable response into ingested_unsigned.

    /// Task-shaped origin event set: attempt + operative task-targeted input
    /// request + operative Approved response (last element).
    fn task_resumption_events() -> (Vec<ShoreEvent>, WorkObjectId) {
        let task_attempt_id = WorkObjectId::new("task-attempt:sha256:ta");
        let session_id = SessionId::new("session:claude:uuid-1");
        let input_request_id = InputRequestId::new("input-request:sha256:1");
        let response_id = InputRequestResponseId::new("input-request-response:sha256:r");

        let events = vec![
            task_attempt_event(
                &task_attempt_id,
                &session_id,
                "uuid-1",
                "2026-05-18T00:00:00Z",
            ),
            task_input_request_event_with_target(
                &task_attempt_id,
                &session_id,
                &input_request_id,
                "source:approve",
                "2026-05-18T00:00:02Z",
                TargetRef::Task(TaskTargetRef::TaskAttempt),
                "needs approval",
            ),
            user_response_event(
                &input_request_id,
                &response_id,
                InputRequestResponseOutcome::Approved,
                AssertionMode::Operative,
                "2026-05-18T00:00:03Z",
            ),
        ];
        (events, task_attempt_id)
    }

    /// Writes the events as local-authored facts — the same store primitive
    /// the domain workflows and the adapter write path use; no seam, no stamp.
    fn local_authored_store(events: &[ShoreEvent]) -> (tempfile::TempDir, EventStore) {
        let root = tempfile::tempdir().unwrap();
        let store = EventStore::open(root.path().join(".shore"));
        for event in events {
            store.record_event_once(event).unwrap();
        }
        (root, store)
    }

    fn resumption_projection(
        stored: &[ShoreEvent],
        task_attempt_id: &WorkObjectId,
        trust: &TrustSet,
        policy: ResumptionBindingPolicy,
    ) -> AgentResumptionProjection {
        agent_resumption_from_events(stored, task_attempt_id, &reader_actor(), trust, policy)
            .unwrap()
    }

    fn identity_reason(projection: &AgentResumptionProjection) -> Option<String> {
        projection
            .diagnostics
            .iter()
            .find(|d| d.code == "agent_resumption_response_identity_not_binding")
            .and_then(|d| d.reason.clone())
    }

    #[test]
    fn local_unsigned_response_binds_in_its_own_store_zero_config() {
        // Possession is the trust root: a human responding in their own
        // worktree binds with zero keys, zero configuration.
        let (events, task_attempt_id) = task_resumption_events();
        let (_root, store) = local_authored_store(&events);

        let stored = store.list_events().unwrap();
        assert!(stored.iter().all(|event| event.ingest.is_none()));
        let projection = resumption_projection(
            &stored,
            &task_attempt_id,
            &TrustSet::default(),
            ResumptionBindingPolicy::default(),
        );

        assert!(projection.may_resume);
        assert_eq!(projection.state, AgentResumptionState::Ready);
    }

    #[test]
    fn ingested_unsigned_response_is_blocked_ingested_unsigned_in_destination() {
        let (events, task_attempt_id) = task_resumption_events();
        let dest = dest_repo();

        ingest_events(IngestEventsOptions::new(dest.path(), events)).unwrap();

        let stored = EventStore::open(dest.path().join(".shore"))
            .list_events()
            .unwrap();
        let projection = resumption_projection(
            &stored,
            &task_attempt_id,
            &TrustSet::default(),
            ResumptionBindingPolicy::default(),
        );

        assert!(!projection.may_resume);
        assert_eq!(projection.state, AgentResumptionState::Blocked);
        assert_eq!(
            identity_reason(&projection).as_deref(),
            Some("ingested_unsigned")
        );
        // The forwarded claimed actorId is preserved as a reported fact but
        // does not bind.
        let response_view = projection.selected_response.as_ref().unwrap();
        assert_eq!(
            response_view.envelope.writer.actor_id.as_str(),
            "actor:claude_code:user"
        );
        assert!(!response_view.identity_treated_as_binding);
    }

    #[test]
    fn ingested_reviewer_signed_response_binds_via_verified_signer_arm() {
        // The reviewer holds the key end-to-end: the response is signed
        // before it ever leaves the origin.
        let (mut events, task_attempt_id) = task_resumption_events();
        let signer = DeterministicSigner::fixture();
        crate::session::sign_event_if_requested(
            events.last_mut().expect("response event"),
            &crate::session::EventSigningOptions::sign_with(signer.clone()),
        )
        .unwrap();
        let trust = trust_for_actor(&ActorId::new("actor:claude_code:user"), &signer);
        let dest = dest_repo();

        ingest_events(IngestEventsOptions::new(dest.path(), events).with_trust_set(trust.clone()))
            .unwrap();

        let stored = EventStore::open(dest.path().join(".shore"))
            .list_events()
            .unwrap();
        let response = stored
            .iter()
            .find(|event| event.event_type == EventType::InputRequestResponded)
            .unwrap();
        assert!(response.ingest.is_some(), "seam stamped the stored copy");
        let projection = resumption_projection(
            &stored,
            &task_attempt_id,
            &trust,
            ResumptionBindingPolicy::default(),
        );

        assert!(projection.may_resume);
        assert_eq!(projection.state, AgentResumptionState::Ready);
    }

    #[test]
    fn signature_stripped_in_transit_response_is_blocked_ingested_unsigned() {
        // The relay consequence: a hop that strips signatures converts a
        // bindable response into ingested_unsigned.
        let (mut events, task_attempt_id) = task_resumption_events();
        let signer = DeterministicSigner::fixture();
        let response = events.last_mut().expect("response event");
        crate::session::sign_event_if_requested(
            response,
            &crate::session::EventSigningOptions::sign_with(signer.clone()),
        )
        .unwrap();
        response.signer = None;
        response.signature = None;
        let trust = trust_for_actor(&ActorId::new("actor:claude_code:user"), &signer);
        let dest = dest_repo();

        ingest_events(IngestEventsOptions::new(dest.path(), events).with_trust_set(trust.clone()))
            .unwrap();

        let stored = EventStore::open(dest.path().join(".shore"))
            .list_events()
            .unwrap();
        let projection = resumption_projection(
            &stored,
            &task_attempt_id,
            &trust,
            ResumptionBindingPolicy::default(),
        );

        assert!(!projection.may_resume);
        assert_eq!(
            identity_reason(&projection).as_deref(),
            Some("ingested_unsigned")
        );
    }

    #[test]
    fn bundle_applied_response_has_parity_with_ingest() {
        use crate::session::store::bundle::import_store_bundle;

        // Unsigned: bundle apply stamps, so the response stops binding.
        let (events, task_attempt_id) = task_resumption_events();
        let (_source_root, _source_store) = local_authored_store(&events);
        let target = tempfile::tempdir().unwrap();
        import_store_bundle(
            _source_root.path().join(".shore"),
            target.path().join(".shore"),
        )
        .unwrap();
        let stored = EventStore::open(target.path().join(".shore"))
            .list_events()
            .unwrap();
        let projection = resumption_projection(
            &stored,
            &task_attempt_id,
            &TrustSet::default(),
            ResumptionBindingPolicy::default(),
        );
        assert!(!projection.may_resume);
        assert_eq!(
            identity_reason(&projection).as_deref(),
            Some("ingested_unsigned")
        );

        // Signed + authorized: binds via arm (b) through bundle apply too.
        let (mut signed_events, task_attempt_id) = task_resumption_events();
        let signer = DeterministicSigner::fixture();
        crate::session::sign_event_if_requested(
            signed_events.last_mut().expect("response event"),
            &crate::session::EventSigningOptions::sign_with(signer.clone()),
        )
        .unwrap();
        let trust = trust_for_actor(&ActorId::new("actor:claude_code:user"), &signer);
        let (signed_source_root, _signed_source_store) = local_authored_store(&signed_events);
        let signed_target = tempfile::tempdir().unwrap();
        import_store_bundle(
            signed_source_root.path().join(".shore"),
            signed_target.path().join(".shore"),
        )
        .unwrap();
        let stored = EventStore::open(signed_target.path().join(".shore"))
            .list_events()
            .unwrap();
        let projection = resumption_projection(
            &stored,
            &task_attempt_id,
            &trust,
            ResumptionBindingPolicy::default(),
        );
        assert!(projection.may_resume);
        assert_eq!(projection.state, AgentResumptionState::Ready);
    }

    #[test]
    fn verified_only_store_blocks_even_its_own_unsigned_response() {
        // Choosing verified-only is choosing that nothing binds without a
        // key — including the store's own unsigned responses.
        let (events, task_attempt_id) = task_resumption_events();
        let (_root, store) = local_authored_store(&events);

        let stored = store.list_events().unwrap();
        let projection = resumption_projection(
            &stored,
            &task_attempt_id,
            &TrustSet::default(),
            ResumptionBindingPolicy::VerifiedOnly,
        );

        assert!(!projection.may_resume);
        assert_eq!(projection.state, AgentResumptionState::Blocked);
        assert_eq!(
            identity_reason(&projection).as_deref(),
            Some("policy_excludes_local")
        );
    }

    // ADR-0009 through the linked-read seam: `store link` stamps every copied
    // event with ingest provenance, and the binding predicate is a pure
    // function of the events actually read — never of which store reads them.
    // The linked store's copies therefore behave exactly like any ingested
    // events, with no special case for "my own link import". These fixtures
    // extend the binding outcome matrix in projection/task.rs through a real
    // worktree pair and the read seam.

    /// A committed main repo plus seed and reader worktrees, with `events`
    /// written into the seed's worktree-local store.
    fn linked_resumption_pair(
        events: &[ShoreEvent],
    ) -> (
        TestRepo,
        tempfile::TempDir,
        std::path::PathBuf,
        std::path::PathBuf,
    ) {
        let main = TestRepo::new();
        main.write("README.md", "base\n");
        main.commit_all("base");

        let parent = tempfile::tempdir().unwrap();
        let seed = parent.path().join("seed");
        let reader = parent.path().join("reader");
        main.git(["worktree", "add", "-b", "seed", seed.to_str().unwrap()]);
        main.git(["worktree", "add", "-b", "reader", reader.to_str().unwrap()]);

        let seed_store = EventStore::open(seed.join(".shore"));
        for event in events {
            seed_store.record_event_once(event).unwrap();
        }
        (main, parent, seed, reader)
    }

    /// Events as a linked checkout's reads see them: through the read seam,
    /// asserting the resolution actually went clone-local.
    fn linked_store_events(repo: &Path) -> Vec<ShoreEvent> {
        use crate::session::store::resolution::{StoreResolutionMode, resolve_read_store};
        let read_store = resolve_read_store(repo).unwrap();
        assert_eq!(read_store.resolution.mode, StoreResolutionMode::CloneLocal);
        EventStore::open(read_store.store_dir())
            .list_events()
            .unwrap()
    }

    fn link(repo: &Path) {
        use crate::session::{StoreLinkOptions, link_clone_local_store};
        link_clone_local_store(StoreLinkOptions::new(repo)).unwrap();
    }

    #[test]
    fn linked_read_unsigned_response_is_non_binding_ingested_unsigned() {
        let (events, task_attempt_id) = task_resumption_events();
        let (_main, _parent, seed, reader) = linked_resumption_pair(&events);

        // Baseline: before linking, possession binds in the seed's own store.
        let local = EventStore::open(seed.join(".shore")).list_events().unwrap();
        assert!(local.iter().all(|event| event.ingest.is_none()));
        let baseline = resumption_projection(
            &local,
            &task_attempt_id,
            &TrustSet::default(),
            ResumptionBindingPolicy::default(),
        );
        assert!(baseline.may_resume);

        link(&seed);
        link(&reader);

        let stored = linked_store_events(&reader);
        assert!(stored.iter().all(|event| event.ingest.is_some()));
        let projection = resumption_projection(
            &stored,
            &task_attempt_id,
            &TrustSet::default(),
            ResumptionBindingPolicy::default(),
        );
        assert!(!projection.may_resume);
        assert_eq!(projection.state, AgentResumptionState::Blocked);
        assert_eq!(
            identity_reason(&projection).as_deref(),
            Some("ingested_unsigned")
        );
    }

    #[test]
    fn linked_read_signed_authorized_response_binds_via_verified_signer() {
        let (mut events, task_attempt_id) = task_resumption_events();
        let signer = DeterministicSigner::fixture();
        crate::session::sign_event_if_requested(
            events.last_mut().expect("response event"),
            &crate::session::EventSigningOptions::sign_with(signer.clone()),
        )
        .unwrap();
        let trust = trust_for_actor(&ActorId::new("actor:claude_code:user"), &signer);
        let (_main, _parent, seed, reader) = linked_resumption_pair(&events);
        link(&seed);
        link(&reader);

        let stored = linked_store_events(&reader);
        let projection = resumption_projection(
            &stored,
            &task_attempt_id,
            &trust,
            ResumptionBindingPolicy::default(),
        );

        // Arm (b) verified-signer binds identically from any store.
        assert!(projection.may_resume);
        assert_eq!(projection.state, AgentResumptionState::Ready);
    }

    #[test]
    fn authoring_worktree_linked_reads_see_their_own_unsigned_response_as_ingested() {
        let (events, task_attempt_id) = task_resumption_events();
        let (_main, _parent, seed, _reader) = linked_resumption_pair(&events);

        let local = EventStore::open(seed.join(".shore")).list_events().unwrap();
        let baseline = resumption_projection(
            &local,
            &task_attempt_id,
            &TrustSet::default(),
            ResumptionBindingPolicy::default(),
        );
        assert!(baseline.may_resume);

        link(&seed);

        // The sharp edge, pinned deliberately: once the author's checkout is
        // linked, its reads resolve the linked store, whose copy is stamped —
        // so even the author projects ingested_unsigned for its own unsigned
        // response. The unstamped original still sits in .shore/, but reads
        // are store-only. Sign responses that must stay binding after linking.
        let stored = linked_store_events(&seed);
        assert!(stored.iter().all(|event| event.ingest.is_some()));
        let projection = resumption_projection(
            &stored,
            &task_attempt_id,
            &TrustSet::default(),
            ResumptionBindingPolicy::default(),
        );
        assert!(!projection.may_resume);
        assert_eq!(
            identity_reason(&projection).as_deref(),
            Some("ingested_unsigned")
        );
    }

    // -- write-side companion: cross-worktree-AUTHORED response ----------------
    //
    // The cross-worktree READ of a response is covered elsewhere; this covers a
    // response *authored* in a sibling worktree against a linked-only request.
    // The resumption-binding predicate is store-agnostic, so a response authored
    // in the reader's worktree-local store and copied by `store link` is
    // bundle-stamped exactly like any ingested event: it stops binding by
    // possession and becomes ingested_unsigned unless signed.
    //
    // The task attempt and its input request live in the linked store (seed
    // authored, linked); the reader — already linked — authors only the response
    // locally.

    fn cross_worktree_response_pair(
        origin_events: &[ShoreEvent],
        response_event: &ShoreEvent,
    ) -> (
        TestRepo,
        tempfile::TempDir,
        std::path::PathBuf,
        std::path::PathBuf,
    ) {
        let main = TestRepo::new();
        main.write("README.md", "base\n");
        main.commit_all("base");

        let parent = tempfile::tempdir().unwrap();
        let seed = parent.path().join("seed");
        let reader = parent.path().join("reader");
        main.git(["worktree", "add", "-b", "seed", seed.to_str().unwrap()]);
        main.git(["worktree", "add", "-b", "reader", reader.to_str().unwrap()]);

        // The seed authors the task attempt + input request and links them into
        // the clone-local store.
        let seed_store = EventStore::open(seed.join(".shore"));
        for event in origin_events {
            seed_store.record_event_once(event).unwrap();
        }
        link(&seed);
        // The reader registers (so its write validation can see the linked-only
        // request), then authors the response in its own worktree-local store —
        // unsynced until it links again.
        link(&reader);
        EventStore::open(reader.join(".shore"))
            .record_event_once(response_event)
            .unwrap();
        (main, parent, seed, reader)
    }

    #[test]
    fn cross_worktree_unsigned_response_is_non_binding_ingested_unsigned_after_link() {
        let (events, task_attempt_id) = task_resumption_events();
        let response = events.last().expect("response event").clone();
        let origin = &events[..events.len() - 1];
        let (_main, _parent, _seed, reader) = cross_worktree_response_pair(origin, &response);

        // The reader links: its unsigned response is copied into the linked store
        // and bundle-stamped.
        link(&reader);
        let stored = linked_store_events(&reader);
        assert!(stored.iter().all(|event| event.ingest.is_some()));

        let projection = resumption_projection(
            &stored,
            &task_attempt_id,
            &TrustSet::default(),
            ResumptionBindingPolicy::default(),
        );

        assert!(!projection.may_resume);
        assert_eq!(projection.state, AgentResumptionState::Blocked);
        assert_eq!(
            identity_reason(&projection).as_deref(),
            Some("ingested_unsigned")
        );
    }

    #[test]
    fn cross_worktree_signed_authorized_response_binds_after_link() {
        let (mut events, task_attempt_id) = task_resumption_events();
        let signer = DeterministicSigner::fixture();
        crate::session::sign_event_if_requested(
            events.last_mut().expect("response event"),
            &crate::session::EventSigningOptions::sign_with(signer.clone()),
        )
        .unwrap();
        let trust = trust_for_actor(&ActorId::new("actor:claude_code:user"), &signer);
        let response = events.last().expect("response event").clone();
        let origin = &events[..events.len() - 1];
        let (_main, _parent, _seed, reader) = cross_worktree_response_pair(origin, &response);

        link(&reader);
        let stored = linked_store_events(&reader);

        let projection = resumption_projection(
            &stored,
            &task_attempt_id,
            &trust,
            ResumptionBindingPolicy::default(),
        );

        // Arm (b) verified-signer binds identically from any store.
        assert!(projection.may_resume);
        assert_eq!(projection.state, AgentResumptionState::Ready);
    }

    #[test]
    fn cross_worktree_unsigned_response_binds_locally_before_link_baseline() {
        let (events, task_attempt_id) = task_resumption_events();
        let response = events.last().expect("response event").clone();
        let origin = &events[..events.len() - 1];
        let (_main, _parent, _seed, reader) = cross_worktree_response_pair(origin, &response);

        // Before the reader links the response, its writer-visible union is the
        // linked store's request plus its OWN unstamped local response.
        let mut union = linked_store_events(&reader);
        union.extend(
            EventStore::open(reader.join(".shore"))
                .list_events()
                .unwrap(),
        );
        assert!(
            union
                .iter()
                .find(|event| event.event_type == EventType::InputRequestResponded)
                .expect("the local response is in the union")
                .ingest
                .is_none(),
            "the reader's own response is unstamped before it links"
        );

        let projection = resumption_projection(
            &union,
            &task_attempt_id,
            &TrustSet::default(),
            ResumptionBindingPolicy::default(),
        );

        // The author's own unstamped response binds via arm (a) possession; the
        // flip to ingested_unsigned is a function of which store the response is
        // read from, not of this slice's write path.
        assert!(projection.may_resume);
        assert_eq!(projection.state, AgentResumptionState::Ready);
    }
}
