//! Snapshot-bound exact-Revision fact reads over the active derived generation.

use std::collections::BTreeMap;

use super::history::{
    CurrentRead, DerivedHistoryAccess, hydrate_events, projection_stamp, state_diagnostics,
};
use super::locator::LocatorRead;
use super::support::{SupportEventPlan, support_event_plan};
#[cfg(any(test, feature = "longitudinal-counting"))]
use crate::bench_support::longitudinal::{
    LongitudinalDerivedAccessPhaseV1 as Phase, enter_derived_access_phase_v1,
};
use crate::canonical_hash::sha256_bytes_hex;
use crate::model::RevisionId;
use crate::session::ProjectionDiagnostic;
use crate::session::derived_access::cursor::TruthCursor;
use crate::session::event::{
    ArtifactRemovedPayload, EventSignatureRecordedPayload, EventType, ShoreEvent,
};

#[derive(Debug)]
pub(crate) enum ExactRevisionFactReadRouteV1 {
    Off,
    Ready(ExactRevisionFactReadV1),
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExactRevisionFactReadV1 {
    pub(crate) as_of: TruthCursor,
    pub(crate) projection_stamp: String,
    pub(crate) events: Vec<ShoreEvent>,
    pub(crate) diagnostics: Vec<ProjectionDiagnostic>,
    pub(crate) event_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExactRevisionFactReadBoundary {
    Selected,
    Hydrated,
    SupportPlanned,
}

impl DerivedHistoryAccess {
    pub(crate) fn exact_revision_fact_read_v1(
        &self,
        revision_id: &RevisionId,
    ) -> Result<ExactRevisionFactReadRouteV1, String> {
        self.exact_revision_fact_read_v1_with_hook(revision_id, |_| {})
    }

    fn exact_revision_fact_read_v1_with_hook(
        &self,
        revision_id: &RevisionId,
        mut hook: impl FnMut(ExactRevisionFactReadBoundary),
    ) -> Result<ExactRevisionFactReadRouteV1, String> {
        let Some((store_identity, _)) = self.active_context() else {
            return Ok(ExactRevisionFactReadRouteV1::Off);
        };
        let current = match self.current()? {
            CurrentRead::Ready(current) => current,
            CurrentRead::Unavailable(_) => {
                return Ok(ExactRevisionFactReadRouteV1::Unavailable);
            }
        };
        let generation_id = current.generation_id().to_owned();
        let observed = current.authority_head();
        let service = current.service();
        #[cfg(any(test, feature = "longitudinal-counting"))]
        let sqlite_phase = enter_derived_access_phase_v1(Phase::FactSqliteSelection);
        let snapshot = match service
            .exact_revision_fact_read_snapshot_at(observed)
            .map_err(|error| error.to_string())?
        {
            LocatorRead::Ready(snapshot) => snapshot,
            LocatorRead::CatchUpRequired { .. } => {
                return Ok(ExactRevisionFactReadRouteV1::Unavailable);
            }
        };
        let selected_event_ids = snapshot
            .exact_revision_event_ids(revision_id, observed)
            .map_err(|error| error.to_string());
        let selection_started = selected_event_ids.is_ok();
        if selection_started {
            hook(ExactRevisionFactReadBoundary::Selected);
        }
        #[cfg(any(test, feature = "longitudinal-counting"))]
        drop(sqlite_phase);

        let prepared = selected_event_ids.and_then(|selected_event_ids| {
            #[cfg(any(test, feature = "longitudinal-counting"))]
            let selected_phase =
                enter_derived_access_phase_v1(Phase::FactSelectedCarrierHydrationValidation);
            let selected = hydrate_events(service, &selected_event_ids, observed)?;
            validate_selected_events(&selected, revision_id)?;
            hook(ExactRevisionFactReadBoundary::Hydrated);
            #[cfg(any(test, feature = "longitudinal-counting"))]
            drop(selected_phase);

            #[cfg(any(test, feature = "longitudinal-counting"))]
            let support_phase =
                enter_derived_access_phase_v1(Phase::FactSupportCarrierHydrationValidation);
            let support_plan = support_event_plan(&snapshot.connection, &selected, observed)?;
            hook(ExactRevisionFactReadBoundary::SupportPlanned);
            let support_event_ids = support_plan.all_event_ids();
            let support = hydrate_events(service, &support_event_ids, observed)?;
            validate_support_events(&support_plan, &selected, &support)?;
            #[cfg(any(test, feature = "longitudinal-counting"))]
            drop(support_phase);

            let mut events = selected;
            events.extend(support);
            normalize_events(&mut events);
            Ok(ExactRevisionFactReadV1 {
                as_of: observed,
                projection_stamp: projection_stamp(store_identity, observed)?,
                events,
                diagnostics: state_diagnostics(&snapshot.state)?,
                event_count: snapshot.state.event_count,
            })
        });
        let finished = snapshot.finish().map_err(|error| error.to_string());
        match (prepared, finished) {
            (_, Err(error)) => {
                if selection_started {
                    record_derived_selection_failed_closed_state();
                }
                Err(error)
            }
            (Err(error), Ok(())) => {
                if selection_started {
                    record_derived_selection_failed_closed_state();
                }
                Err(error)
            }
            (Ok(read), Ok(())) => {
                let final_current = match self.current()? {
                    CurrentRead::Ready(final_current) => final_current,
                    CurrentRead::Unavailable(_) => {
                        record_derived_selection_failed_closed_state();
                        return Err(
                            "derived exact Revision fact snapshot moved before response preparation"
                                .to_owned(),
                        );
                    }
                };
                let final_observed = final_current
                    .service()
                    .truth_head()
                    .map_err(|error| error.to_string())?
                    .cursor;
                if final_current.generation_id() != generation_id
                    || final_current.authority_head() != observed
                    || final_observed != observed
                {
                    record_derived_selection_failed_closed_state();
                    return Err(
                        "derived exact Revision fact snapshot moved before response preparation"
                            .to_owned(),
                    );
                }
                Ok(ExactRevisionFactReadRouteV1::Ready(read))
            }
        }
    }
}

#[cfg(any(test, feature = "longitudinal-counting"))]
fn record_derived_selection_failed_closed_state() {
    if let Some(scope) = crate::bench_support::longitudinal::LongitudinalCountingScopeV1::current()
    {
        scope.record_observed_route_state_once(
            crate::bench_support::longitudinal::InteractionObservedRouteStateV1::DerivedSelectionFailedClosed,
        );
    }
}

#[cfg(not(any(test, feature = "longitudinal-counting")))]
fn record_derived_selection_failed_closed_state() {}

fn validate_selected_events(events: &[ShoreEvent], revision_id: &RevisionId) -> Result<(), String> {
    let mut has_proposal = false;
    for event in events {
        let subject = event
            .subject_revision_id()
            .map_err(|error| error.to_string())?;
        if subject.as_ref() != Some(revision_id) {
            return Err(format!(
                "selected authoritative event {} has the wrong exact Revision binding",
                event.event_id.as_str()
            ));
        }
        has_proposal |= event.event_type == EventType::WorkObjectProposed;
    }
    if !events.is_empty() && !has_proposal {
        return Err("selected exact Revision facts have no proposal carrier".to_owned());
    }
    Ok(())
}

fn validate_support_events(
    plan: &SupportEventPlan,
    selected: &[ShoreEvent],
    support: &[ShoreEvent],
) -> Result<(), String> {
    let support_by_id = support
        .iter()
        .map(|event| (event.event_id.as_str(), event))
        .collect::<BTreeMap<_, _>>();
    let referenced_content = crate::session::workflow::selected_support_content_hashes(selected)
        .map_err(|error| error.to_string())?;
    let mut signature_targets = selected
        .iter()
        .map(|event| (event.event_id.as_str(), event))
        .collect::<BTreeMap<_, _>>();

    for event_id in &plan.removal_event_ids {
        let event = support_by_id
            .get(event_id.as_str())
            .ok_or_else(|| format!("support removal event {event_id} is absent"))?;
        if event.event_type != EventType::ArtifactRemoved {
            return Err(format!(
                "support removal event {event_id} has the wrong family"
            ));
        }
        let payload: ArtifactRemovedPayload = serde_json::from_value(event.payload.clone())
            .map_err(|error| format!("support removal event {event_id} is invalid: {error}"))?;
        if !referenced_content.contains(&payload.content_hash) {
            return Err(format!(
                "support removal event {event_id} does not match selected authoritative content"
            ));
        }
        signature_targets.insert(event.event_id.as_str(), event);
    }
    for event_id in &plan.signature_event_ids {
        let event = support_by_id
            .get(event_id.as_str())
            .ok_or_else(|| format!("detached signature event {event_id} is absent"))?;
        if event.event_type != EventType::EventSignatureRecorded {
            return Err(format!(
                "detached signature event {event_id} has the wrong family"
            ));
        }
        let payload: EventSignatureRecordedPayload = serde_json::from_value(event.payload.clone())
            .map_err(|error| format!("detached signature event {event_id} is invalid: {error}"))?;
        let target = signature_targets
            .get(payload.target_event_id.as_str())
            .ok_or_else(|| {
                format!("detached signature event {event_id} has an unselected target")
            })?;
        let target_hash = target
            .event_record_hash()
            .map_err(|error| error.to_string())?;
        if payload.target_event_record_hash != target_hash {
            return Err(format!(
                "detached signature event {event_id} has the wrong target record hash"
            ));
        }
    }
    Ok(())
}

fn normalize_events(events: &mut Vec<ShoreEvent>) {
    events.sort_by(|left, right| {
        sha256_bytes_hex(left.idempotency_key.as_bytes())
            .cmp(&sha256_bytes_hex(right.idempotency_key.as_bytes()))
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
    events.dedup_by(|left, right| left.event_id == right.event_id);
}

#[cfg(test)]
mod contract_tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use sha2::Digest;
    use tempfile::TempDir;

    use super::*;
    use crate::bench_support::longitudinal::{
        InteractionActorV1, InteractionExecutionIdentityV1,
        InteractionPerformanceExpectedContextV1, InteractionRouteV1, InteractionSetupExpectationV1,
        LongitudinalCountingScopeV1, interaction_route_state_contract_v1,
    };
    use crate::crypto::{EventSigner, TestEd25519Signer};
    use crate::model::{
        AssessmentId, ChangeIdentityDescriptorV1, EngagementId, InputRequestId,
        InputRequestResponseId, JournalId, ObjectId, ObservationId, ReviewTargetRef, TargetRef,
        TrackId, ValidationCheckId, ValidationStatus, ValidationTarget, ValidationTrigger,
    };
    use crate::session::derived_access::history::DerivedHistoryMode;
    use crate::session::derived_access::lifecycle::{DerivedAccessLifecycle, LifecycleControl};
    use crate::session::derived_access::product_contract::DerivedAccessProfile;
    use crate::session::derived_access::sqlite::ExactRevisionFactReadSnapshot;
    use crate::session::derived_access::writer::DerivedWriteCoordinator;
    use crate::session::event::{
        BodyContentType, EventSignature, EventToBeSigned, InputRequestOpenedPayload,
        InputRequestReasonCode, InputRequestRespondedPayload, InputRequestResponseOutcome,
        ReviewAssessment, ReviewAssessmentRecordedPayload, ReviewInitializedPayload,
        ReviewObservationRecordedPayload, Revision, ValidationCheckRecordedPayload,
        WorkObjectProposal, WorkObjectProposedPayload, Writer, build_change_declared,
        event_signature_pre_authentication_encoding,
    };
    use crate::session::store::backend::StoreBackend;
    use crate::session::store::capabilities::{
        CapabilityFixtureState, write_capability_fixture_for_test,
    };
    use crate::session::store::resolution::opaque_path_identity;
    use crate::session::{EventStore, EventWriteOutcome};

    const JOURNAL: &str = "journal:fact-read-contract";
    const TRACK: &str = "agent:fact-read-contract";

    struct FactFixture {
        _temp: TempDir,
        root: PathBuf,
        backend: StoreBackend,
        store: EventStore,
        access: DerivedHistoryAccess,
        revision_id: RevisionId,
        proposal: ShoreEvent,
        selected_corruption_key: String,
        support_corruption_key: String,
        selected_count: usize,
        support_count: usize,
    }

    impl FactFixture {
        fn new(unrelated_changes: usize, validation_log_count: usize) -> Self {
            let temp = TempDir::new().expect("create disposable fact root");
            let root = temp.path().to_path_buf();
            let backend = StoreBackend::Local(root.clone());
            write_capability_fixture_for_test(
                backend.journal().as_ref(),
                CapabilityFixtureState::EmptyL2,
            )
            .expect("activate disposable L2 fact root");
            let store_identity =
                opaque_path_identity("store", &root).expect("derive fact fixture identity");
            let lifecycle = DerivedAccessLifecycle::new(
                DerivedAccessProfile::SqliteWalBodylessV1,
                &root,
                store_identity.clone(),
            )
            .expect("create fact lifecycle");
            lifecycle
                .rebuild(|_| LifecycleControl::Continue)
                .expect("publish empty fact generation");
            let coordinator =
                DerivedWriteCoordinator::new(lifecycle.clone()).expect("admit fact writer");
            let store = EventStore::from_backend(&backend).with_coordinator(coordinator);
            let revision_id = RevisionId::new(format!("rev:sha256:{}", "a".repeat(64)));
            let object_hash = format!("sha256:{}", "1".repeat(64));
            let observation_hash = format!("sha256:{}", "2".repeat(64));
            let log_hashes = (0..validation_log_count.max(1))
                .map(|index| format!("sha256:{:064x}", index + 16))
                .collect::<Vec<_>>();

            let proposal = proposal_event(&revision_id, &object_hash, "proposal");
            record(&store, &proposal);
            let duplicate_assessment_id = AssessmentId::new("assess:sha256:duplicate");
            let first_assessment = assessment_event(
                &revision_id,
                duplicate_assessment_id.clone(),
                "assessment-one",
                Some("inline assessment summary"),
            );
            record(&store, &first_assessment);
            record(
                &store,
                &assessment_event(
                    &revision_id,
                    duplicate_assessment_id,
                    "assessment-two",
                    None,
                ),
            );
            let request_id = InputRequestId::new("input-request:sha256:fact");
            record(&store, &request_event(&revision_id, &request_id));
            record(&store, &response_event(&revision_id, &request_id));
            record(&store, &observation_event(&revision_id, &observation_hash));
            record(&store, &validation_event(&revision_id, log_hashes.clone()));

            let object_removal = removal_event(&object_hash, "object");
            record(&store, &object_removal);
            record(&store, &removal_event(&observation_hash, "observation"));
            record(&store, &removal_event(&log_hashes[0], "validation-log"));
            record(
                &store,
                &signature_event(&first_assessment, "selected-assessment"),
            );
            let removal_signature = signature_event(&object_removal, "object-removal");
            record(&store, &removal_signature);

            for index in 0..unrelated_changes {
                record(&store, &unrelated_change_event(index));
            }

            let access = DerivedHistoryAccess::from_mode(DerivedHistoryMode::Active {
                lifecycle: lifecycle.clone(),
                current: Mutex::new(None),
                store_identity,
                backend: backend.clone(),
            });
            Self {
                _temp: temp,
                root,
                backend,
                store,
                access,
                revision_id,
                proposal,
                selected_corruption_key: first_assessment.idempotency_key,
                support_corruption_key: removal_signature.idempotency_key,
                selected_count: 7,
                support_count: 5,
            }
        }

        fn append_unrelated_change(&self, index: usize) {
            record(&self.store, &unrelated_change_event(index));
        }

        fn carrier_path(&self, idempotency_key: &str) -> PathBuf {
            self.root.join("events").join(format!(
                "{}.json",
                sha256_bytes_hex(idempotency_key.as_bytes())
            ))
        }
    }

    fn review_target(revision_id: &RevisionId) -> ReviewTargetRef {
        ReviewTargetRef::Revision {
            revision_id: revision_id.clone(),
        }
    }

    fn event_target(revision_id: &RevisionId) -> crate::session::event::EventTarget {
        crate::session::event::EventTarget::for_revision(
            JournalId::new(JOURNAL),
            revision_id.clone(),
            Some(TrackId::new(TRACK)),
        )
        .expect("build exact Revision target")
    }

    fn proposal_event(revision_id: &RevisionId, object_hash: &str, suffix: &str) -> ShoreEvent {
        ShoreEvent::new(
            EventType::WorkObjectProposed,
            format!("fact:{suffix}"),
            event_target(revision_id),
            Writer::shore_local("fact-read-test"),
            WorkObjectProposedPayload {
                engagement_id: EngagementId::new(format!("engagement:sha256:{}", "b".repeat(64))),
                work_object: WorkObjectProposal::Revision {
                    revision: Revision {
                        id: revision_id.clone(),
                        object_id: ObjectId::new(format!("obj:sha256:{}", "c".repeat(64))),
                        git_provenance: None,
                    },
                    summary: Some("inline Revision summary".to_owned()),
                    object_artifact_content_hash: object_hash.to_owned(),
                    supersedes: Vec::new(),
                },
            },
            "2026-08-24T20:00:00Z",
        )
        .expect("build proposal")
    }

    fn assessment_event(
        revision_id: &RevisionId,
        assessment_id: AssessmentId,
        suffix: &str,
        summary: Option<&str>,
    ) -> ShoreEvent {
        ShoreEvent::new(
            EventType::ReviewAssessmentRecorded,
            ReviewAssessmentRecordedPayload::idempotency_key(
                revision_id,
                &TrackId::new(TRACK),
                suffix,
            ),
            event_target(revision_id),
            Writer::shore_local("fact-read-test"),
            ReviewAssessmentRecordedPayload {
                assessment_id,
                target: review_target(revision_id),
                assessment: ReviewAssessment::NeedsChanges,
                summary: summary.map(str::to_owned),
                summary_content_type: BodyContentType::TextMarkdown,
                summary_artifact_path: None,
                summary_byte_size: None,
                summary_content_hash: None,
                replaces_assessment_ids: Vec::new(),
                related_observation_ids: Vec::new(),
                related_input_request_ids: Vec::new(),
            },
            if suffix == "assessment-one" {
                "2026-08-24T20:00:01Z"
            } else {
                "2026-08-24T20:00:02Z"
            },
        )
        .expect("build assessment")
    }

    fn request_event(revision_id: &RevisionId, request_id: &InputRequestId) -> ShoreEvent {
        ShoreEvent::new(
            EventType::InputRequestOpened,
            InputRequestOpenedPayload::idempotency_key(
                revision_id,
                &TrackId::new(TRACK),
                request_id.as_str(),
            ),
            event_target(revision_id),
            Writer::shore_local("fact-read-test"),
            InputRequestOpenedPayload {
                input_request_id: request_id.clone(),
                target: review_target(revision_id),
                task_target: None,
                reason_code: InputRequestReasonCode::ManualDecisionRequired,
                title: "Choose".to_owned(),
                body: Some("inline request body".to_owned()),
                body_content_type: BodyContentType::TextPlain,
                body_artifact_path: None,
                body_byte_size: None,
                body_content_hash: None,
                target_fingerprint: None,
            },
            "2026-08-24T20:00:03Z",
        )
        .expect("build input request")
    }

    fn response_event(revision_id: &RevisionId, request_id: &InputRequestId) -> ShoreEvent {
        let target = ReviewTargetRef::InputRequest {
            revision_id: revision_id.clone(),
            input_request_id: request_id.clone(),
        };
        ShoreEvent::new(
            EventType::InputRequestResponded,
            InputRequestRespondedPayload::idempotency_key(request_id, "response"),
            crate::session::event::EventTarget::for_subject(
                JournalId::new(JOURNAL),
                TargetRef::Review(target),
                Some(TrackId::new(TRACK)),
            )
            .expect("build input response target"),
            Writer::shore_local("fact-read-test"),
            InputRequestRespondedPayload {
                input_request_response_id: InputRequestResponseId::new(
                    "input-response:sha256:fact",
                ),
                input_request_id: request_id.clone(),
                revision_id: Some(revision_id.clone()),
                task_target: None,
                outcome: InputRequestResponseOutcome::Approved,
                reason: Some("inline response reason".to_owned()),
                reason_content_type: BodyContentType::TextPlain,
                reason_artifact_path: None,
                reason_byte_size: None,
                reason_content_hash: None,
                target_fingerprint: None,
            },
            "2026-08-24T20:00:04Z",
        )
        .expect("build input response")
    }

    fn observation_event(revision_id: &RevisionId, content_hash: &str) -> ShoreEvent {
        ShoreEvent::new(
            EventType::ReviewObservationRecorded,
            ReviewObservationRecordedPayload::idempotency_key(
                revision_id,
                &TrackId::new(TRACK),
                "observation",
            ),
            event_target(revision_id),
            Writer::shore_local("fact-read-test"),
            ReviewObservationRecordedPayload {
                observation_id: ObservationId::new("observation:sha256:fact"),
                target: review_target(revision_id),
                title: "External observation".to_owned(),
                body: None,
                body_content_type: BodyContentType::TextMarkdown,
                body_artifact_path: Some(format!(
                    "artifacts/notes/{}.json",
                    content_hash.strip_prefix("sha256:").unwrap()
                )),
                body_byte_size: Some(42),
                body_content_hash: Some(content_hash.to_owned()),
                tags: vec!["correctness".to_owned()],
                confidence: Some("high".to_owned()),
                supersedes_observation_ids: Vec::new(),
                responds_to_observation_ids: Vec::new(),
            },
            "2026-08-24T20:00:05Z",
        )
        .expect("build observation")
    }

    fn validation_event(revision_id: &RevisionId, log_hashes: Vec<String>) -> ShoreEvent {
        ShoreEvent::new(
            EventType::ValidationCheckRecorded,
            ValidationCheckRecordedPayload::idempotency_key(
                revision_id,
                &TrackId::new(TRACK),
                "validation",
            ),
            event_target(revision_id),
            Writer::shore_local("fact-read-test"),
            ValidationCheckRecordedPayload {
                validation_check_id: ValidationCheckId::new("validation:sha256:fact"),
                target: ValidationTarget::Revision {
                    revision_id: revision_id.clone(),
                },
                check_name: "fact contract".to_owned(),
                command: Some("cargo test".to_owned()),
                status: ValidationStatus::Passed,
                exit_code: Some(0),
                trigger: ValidationTrigger::Manual,
                source_fingerprint: None,
                summary: Some("inline validation summary".to_owned()),
                summary_content_type: BodyContentType::TextPlain,
                summary_artifact_path: None,
                summary_byte_size: None,
                summary_content_hash: None,
                started_at: None,
                completed_at: None,
                log_artifact_content_hashes: log_hashes,
            },
            "2026-08-24T20:00:06Z",
        )
        .expect("build validation")
    }

    fn removal_event(content_hash: &str, suffix: &str) -> ShoreEvent {
        ShoreEvent::new(
            EventType::ArtifactRemoved,
            format!("fact:removal:{suffix}"),
            crate::session::event::EventTarget::for_journal(JournalId::new(JOURNAL)),
            Writer::shore_local("fact-read-test"),
            ArtifactRemovedPayload {
                content_hash: content_hash.to_owned(),
            },
            "2026-08-24T20:00:07Z",
        )
        .expect("build removal")
    }

    fn signature_event(target: &ShoreEvent, suffix: &str) -> ShoreEvent {
        let signer = TestEd25519Signer::from_seed([91; 32]);
        let to_be_signed = EventToBeSigned::from_event(target, signer.signer_id())
            .expect("build signature message");
        let signature = signer
            .sign_event_message(
                &event_signature_pre_authentication_encoding(&to_be_signed)
                    .expect("encode signature message"),
            )
            .expect("sign support target");
        let payload = EventSignatureRecordedPayload {
            target_event_id: target.event_id.clone(),
            target_event_record_hash: target.event_record_hash().expect("hash signature target"),
            attesting_signer: signer.signer_id().clone(),
            attestation: EventSignature::ed25519_v1(signature),
            inclusion_proof: None,
        };
        ShoreEvent::new(
            EventType::EventSignatureRecorded,
            format!("fact:signature:{suffix}"),
            crate::session::event::EventTarget::for_journal(JournalId::new(JOURNAL)),
            Writer::shore_local("fact-read-test"),
            payload,
            "2026-08-24T20:00:08Z",
        )
        .expect("build detached signature")
    }

    fn unrelated_change_event(index: usize) -> ShoreEvent {
        let mut nonce = [0_u8; 32];
        nonce[..8].copy_from_slice(&u64::try_from(index + 1).unwrap().to_be_bytes());
        let declaration = build_change_declared(
            ChangeIdentityDescriptorV1::opaque_nonce(nonce),
            sha2::Sha256::digest(format!("fact-change:{index}").as_bytes()).into(),
        )
        .expect("build unrelated Change declaration");
        ShoreEvent::new(
            EventType::ChangeDeclared,
            format!("fact:unrelated-change:{index}"),
            crate::session::event::EventTarget::for_journal(JournalId::new(JOURNAL)),
            Writer::shore_local("fact-read-test"),
            declaration,
            format!("2026-08-24T20:01:{:02}Z", index % 60),
        )
        .expect("build unrelated Change event")
    }

    fn unrelated_authoritative_event(suffix: &str) -> ShoreEvent {
        let journal = JournalId::new(format!("journal:fact-read:{suffix}"));
        ShoreEvent::new(
            EventType::ReviewInitialized,
            ReviewInitializedPayload::idempotency_key(&journal),
            crate::session::event::EventTarget::for_journal(journal),
            Writer::shore_local("fact-read-test"),
            ReviewInitializedPayload {},
            "2026-08-24T20:02:00Z",
        )
        .expect("build unrelated authoritative event")
    }

    fn record(store: &EventStore, event: &ShoreEvent) {
        assert_eq!(
            store
                .record_event_once(event)
                .expect("record fixture event"),
            EventWriteOutcome::Created
        );
    }

    fn corrupt(path: &Path) {
        std::fs::write(path, b"{\"not\":\"a valid Shore event\"}")
            .expect("corrupt disposable carrier");
    }

    #[test]
    fn exact_revision_fact_read_api_matches_the_reviewed_contract() {
        fn snapshot_shape(snapshot: ExactRevisionFactReadSnapshot) {
            let ExactRevisionFactReadSnapshot { connection, state } = snapshot;
            drop((connection, state));
        }

        fn route_shape(route: ExactRevisionFactReadRouteV1) {
            match route {
                ExactRevisionFactReadRouteV1::Off | ExactRevisionFactReadRouteV1::Unavailable => {}
                ExactRevisionFactReadRouteV1::Ready(ExactRevisionFactReadV1 { .. }) => {}
            }
        }

        let method = DerivedHistoryAccess::exact_revision_fact_read_v1;
        let snapshot_method = ExactRevisionFactReadSnapshot::exact_revision_event_ids;
        let finish = ExactRevisionFactReadSnapshot::finish;
        let _ = (
            snapshot_shape,
            route_shape,
            method,
            snapshot_method,
            finish,
            RevisionId::new("rev:sha256:contract"),
        );
    }

    #[test]
    fn rich_exact_revision_fact_read_is_indexed_snapshot_bound_and_fact_only() {
        let fixture = FactFixture::new(64, 2);
        let authoritative = fixture.store.list_events().expect("read fixture authority");
        let expected_state =
            crate::session::derived_access::semantic::state::SemanticStateSnapshot::from_events(
                &authoritative,
            )
            .expect("project authoritative state");
        fixture
            .access
            .current()
            .expect("warm current generation before counting");

        let scope = LongitudinalCountingScopeV1::new("a".repeat(64)).unwrap();
        let guard = scope.enter();
        let route = fixture
            .access
            .exact_revision_fact_read_v1(&fixture.revision_id)
            .expect("read exact Revision facts");
        drop(guard);
        let ExactRevisionFactReadRouteV1::Ready(read) = route else {
            panic!("current fact fixture must be Ready");
        };

        assert_eq!(read.as_of.sequence, authoritative.len() as u64);
        assert_eq!(read.event_count, authoritative.len());
        assert!(!read.projection_stamp.is_empty());
        assert_eq!(
            read.diagnostics,
            state_diagnostics(&expected_state).unwrap()
        );
        assert_eq!(
            read.events
                .iter()
                .filter(|event| event.event_type == EventType::WorkObjectProposed)
                .count(),
            1
        );
        assert_eq!(
            read.events
                .iter()
                .filter(|event| event.event_type == EventType::ReviewAssessmentRecorded)
                .count(),
            2
        );
        assert_eq!(
            read.events
                .iter()
                .filter(|event| event.event_type == EventType::ArtifactRemoved)
                .count(),
            3
        );
        assert_eq!(
            read.events
                .iter()
                .filter(|event| event.event_type == EventType::EventSignatureRecorded)
                .count(),
            2
        );
        assert_eq!(
            read.events.len(),
            fixture.selected_count + fixture.support_count
        );
        let mut normalized = read.events.clone();
        normalized.reverse();
        normalized.push(read.events[0].clone());
        normalize_events(&mut normalized);
        assert_eq!(normalized, read.events);

        let counters = &scope.snapshot().counters;
        assert_eq!(counters.strict_journal_inspections, 0);
        assert_eq!(
            counters.fact_sqlite_rows_selected,
            fixture.selected_count as u64
        );
        assert_eq!(counters.change_semantic_constructions, 0);
        assert_eq!(counters.change_projection_constructions, 0);
    }

    #[test]
    fn exact_revision_query_uses_the_existing_revision_index() {
        fn query_shape(fixture: &FactFixture) -> (Vec<String>, u64, usize) {
            let CurrentRead::Ready(current) = fixture.access.current().unwrap() else {
                panic!("fixture generation must be current");
            };
            let observed = current.authority_head();
            let LocatorRead::Ready(snapshot) = current
                .service()
                .exact_revision_fact_read_snapshot_at(observed)
                .unwrap()
            else {
                panic!("fixture snapshot must be current");
            };
            let details = snapshot
                .exact_revision_event_query_plan(&fixture.revision_id, observed)
                .expect("read exact Revision query plan");
            let vm_steps = snapshot
                .exact_revision_event_vm_steps(&fixture.revision_id, observed)
                .expect("measure exact Revision query work");
            let selected = snapshot
                .exact_revision_event_ids(&fixture.revision_id, observed)
                .unwrap()
                .len();
            snapshot.finish().unwrap();
            (details, vm_steps, selected)
        }

        let small = FactFixture::new(0, 1);
        let large = FactFixture::new(64, 1);
        let (_, small_steps, small_selected) = query_shape(&small);
        let (details, large_steps, large_selected) = query_shape(&large);
        assert!(
            details
                .iter()
                .any(|detail| detail.contains("semantic_event_fact_revision")),
            "exact Revision selection must use its physical index: {details:?}"
        );
        assert_eq!(small_selected, small.selected_count);
        assert_eq!(large_selected, large.selected_count);
        assert!(
            large_steps <= small_steps.saturating_add(24),
            "exact Revision work grew with unrelated Change history: small={small_steps}, large={large_steps}"
        );
    }

    #[test]
    fn exact_revision_miss_off_and_preselection_unavailable_are_typed() {
        let off = DerivedHistoryAccess::from_mode(DerivedHistoryMode::Off);
        assert!(matches!(
            off.exact_revision_fact_read_v1(&RevisionId::new("rev:sha256:missing"))
                .unwrap(),
            ExactRevisionFactReadRouteV1::Off
        ));

        let fixture = FactFixture::new(0, 1);
        let missing = RevisionId::new(format!("rev:sha256:{}", "f".repeat(64)));
        let ExactRevisionFactReadRouteV1::Ready(read) = fixture
            .access
            .exact_revision_fact_read_v1(&missing)
            .expect("read exact miss")
        else {
            panic!("current exact miss is final");
        };
        assert!(read.events.is_empty());
        assert_eq!(read.event_count, fixture.store.list_events().unwrap().len());

        let temp = TempDir::new().unwrap();
        let backend = StoreBackend::Local(temp.path().to_path_buf());
        write_capability_fixture_for_test(
            backend.journal().as_ref(),
            CapabilityFixtureState::EmptyL2,
        )
        .unwrap();
        let identity = opaque_path_identity("store", temp.path()).unwrap();
        let lifecycle = DerivedAccessLifecycle::new(
            DerivedAccessProfile::SqliteWalBodylessV1,
            temp.path(),
            identity.clone(),
        )
        .unwrap();
        let unavailable = DerivedHistoryAccess::from_mode(DerivedHistoryMode::Active {
            lifecycle,
            current: Mutex::new(None),
            store_identity: identity,
            backend,
        });
        assert!(matches!(
            unavailable.exact_revision_fact_read_v1(&missing).unwrap(),
            ExactRevisionFactReadRouteV1::Unavailable
        ));
    }

    #[test]
    fn current_to_catching_up_before_selection_is_unavailable() {
        let fixture = FactFixture::new(0, 1);
        fixture.access.current().expect("warm current generation");
        let out_of_band = unrelated_authoritative_event("catching-up");
        record(&EventStore::from_backend(&fixture.backend), &out_of_band);
        assert!(matches!(
            fixture
                .access
                .exact_revision_fact_read_v1(&fixture.revision_id)
                .unwrap(),
            ExactRevisionFactReadRouteV1::Unavailable
        ));
    }

    #[test]
    fn selected_or_support_corruption_after_selection_is_terminal() {
        let selected = FactFixture::new(0, 1);
        let selected_path = selected.carrier_path(&selected.selected_corruption_key);
        let selected_error = selected
            .access
            .exact_revision_fact_read_v1_with_hook(&selected.revision_id, |boundary| {
                if boundary == ExactRevisionFactReadBoundary::Selected {
                    std::fs::remove_file(&selected_path)
                        .expect("remove disposable selected carrier");
                }
            })
            .expect_err("selected corruption must fail closed");
        assert!(!selected_error.is_empty());

        let support = FactFixture::new(0, 1);
        let support_path = support.carrier_path(&support.support_corruption_key);
        let support_error = support
            .access
            .exact_revision_fact_read_v1_with_hook(&support.revision_id, |boundary| {
                if boundary == ExactRevisionFactReadBoundary::SupportPlanned {
                    corrupt(&support_path);
                }
            })
            .expect_err("support corruption must fail closed");
        assert!(!support_error.is_empty());
    }

    #[test]
    fn postselection_authority_movement_is_terminal_not_fallback() {
        let fixture = FactFixture::new(0, 1);
        let mut appended = false;
        let error = fixture
            .access
            .exact_revision_fact_read_v1_with_hook(&fixture.revision_id, |boundary| {
                if boundary == ExactRevisionFactReadBoundary::Selected && !appended {
                    fixture.append_unrelated_change(10_000);
                    appended = true;
                }
            })
            .expect_err("post-selection movement must fail closed");
        assert!(appended);
        assert!(
            error.contains("moved"),
            "unexpected terminal error: {error}"
        );
    }

    #[test]
    fn postselection_authority_movement_publishes_truthful_terminal_receipt() {
        let fixture = FactFixture::new(0, 1);
        let scope = LongitudinalCountingScopeV1::new("f".repeat(64)).unwrap();
        scope.record_observed_route_once(InteractionRouteV1::AssessmentCurrentResult);
        scope.record_execution_actor_once(InteractionActorV1::RequestReader);
        let mut guard = Some(scope.enter());

        let mut appended = false;
        let result = fixture.access.exact_revision_fact_read_v1_with_hook(
            &fixture.revision_id,
            |boundary| {
                if boundary == ExactRevisionFactReadBoundary::Selected && !appended {
                    drop(guard.take());
                    let out_of_band =
                        unrelated_authoritative_event("postselection-counted-movement");
                    record(&EventStore::from_backend(&fixture.backend), &out_of_band);
                    guard = Some(scope.enter());
                    appended = true;
                }
            },
        );
        fixture
            .access
            .cancel_background_rebuild()
            .expect("join post-selection maintenance child");
        scope.record_semantic_result_sha256_once("9".repeat(64));
        scope.record_outcome_once(false, 1);
        drop(guard.take());

        assert!(appended);
        assert!(result.unwrap_err().contains("moved"));
        let contract = interaction_route_state_contract_v1(
            InteractionRouteV1::AssessmentCurrentResult,
            InteractionSetupExpectationV1::FactPostSelectionFailure,
        )
        .expect("post-selection terminal contract");
        let expected_child_actors = if contract.background_maintenance_children > 0 {
            BTreeMap::from([(
                InteractionActorV1::BackgroundMaintenance,
                u16::from(contract.background_maintenance_children),
            )])
        } else {
            BTreeMap::new()
        };
        let receipt = scope
            .interaction_receipt(InteractionPerformanceExpectedContextV1 {
                execution: InteractionExecutionIdentityV1 {
                    source_commit: "a".repeat(40),
                    source_tree: "b".repeat(40),
                    cargo_lock_sha256: "c".repeat(64),
                    binary_path: std::env::current_exe()
                        .expect("current fact-read test binary")
                        .display()
                        .to_string(),
                    binary_sha256: "d".repeat(64),
                    build_profile: "test".to_owned(),
                    rustc_version: "rustc test".to_owned(),
                    features: vec!["gix".to_owned(), "longitudinal-counting".to_owned()],
                },
                route: InteractionRouteV1::AssessmentCurrentResult,
                arguments: vec!["assessment".to_owned(), "show".to_owned()],
                setup_expectation: InteractionSetupExpectationV1::FactPostSelectionFailure,
                fixture_identity_sha256: Some("e".repeat(64)),
                revision: Some(fixture.revision_id.as_str().to_owned()),
                track: Some(TRACK.to_owned()),
                domain_actor: Some("actor:agent:fact-read-test".to_owned()),
                expected_child_actors,
            })
            .expect("truthful post-selection movement receipt");
        receipt.validate().expect("terminal receipt validates");
        assert_eq!(receipt.children.len(), 1);
        assert_eq!(
            receipt.children[0].actor,
            InteractionActorV1::BackgroundMaintenance
        );
        assert_eq!(receipt.counters.authoritative_fallbacks, 0);
        assert_eq!(receipt.counters.full_history_fallbacks, 0);
    }

    #[test]
    fn fact_snapshot_rejects_k_state_with_a_k_plus_one_checkpoint() {
        let fixture = FactFixture::new(0, 1);
        let CurrentRead::Ready(current) = fixture.access.current().unwrap() else {
            panic!("fixture generation must be current");
        };
        let observed = current.authority_head();
        let LocatorRead::Ready(snapshot) = current
            .service()
            .exact_revision_fact_read_snapshot_at(observed)
            .unwrap()
        else {
            panic!("fixture snapshot must be current");
        };
        assert_eq!(snapshot.state.event_count as u64, observed.sequence);
        assert_eq!(
            snapshot
                .exact_revision_event_ids(&fixture.revision_id, observed)
                .unwrap()
                .len(),
            fixture.selected_count
        );
        fixture.append_unrelated_change(20_000);
        assert_eq!(snapshot.state.event_count as u64, observed.sequence);
        snapshot.finish().unwrap();
        assert!(matches!(
            current
                .service()
                .exact_revision_fact_read_snapshot_at(observed)
                .unwrap(),
            LocatorRead::CatchUpRequired { .. }
        ));
    }

    #[test]
    fn explicit_snapshot_close_failure_is_observable() {
        let fixture = FactFixture::new(0, 1);
        let CurrentRead::Ready(current) = fixture.access.current().unwrap() else {
            panic!("fixture generation must be current");
        };
        let observed = current.authority_head();
        let LocatorRead::Ready(snapshot) = current
            .service()
            .exact_revision_fact_read_snapshot_at(observed)
            .unwrap()
        else {
            panic!("fixture snapshot must be current");
        };
        snapshot.connection.execute_batch("ROLLBACK").unwrap();
        assert!(snapshot.finish().is_err());
    }

    #[test]
    fn support_planning_is_not_limited_by_sqlite_bind_count() {
        let fixture = FactFixture::new(0, 1_005);
        let ExactRevisionFactReadRouteV1::Ready(read) = fixture
            .access
            .exact_revision_fact_read_v1(&fixture.revision_id)
            .expect("read fact with retained-scale support references")
        else {
            panic!("bind-scale fact fixture must be Ready");
        };
        assert_eq!(
            read.events.len(),
            fixture.selected_count + fixture.support_count
        );
    }

    #[test]
    fn selected_binding_and_scoped_source_fences_fail_closed() {
        let fixture = FactFixture::new(0, 1);
        let other_revision = RevisionId::new(format!("rev:sha256:{}", "e".repeat(64)));
        let wrong = proposal_event(
            &other_revision,
            &format!("sha256:{}", "9".repeat(64)),
            "wrong-revision",
        );
        assert!(validate_selected_events(&[wrong], &fixture.revision_id).is_err());
        let wrong_support_plan = SupportEventPlan {
            removal_event_ids: vec![fixture.proposal.event_id.as_str().to_owned()],
            signature_event_ids: Vec::new(),
        };
        assert!(
            validate_support_events(
                &wrong_support_plan,
                std::slice::from_ref(&fixture.proposal),
                std::slice::from_ref(&fixture.proposal),
            )
            .is_err()
        );

        let fact_source = include_str!("fact_reads.rs");
        let semantic_source = include_str!("sqlite/semantic.rs");
        let constructor_start = semantic_source
            .find("pub(crate) fn exact_revision_fact_read_snapshot(")
            .expect("fact snapshot constructor marker");
        let constructor_tail = &semantic_source[constructor_start..];
        let constructor_end = constructor_tail
            .find("/// Open the exact Timeline snapshot")
            .expect("fact snapshot constructor terminator");
        let constructor = &constructor_tail[..constructor_end];
        let forbidden = [
            ["ProductHistory", "ReadSnapshot"].concat(),
            ["product_history", "_read_snapshot"].concat(),
            ["product_history", "_connection"].concat(),
            ["facts_for_revision", "_hydrated"].concat(),
            ["query_materialized_change", "_projection"].concat(),
            ["query_materialized_change", "_projections"].concat(),
            ["query_materialized_change_document", "_facts"].concat(),
            ["project_changes", "_from_facts"].concat(),
            ["project_change_documents", "_from_facts"].concat(),
        ];
        for token in forbidden {
            assert!(
                !fact_source.contains(&token),
                "fact producer must not contain {token}"
            );
            assert!(
                !constructor.contains(&token),
                "fact snapshot constructor must not contain {token}"
            );
        }
    }
}
