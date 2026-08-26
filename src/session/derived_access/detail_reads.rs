//! Snapshot-bound exact-Revision component detail reads over the active
//! derived generation.
//!
//! The producer follows the fact-read discipline: prove currentness, pin one
//! exact truth cursor, open the fact-only snapshot with checkpoint equality,
//! select the supersession-component closure and the store-wide removal-audit
//! closure on that one open transaction, hydrate and validate every selected,
//! support, and audit carrier through the authoritative store, take the
//! materialized global state, finish explicitly, and re-prove stability before
//! returning `Ready`. Failures after selection begins fail closed and never
//! fall back to a full replay.

use std::collections::{BTreeMap, BTreeSet};

use super::fact_reads::{
    normalize_events, record_derived_selection_failed_closed_state, validate_support_events,
};
use super::history::{
    CurrentRead, DerivedHistoryAccess, hydrate_events, projection_stamp, state_diagnostics,
};
use super::locator::LocatorRead;
use super::support::support_event_plan;
#[cfg(any(test, feature = "longitudinal-counting"))]
use crate::bench_support::longitudinal::{
    LongitudinalDerivedAccessPhaseV1 as Phase, enter_derived_access_phase_v1,
};
use crate::model::RevisionId;
use crate::session::ProjectionDiagnostic;
use crate::session::derived_access::cursor::TruthCursor;
use crate::session::event::{
    ArtifactRemovedPayload, EventSignatureRecordedPayload, EventType, ShoreEvent,
};
use crate::session::workflow::referenced_content_hashes_for_event;

#[derive(Debug)]
pub(crate) enum ExactRevisionDetailReadRouteV1 {
    Off,
    Ready(Box<ExactRevisionDetailReadV1>),
    Unavailable,
}

/// One prepared derived detail read. `events` is the normalized selected
/// component closure plus its support closure; `audit_events` is the hydrated
/// D22-A removal-audit closure, kept separate so it feeds only the two
/// removal-audit diagnostic folds and can never perturb component-scoped
/// output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExactRevisionDetailReadV1 {
    pub(crate) as_of: TruthCursor,
    pub(crate) projection_stamp: String,
    pub(crate) events: Vec<ShoreEvent>,
    pub(crate) audit_events: Vec<ShoreEvent>,
    pub(crate) diagnostics: Vec<ProjectionDiagnostic>,
    pub(crate) event_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::session) enum ExactRevisionDetailReadBoundary {
    Selected,
    Hydrated,
    SupportPlanned,
    AuditHydrated,
}

impl DerivedHistoryAccess {
    pub(crate) fn exact_revision_detail_read_v1(
        &self,
        revision_id: &RevisionId,
    ) -> Result<ExactRevisionDetailReadRouteV1, String> {
        self.exact_revision_detail_read_v1_inner(revision_id, |_| {})
    }

    #[cfg(test)]
    pub(in crate::session) fn exact_revision_detail_read_v1_with_hook(
        &self,
        revision_id: &RevisionId,
        hook: impl FnMut(ExactRevisionDetailReadBoundary),
    ) -> Result<ExactRevisionDetailReadRouteV1, String> {
        self.exact_revision_detail_read_v1_inner(revision_id, hook)
    }

    fn exact_revision_detail_read_v1_inner(
        &self,
        revision_id: &RevisionId,
        mut hook: impl FnMut(ExactRevisionDetailReadBoundary),
    ) -> Result<ExactRevisionDetailReadRouteV1, String> {
        let Some((store_identity, _)) = self.active_context() else {
            return Ok(ExactRevisionDetailReadRouteV1::Off);
        };
        let current = match self.current()? {
            CurrentRead::Ready(current) => current,
            CurrentRead::Unavailable(_) => {
                return Ok(ExactRevisionDetailReadRouteV1::Unavailable);
            }
        };
        let generation_id = current.generation_id().to_owned();
        let observed = current.authority_head();
        let service = current.service();
        #[cfg(any(test, feature = "longitudinal-counting"))]
        let sqlite_phase = enter_derived_access_phase_v1(Phase::RevisionDetailSqlSelection);
        let snapshot = match service
            .exact_revision_fact_read_snapshot_at(observed)
            .map_err(|error| error.to_string())?
        {
            LocatorRead::Ready(snapshot) => snapshot,
            LocatorRead::CatchUpRequired { .. } => {
                return Ok(ExactRevisionDetailReadRouteV1::Unavailable);
            }
        };
        let selected_event_ids = snapshot
            .revision_component_event_ids(revision_id, observed)
            .map_err(|error| error.to_string());
        // The audit SQL selection is attributed inside the same SQL-selection
        // phase; it runs only once component selection has begun.
        let audit_event_ids = match &selected_event_ids {
            Ok(_) => snapshot
                .store_removal_audit_event_ids(observed)
                .map_err(|error| error.to_string()),
            Err(error) => Err(error.clone()),
        };
        let selection_started = selected_event_ids.is_ok();
        if selection_started {
            hook(ExactRevisionDetailReadBoundary::Selected);
        }
        #[cfg(any(test, feature = "longitudinal-counting"))]
        drop(sqlite_phase);

        let prepared = selected_event_ids
            .and_then(|selected| audit_event_ids.map(|audit| (selected, audit)))
            .and_then(|(selected_event_ids, audit_event_ids)| {
                #[cfg(any(test, feature = "longitudinal-counting"))]
                let selected_phase = enter_derived_access_phase_v1(
                    Phase::RevisionDetailSelectedCarrierHydrationValidation,
                );
                let selected = hydrate_events(service, &selected_event_ids, observed)?;
                validate_selected_component_events(&selected, revision_id)?;
                hook(ExactRevisionDetailReadBoundary::Hydrated);
                #[cfg(any(test, feature = "longitudinal-counting"))]
                drop(selected_phase);

                #[cfg(any(test, feature = "longitudinal-counting"))]
                let support_phase = enter_derived_access_phase_v1(
                    Phase::RevisionDetailSupportCarrierHydrationValidation,
                );
                let support_plan = support_event_plan(&snapshot.connection, &selected, observed)?;
                hook(ExactRevisionDetailReadBoundary::SupportPlanned);
                let support_event_ids = support_plan.all_event_ids();
                let support = hydrate_events(service, &support_event_ids, observed)?;
                validate_support_events(&support_plan, &selected, &support)?;
                #[cfg(any(test, feature = "longitudinal-counting"))]
                drop(support_phase);

                #[cfg(any(test, feature = "longitudinal-counting"))]
                let audit_phase = enter_derived_access_phase_v1(
                    Phase::RevisionDetailAuditCarrierHydrationValidation,
                );
                let mut audit_events = hydrate_events(service, &audit_event_ids, observed)?;
                validate_audit_events(&audit_events)?;
                hook(ExactRevisionDetailReadBoundary::AuditHydrated);
                #[cfg(any(test, feature = "longitudinal-counting"))]
                drop(audit_phase);

                let mut events = selected;
                events.extend(support);
                normalize_events(&mut events);
                normalize_events(&mut audit_events);
                Ok(ExactRevisionDetailReadV1 {
                    as_of: observed,
                    projection_stamp: projection_stamp(store_identity, observed)?,
                    events,
                    audit_events,
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
                            "derived Revision detail snapshot moved before response preparation"
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
                        "derived Revision detail snapshot moved before response preparation"
                            .to_owned(),
                    );
                }
                Ok(ExactRevisionDetailReadRouteV1::Ready(Box::new(read)))
            }
        }
    }
}

/// Every selected event must carry a Revision binding, and every component
/// revision that appears must be represented by its proposal carrier — the
/// closure is complete per component member, never a partial fact slice.
fn validate_selected_component_events(
    events: &[ShoreEvent],
    revision_id: &RevisionId,
) -> Result<(), String> {
    let mut subjects = BTreeSet::new();
    let mut proposals = BTreeSet::new();
    for event in events {
        let Some(subject) = event
            .subject_revision_id()
            .map_err(|error| error.to_string())?
        else {
            return Err(format!(
                "selected component event {} has no Revision binding",
                event.event_id.as_str()
            ));
        };
        if event.event_type == EventType::WorkObjectProposed {
            proposals.insert(subject.clone());
        }
        subjects.insert(subject);
    }
    if events.is_empty() {
        return Ok(());
    }
    if !subjects.contains(revision_id) {
        return Err(
            "selected component closure does not contain the addressed Revision".to_owned(),
        );
    }
    for subject in &subjects {
        if !proposals.contains(subject) {
            return Err(format!(
                "component revision {} has no proposal carrier",
                subject.as_str()
            ));
        }
    }
    Ok(())
}

/// The removal-audit closure admits `ArtifactRemoved` carriers, detached
/// signatures whose target is one of those removals (with a matching record
/// hash), and any carrier that references a removed content hash under the
/// shared referenced-artifacts semantics — proposals binding a removed object
/// hash and body-bearing carriers whose externalized note body is the removed
/// content. Anything else fails closed.
fn validate_audit_events(events: &[ShoreEvent]) -> Result<(), String> {
    let mut removals: BTreeMap<&str, &ShoreEvent> = BTreeMap::new();
    let mut removed_hashes: BTreeSet<String> = BTreeSet::new();
    for event in events {
        if event.event_type == EventType::ArtifactRemoved {
            let payload: ArtifactRemovedPayload = serde_json::from_value(event.payload.clone())
                .map_err(|error| {
                    format!(
                        "audit removal event {} is invalid: {error}",
                        event.event_id.as_str()
                    )
                })?;
            removed_hashes.insert(payload.content_hash);
            removals.insert(event.event_id.as_str(), event);
        }
    }
    for event in events {
        match event.event_type {
            EventType::ArtifactRemoved => {}
            EventType::EventSignatureRecorded => {
                let payload: EventSignatureRecordedPayload =
                    serde_json::from_value(event.payload.clone()).map_err(|error| {
                        format!(
                            "audit signature event {} is invalid: {error}",
                            event.event_id.as_str()
                        )
                    })?;
                let target = removals
                    .get(payload.target_event_id.as_str())
                    .ok_or_else(|| {
                        format!(
                            "audit signature event {} does not target a removal carrier",
                            event.event_id.as_str()
                        )
                    })?;
                let target_hash = target
                    .event_record_hash()
                    .map_err(|error| error.to_string())?;
                if payload.target_event_record_hash != target_hash {
                    return Err(format!(
                        "audit signature event {} has the wrong target record hash",
                        event.event_id.as_str()
                    ));
                }
            }
            _ => {
                let referenced = referenced_content_hashes_for_event(event).map_err(|error| {
                    format!(
                        "audit carrier {} has unreadable content references: {error}",
                        event.event_id.as_str()
                    )
                })?;
                if !referenced.iter().any(|hash| removed_hashes.contains(hash)) {
                    return Err(format!(
                        "audit carrier {} does not reference a removed content hash",
                        event.event_id.as_str()
                    ));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod contract_tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use sha2::Digest;
    use tempfile::TempDir;

    use super::*;
    use crate::bench_support::longitudinal::LongitudinalCountingScopeV1;
    use crate::canonical_hash::sha256_bytes_hex;
    use crate::crypto::{EventSigner, TestEd25519Signer};
    use crate::model::{
        AssessmentId, ChangeIdentityDescriptorV1, EngagementId, InputRequestId,
        InputRequestResponseId, JournalId, ObjectId, ObservationId, ReviewTargetRef, TargetRef,
        TrackId, ValidationCheckId, ValidationStatus, ValidationTarget, ValidationTrigger,
    };
    use crate::session::derived_access::history::DerivedHistoryMode;
    use crate::session::derived_access::lifecycle::{DerivedAccessLifecycle, LifecycleControl};
    use crate::session::derived_access::product_contract::DerivedAccessProfile;
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

    const JOURNAL: &str = "journal:detail-read-contract";
    const TRACK: &str = "agent:detail-read-contract";

    fn revision(digit: &str) -> RevisionId {
        RevisionId::new(format!("rev:sha256:{}", digit.repeat(32)))
    }

    fn hash(digit: &str) -> String {
        format!("sha256:{}", digit.repeat(32))
    }

    struct DetailFixture {
        _temp: TempDir,
        root: PathBuf,
        backend: StoreBackend,
        store: EventStore,
        access: DerivedHistoryAccess,
        root_revision: RevisionId,
        head_revision: RevisionId,
        fork_revision: RevisionId,
        selected_count: usize,
        support_count: usize,
        expected_audit_ids: BTreeSet<String>,
        selected_corruption_key: String,
        support_corruption_key: String,
        audit_corruption_key: String,
    }

    impl DetailFixture {
        #[allow(clippy::too_many_lines)]
        fn new(unrelated_changes: usize, extra_removals: usize) -> Self {
            let temp = TempDir::new().expect("create disposable detail root");
            let root = temp.path().to_path_buf();
            let backend = StoreBackend::Local(root.clone());
            write_capability_fixture_for_test(
                backend.journal().as_ref(),
                CapabilityFixtureState::EmptyL2,
            )
            .expect("activate disposable L2 detail root");
            let store_identity =
                opaque_path_identity("store", &root).expect("derive detail fixture identity");
            let lifecycle = DerivedAccessLifecycle::new(
                DerivedAccessProfile::SqliteWalBodylessV1,
                &root,
                store_identity.clone(),
            )
            .expect("create detail lifecycle");
            let store = EventStore::from_backend(&backend);

            let root_revision = revision("a1");
            let head_revision = revision("a2");
            let fork_revision = revision("a3");
            let mut expected_audit_ids = BTreeSet::new();

            // Component: A root, B supersedes A (head), C supersedes A (fork).
            let a_proposal =
                proposal_event(&root_revision, "01", &hash("c1"), &[], "proposal-a", 0);
            record(&store, &a_proposal);
            let b_proposal = proposal_event(
                &head_revision,
                "02",
                &hash("c2"),
                &[&root_revision],
                "proposal-b",
                1,
            );
            record(&store, &b_proposal);
            expected_audit_ids.insert(b_proposal.event_id.as_str().to_owned());
            let c_proposal = proposal_event(
                &fork_revision,
                "03",
                &hash("c3"),
                &[&root_revision],
                "proposal-c",
                2,
            );
            record(&store, &c_proposal);

            // Facts on the head: duplicate assessments, an answered request,
            // an observation with an external body, a validation with a log
            // artifact reference.
            let duplicate_assessment = AssessmentId::new("assess:sha256:detail-duplicate");
            let first_assessment = assessment_event(
                &head_revision,
                duplicate_assessment.clone(),
                "assessment-one",
                3,
            );
            record(&store, &first_assessment);
            record(
                &store,
                &assessment_event(&head_revision, duplicate_assessment, "assessment-two", 4),
            );
            let request_id = InputRequestId::new("input-request:sha256:detail");
            record(&store, &request_event(&head_revision, &request_id, 5));
            record(&store, &response_event(&head_revision, &request_id, 6));
            let observation = observation_event(&head_revision, &hash("c4"), 7);
            record(&store, &observation);
            expected_audit_ids.insert(observation.event_id.as_str().to_owned());
            record(
                &store,
                &validation_event(&head_revision, vec![hash("c5")], 8),
            );

            // Support removals: the head's object artifact, the observation
            // body, and the validation log.
            let object_removal = removal_event(&hash("c2"), "removal-object", 9);
            record(&store, &object_removal);
            expected_audit_ids.insert(object_removal.event_id.as_str().to_owned());
            let observation_removal = removal_event(&hash("c4"), "removal-observation", 10);
            record(&store, &observation_removal);
            expected_audit_ids.insert(observation_removal.event_id.as_str().to_owned());
            let log_removal = removal_event(&hash("c5"), "removal-log", 11);
            record(&store, &log_removal);
            expected_audit_ids.insert(log_removal.event_id.as_str().to_owned());

            // Support signatures: one over a selected carrier, one over the
            // object removal.
            record(
                &store,
                &signature_event(&first_assessment, "signature-assessment", 12),
            );
            let removal_signature = signature_event(&object_removal, "signature-removal", 13);
            record(&store, &removal_signature);
            expected_audit_ids.insert(removal_signature.event_id.as_str().to_owned());

            // Audit-only carriers outside the component: a removed hash bound
            // by two proposals (reuse), a never-referenced removal, duplicate
            // removal carriers over one hash with a binding proposal, and a
            // detached signature over the reuse removal.
            let x_proposal =
                proposal_event(&revision("b1"), "04", &hash("c6"), &[], "proposal-x", 14);
            record(&store, &x_proposal);
            expected_audit_ids.insert(x_proposal.event_id.as_str().to_owned());
            let reuse_removal = removal_event(&hash("c6"), "removal-reuse", 15);
            record(&store, &reuse_removal);
            expected_audit_ids.insert(reuse_removal.event_id.as_str().to_owned());
            let y_proposal =
                proposal_event(&revision("b2"), "04", &hash("c6"), &[], "proposal-y", 16);
            record(&store, &y_proposal);
            expected_audit_ids.insert(y_proposal.event_id.as_str().to_owned());
            let reuse_signature = signature_event(&reuse_removal, "signature-reuse", 17);
            record(&store, &reuse_signature);
            expected_audit_ids.insert(reuse_signature.event_id.as_str().to_owned());

            let ghost_removal = removal_event(&hash("c7"), "removal-ghost", 18);
            record(&store, &ghost_removal);
            expected_audit_ids.insert(ghost_removal.event_id.as_str().to_owned());

            let dup_removal_one = removal_event(&hash("c8"), "removal-dup-one", 19);
            record(&store, &dup_removal_one);
            expected_audit_ids.insert(dup_removal_one.event_id.as_str().to_owned());
            let dup_removal_two = removal_event(&hash("c8"), "removal-dup-two", 20);
            record(&store, &dup_removal_two);
            expected_audit_ids.insert(dup_removal_two.event_id.as_str().to_owned());
            let z_proposal =
                proposal_event(&revision("b3"), "05", &hash("c8"), &[], "proposal-z", 21);
            record(&store, &z_proposal);
            expected_audit_ids.insert(z_proposal.event_id.as_str().to_owned());

            for index in 0..extra_removals {
                let extra = removal_event(
                    &format!("sha256:{:064x}", index + 4096),
                    &format!("removal-extra-{index}"),
                    22,
                );
                record(&store, &extra);
                expected_audit_ids.insert(extra.event_id.as_str().to_owned());
            }
            for index in 0..unrelated_changes {
                record(&store, &unrelated_change_event(index));
            }

            lifecycle
                .rebuild(|_| LifecycleControl::Continue)
                .expect("publish current detail generation");
            let access = DerivedHistoryAccess::from_mode(DerivedHistoryMode::Active {
                lifecycle,
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
                root_revision,
                head_revision,
                fork_revision,
                selected_count: 9,
                support_count: 5,
                expected_audit_ids,
                selected_corruption_key: first_assessment.idempotency_key,
                support_corruption_key: removal_signature.idempotency_key,
                audit_corruption_key: ghost_removal.idempotency_key,
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

    fn event_target(revision_id: &RevisionId) -> crate::session::event::EventTarget {
        crate::session::event::EventTarget::for_revision(
            JournalId::new(JOURNAL),
            revision_id.clone(),
            Some(TrackId::new(TRACK)),
        )
        .expect("build detail target")
    }

    fn occurred(offset: usize) -> String {
        format!("2026-08-25T20:00:{:02}Z", offset % 60)
    }

    fn proposal_event(
        revision_id: &RevisionId,
        object_digit: &str,
        object_hash: &str,
        supersedes: &[&RevisionId],
        suffix: &str,
        offset: usize,
    ) -> ShoreEvent {
        ShoreEvent::new(
            EventType::WorkObjectProposed,
            format!("detail:{suffix}"),
            event_target(revision_id),
            Writer::shore_local("detail-read-test"),
            WorkObjectProposedPayload {
                engagement_id: EngagementId::new(format!("engagement:sha256:{}", "b".repeat(64))),
                work_object: WorkObjectProposal::Revision {
                    revision: Revision {
                        id: revision_id.clone(),
                        object_id: ObjectId::new(format!("obj:sha256:{}", object_digit.repeat(32))),
                        git_provenance: None,
                    },
                    summary: Some("detail Revision".to_owned()),
                    object_artifact_content_hash: object_hash.to_owned(),
                    supersedes: supersedes.iter().map(|id| (*id).clone()).collect(),
                },
            },
            occurred(offset),
        )
        .expect("build detail proposal")
    }

    fn assessment_event(
        revision_id: &RevisionId,
        assessment_id: AssessmentId,
        suffix: &str,
        offset: usize,
    ) -> ShoreEvent {
        ShoreEvent::new(
            EventType::ReviewAssessmentRecorded,
            ReviewAssessmentRecordedPayload::idempotency_key(
                revision_id,
                &TrackId::new(TRACK),
                suffix,
            ),
            event_target(revision_id),
            Writer::shore_local("detail-read-test"),
            ReviewAssessmentRecordedPayload {
                assessment_id,
                target: ReviewTargetRef::Revision {
                    revision_id: revision_id.clone(),
                },
                assessment: ReviewAssessment::NeedsChanges,
                summary: Some("inline assessment summary".to_owned()),
                summary_content_type: BodyContentType::TextMarkdown,
                summary_artifact_path: None,
                summary_byte_size: None,
                summary_content_hash: None,
                replaces_assessment_ids: Vec::new(),
                related_observation_ids: Vec::new(),
                related_input_request_ids: Vec::new(),
            },
            occurred(offset),
        )
        .expect("build detail assessment")
    }

    fn request_event(
        revision_id: &RevisionId,
        request_id: &InputRequestId,
        offset: usize,
    ) -> ShoreEvent {
        ShoreEvent::new(
            EventType::InputRequestOpened,
            InputRequestOpenedPayload::idempotency_key(
                revision_id,
                &TrackId::new(TRACK),
                request_id.as_str(),
            ),
            event_target(revision_id),
            Writer::shore_local("detail-read-test"),
            InputRequestOpenedPayload {
                input_request_id: request_id.clone(),
                target: ReviewTargetRef::Revision {
                    revision_id: revision_id.clone(),
                },
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
            occurred(offset),
        )
        .expect("build detail input request")
    }

    fn response_event(
        revision_id: &RevisionId,
        request_id: &InputRequestId,
        offset: usize,
    ) -> ShoreEvent {
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
            .expect("build detail response target"),
            Writer::shore_local("detail-read-test"),
            InputRequestRespondedPayload {
                input_request_response_id: InputRequestResponseId::new(
                    "input-response:sha256:detail",
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
            occurred(offset),
        )
        .expect("build detail response")
    }

    fn observation_event(
        revision_id: &RevisionId,
        content_hash: &str,
        offset: usize,
    ) -> ShoreEvent {
        ShoreEvent::new(
            EventType::ReviewObservationRecorded,
            ReviewObservationRecordedPayload::idempotency_key(
                revision_id,
                &TrackId::new(TRACK),
                "observation",
            ),
            event_target(revision_id),
            Writer::shore_local("detail-read-test"),
            ReviewObservationRecordedPayload {
                observation_id: ObservationId::new("observation:sha256:detail"),
                target: ReviewTargetRef::Revision {
                    revision_id: revision_id.clone(),
                },
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
            occurred(offset),
        )
        .expect("build detail observation")
    }

    fn validation_event(
        revision_id: &RevisionId,
        log_hashes: Vec<String>,
        offset: usize,
    ) -> ShoreEvent {
        ShoreEvent::new(
            EventType::ValidationCheckRecorded,
            ValidationCheckRecordedPayload::idempotency_key(
                revision_id,
                &TrackId::new(TRACK),
                "validation",
            ),
            event_target(revision_id),
            Writer::shore_local("detail-read-test"),
            ValidationCheckRecordedPayload {
                validation_check_id: ValidationCheckId::new("validation:sha256:detail"),
                target: ValidationTarget::Revision {
                    revision_id: revision_id.clone(),
                },
                check_name: "detail contract".to_owned(),
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
            occurred(offset),
        )
        .expect("build detail validation")
    }

    fn removal_event(content_hash: &str, suffix: &str, offset: usize) -> ShoreEvent {
        ShoreEvent::new(
            EventType::ArtifactRemoved,
            format!("detail:removal:{suffix}"),
            crate::session::event::EventTarget::for_journal(JournalId::new(JOURNAL)),
            Writer::shore_local("detail-read-test"),
            ArtifactRemovedPayload {
                content_hash: content_hash.to_owned(),
            },
            occurred(offset),
        )
        .expect("build detail removal")
    }

    fn signature_event(target: &ShoreEvent, suffix: &str, offset: usize) -> ShoreEvent {
        let signer = TestEd25519Signer::from_seed([37; 32]);
        let to_be_signed = EventToBeSigned::from_event(target, signer.signer_id())
            .expect("build signature message");
        let signature = signer
            .sign_event_message(
                &event_signature_pre_authentication_encoding(&to_be_signed)
                    .expect("encode signature message"),
            )
            .expect("sign detail target");
        let payload = EventSignatureRecordedPayload {
            target_event_id: target.event_id.clone(),
            target_event_record_hash: target.event_record_hash().expect("hash signature target"),
            attesting_signer: signer.signer_id().clone(),
            attestation: EventSignature::ed25519_v1(signature),
            inclusion_proof: None,
        };
        ShoreEvent::new(
            EventType::EventSignatureRecorded,
            format!("detail:signature:{suffix}"),
            crate::session::event::EventTarget::for_journal(JournalId::new(JOURNAL)),
            Writer::shore_local("detail-read-test"),
            payload,
            occurred(offset),
        )
        .expect("build detail signature")
    }

    fn unrelated_change_event(index: usize) -> ShoreEvent {
        let mut nonce = [0_u8; 32];
        nonce[..8].copy_from_slice(&u64::try_from(index + 1).unwrap().to_be_bytes());
        let declaration = build_change_declared(
            ChangeIdentityDescriptorV1::opaque_nonce(nonce),
            sha2::Sha256::digest(format!("detail-change:{index}").as_bytes()).into(),
        )
        .expect("build unrelated Change declaration");
        ShoreEvent::new(
            EventType::ChangeDeclared,
            format!("detail:unrelated-change:{index}"),
            crate::session::event::EventTarget::for_journal(JournalId::new(JOURNAL)),
            Writer::shore_local("detail-read-test"),
            declaration,
            format!("2026-08-25T20:01:{:02}Z", index % 60),
        )
        .expect("build unrelated Change event")
    }

    fn unrelated_authoritative_event(suffix: &str) -> ShoreEvent {
        let journal = JournalId::new(format!("journal:detail-read:{suffix}"));
        ShoreEvent::new(
            EventType::ReviewInitialized,
            ReviewInitializedPayload::idempotency_key(&journal),
            crate::session::event::EventTarget::for_journal(journal),
            Writer::shore_local("detail-read-test"),
            ReviewInitializedPayload {},
            "2026-08-25T20:02:00Z",
        )
        .expect("build unrelated authoritative event")
    }

    fn record(store: &EventStore, event: &ShoreEvent) {
        assert_eq!(
            store
                .record_event_once(event)
                .expect("record detail fixture event"),
            EventWriteOutcome::Created
        );
    }

    fn corrupt(path: &Path) {
        std::fs::write(path, b"{\"not\":\"a valid Shore event\"}")
            .expect("corrupt disposable carrier");
    }

    fn ready_read(fixture: &DetailFixture, seed: &RevisionId) -> ExactRevisionDetailReadV1 {
        match fixture
            .access
            .exact_revision_detail_read_v1(seed)
            .expect("read component detail")
        {
            ExactRevisionDetailReadRouteV1::Ready(read) => *read,
            other => panic!("current detail fixture must be Ready, got {other:?}"),
        }
    }

    #[test]
    fn exact_revision_detail_read_api_matches_the_reviewed_contract() {
        fn route_shape(route: ExactRevisionDetailReadRouteV1) {
            match route {
                ExactRevisionDetailReadRouteV1::Off
                | ExactRevisionDetailReadRouteV1::Unavailable => {}
                ExactRevisionDetailReadRouteV1::Ready(read) => {
                    let ExactRevisionDetailReadV1 { .. } = *read;
                }
            }
        }

        let method = DerivedHistoryAccess::exact_revision_detail_read_v1;
        let component_method = crate::session::derived_access::sqlite::ExactRevisionFactReadSnapshot::revision_component_event_ids;
        let audit_method = crate::session::derived_access::sqlite::ExactRevisionFactReadSnapshot::store_removal_audit_event_ids;
        let _ = (route_shape, method, component_method, audit_method);
    }

    #[test]
    fn rich_component_detail_read_selects_component_support_and_audit() {
        let fixture = DetailFixture::new(64, 0);
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
        let read = ready_read(&fixture, &fixture.head_revision);
        drop(guard);

        assert_eq!(read.as_of.sequence, authoritative.len() as u64);
        assert_eq!(read.event_count, authoritative.len());
        assert!(!read.projection_stamp.is_empty());
        assert_eq!(
            read.diagnostics,
            state_diagnostics(&expected_state).unwrap()
        );
        assert_eq!(
            read.events.len(),
            fixture.selected_count + fixture.support_count
        );
        assert_eq!(
            read.events
                .iter()
                .filter(|event| event.event_type == EventType::WorkObjectProposed)
                .count(),
            3,
            "the component closure carries exactly the three component proposals"
        );
        let audit_ids: BTreeSet<String> = read
            .audit_events
            .iter()
            .map(|event| event.event_id.as_str().to_owned())
            .collect();
        assert_eq!(
            audit_ids, fixture.expected_audit_ids,
            "the removal-audit closure is exactly every removal carrier, the detached \
             signatures targeting them, and every carrier referencing a removed hash"
        );
        let mut renormalized = read.events.clone();
        renormalized.reverse();
        renormalized.push(read.events[0].clone());
        normalize_events(&mut renormalized);
        assert_eq!(renormalized, read.events);

        let counters = &scope.snapshot().counters;
        assert_eq!(counters.strict_journal_inspections, 0);
        assert_eq!(counters.change_semantic_constructions, 0);
        assert_eq!(counters.change_projection_constructions, 0);
        assert_eq!(
            counters.fact_sqlite_rows_selected,
            (fixture.selected_count + fixture.expected_audit_ids.len()) as u64,
            "component selection and audit selection both record truthful row counts"
        );
    }

    #[test]
    fn component_selection_covers_head_superseded_and_absent_seeds() {
        let fixture = DetailFixture::new(4, 0);
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

        let from_head = snapshot
            .revision_component_event_ids(&fixture.head_revision, observed)
            .unwrap();
        assert_eq!(from_head.len(), fixture.selected_count);
        let from_root = snapshot
            .revision_component_event_ids(&fixture.root_revision, observed)
            .unwrap();
        assert_eq!(from_root, from_head, "any member seeds the same component");
        let from_fork = snapshot
            .revision_component_event_ids(&fixture.fork_revision, observed)
            .unwrap();
        assert_eq!(from_fork, from_head);

        let absent = snapshot
            .revision_component_event_ids(&revision("ff"), observed)
            .unwrap();
        assert!(absent.is_empty(), "an absent seed selects nothing");

        let unrelated = snapshot
            .revision_component_event_ids(&revision("b1"), observed)
            .unwrap();
        assert_eq!(
            unrelated.len(),
            1,
            "an unrelated single-revision component selects only its own carrier"
        );
        assert!(!from_head.iter().any(|id| unrelated.contains(id)));
        snapshot.finish().unwrap();
    }

    #[test]
    fn component_selection_pins_planner_fences_and_single_scan_growth() {
        fn probe(fixture: &DetailFixture) -> (Vec<String>, u64, usize) {
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
                .revision_component_event_query_plan(&fixture.head_revision, observed)
                .expect("read component query plan");
            let steps = snapshot
                .revision_component_event_vm_steps(&fixture.head_revision, observed)
                .expect("measure component query work");
            let selected = snapshot
                .revision_component_event_ids(&fixture.head_revision, observed)
                .unwrap()
                .len();
            snapshot.finish().unwrap();
            (details, steps, selected)
        }

        let small = DetailFixture::new(0, 0);
        let large = DetailFixture::new(64, 0);
        let (details, small_steps, small_selected) = probe(&small);
        let (_, large_steps, large_selected) = probe(&large);
        assert!(
            details
                .iter()
                .any(|detail| detail.contains("product_revision_identity")),
            "backward expansion must be fenced onto the identity index: {details:?}"
        );
        assert!(
            details
                .iter()
                .any(|detail| detail.contains("product_revision_edge_target")),
            "forward expansion must be fenced onto the edge-target index: {details:?}"
        );
        assert_eq!(small_selected, small.selected_count);
        assert_eq!(large_selected, large.selected_count);
        // The final membership test scans the retained range once by design;
        // the fences keep growth linear-once, never O(history * component).
        // Observed slope is ~25 VM steps per unrelated event (one membership
        // scan); a multiplicative regression multiplies that by component
        // size, which this bound rejects.
        assert!(
            large_steps <= small_steps + 64 * 40,
            "component work grew super-linearly with unrelated history: \
             small={small_steps}, large={large_steps}"
        );
    }

    #[test]
    fn removal_audit_closure_is_priced_by_removal_cardinality() {
        fn audit_probe(fixture: &DetailFixture) -> (u64, usize) {
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
            let steps = snapshot
                .store_removal_audit_vm_steps(observed)
                .expect("measure removal-audit work");
            let selected = snapshot
                .store_removal_audit_event_ids(observed)
                .unwrap()
                .len();
            snapshot.finish().unwrap();
            (steps, selected)
        }

        let base = DetailFixture::new(0, 0);
        let wide_history = DetailFixture::new(64, 0);
        let more_removals = DetailFixture::new(0, 8);
        let (base_steps, base_selected) = audit_probe(&base);
        let (history_steps, history_selected) = audit_probe(&wide_history);
        let (removal_steps, removal_selected) = audit_probe(&more_removals);

        assert_eq!(base_selected, base.expected_audit_ids.len());
        assert_eq!(history_selected, base_selected);
        assert_eq!(removal_selected, base_selected + 8);
        assert!(
            history_steps <= base_steps + 24,
            "removal-audit work grew with unrelated event history: \
             base={base_steps}, wide={history_steps}"
        );
        assert!(
            removal_steps > base_steps,
            "removal-audit work must scale with removal cardinality: \
             base={base_steps}, more={removal_steps}"
        );
    }

    #[test]
    fn detail_miss_off_and_preselection_unavailable_are_typed() {
        let off = DerivedHistoryAccess::from_mode(DerivedHistoryMode::Off);
        assert!(matches!(
            off.exact_revision_detail_read_v1(&revision("ff")).unwrap(),
            ExactRevisionDetailReadRouteV1::Off
        ));

        let fixture = DetailFixture::new(0, 0);
        let read = ready_read(&fixture, &revision("ff"));
        assert!(read.events.is_empty(), "a current miss is final and empty");
        assert!(
            !read.audit_events.is_empty(),
            "the audit closure is store-wide even on a component miss"
        );
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
            unavailable
                .exact_revision_detail_read_v1(&revision("ff"))
                .unwrap(),
            ExactRevisionDetailReadRouteV1::Unavailable
        ));
    }

    #[test]
    fn current_to_catching_up_before_selection_is_unavailable() {
        let fixture = DetailFixture::new(0, 0);
        fixture.access.current().expect("warm current generation");
        record(
            &EventStore::from_backend(&fixture.backend),
            &unrelated_authoritative_event("catching-up"),
        );
        assert!(matches!(
            fixture
                .access
                .exact_revision_detail_read_v1(&fixture.head_revision)
                .unwrap(),
            ExactRevisionDetailReadRouteV1::Unavailable
        ));
    }

    #[test]
    fn selected_support_and_audit_corruption_after_selection_is_terminal() {
        let selected = DetailFixture::new(0, 0);
        let selected_path = selected.carrier_path(&selected.selected_corruption_key);
        let selected_error = selected
            .access
            .exact_revision_detail_read_v1_with_hook(&selected.head_revision, |boundary| {
                if boundary == ExactRevisionDetailReadBoundary::Selected {
                    std::fs::remove_file(&selected_path)
                        .expect("remove disposable selected carrier");
                }
            })
            .expect_err("selected corruption must fail closed");
        assert!(!selected_error.is_empty());

        let support = DetailFixture::new(0, 0);
        let support_path = support.carrier_path(&support.support_corruption_key);
        let support_error = support
            .access
            .exact_revision_detail_read_v1_with_hook(&support.head_revision, |boundary| {
                if boundary == ExactRevisionDetailReadBoundary::SupportPlanned {
                    corrupt(&support_path);
                }
            })
            .expect_err("support corruption must fail closed");
        assert!(!support_error.is_empty());

        let audit = DetailFixture::new(0, 0);
        let audit_path = audit.carrier_path(&audit.audit_corruption_key);
        let audit_error = audit
            .access
            .exact_revision_detail_read_v1_with_hook(&audit.head_revision, |boundary| {
                if boundary == ExactRevisionDetailReadBoundary::SupportPlanned {
                    std::fs::remove_file(&audit_path).expect("remove disposable audit carrier");
                }
            })
            .expect_err("audit corruption must fail closed");
        assert!(!audit_error.is_empty());
    }

    #[test]
    fn postselection_authority_movement_is_terminal_not_fallback() {
        let fixture = DetailFixture::new(0, 0);
        let mut appended = false;
        let error = fixture
            .access
            .exact_revision_detail_read_v1_with_hook(&fixture.head_revision, |boundary| {
                if boundary == ExactRevisionDetailReadBoundary::Selected && !appended {
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
    fn audit_validation_refuses_non_audit_and_unbound_carriers() {
        let unrelated = unrelated_authoritative_event("non-audit");
        let error = validate_audit_events(std::slice::from_ref(&unrelated)).expect_err("non-audit");
        assert!(
            error.contains("does not reference a removed content hash"),
            "{error}"
        );

        let stray_binding = proposal_event(&revision("b9"), "09", &hash("c9"), &[], "stray", 0);
        let error = validate_audit_events(std::slice::from_ref(&stray_binding))
            .expect_err("unbound binding proposal");
        assert!(
            error.contains("does not reference a removed content hash"),
            "{error}"
        );

        let removal = removal_event(&hash("c9"), "stray-removal", 1);
        let bound = vec![removal.clone(), stray_binding];
        validate_audit_events(&bound).expect("a bound proposal validates");

        // A body-bearing carrier whose externalized note body is the removed
        // content is an admitted reference carrier.
        let body_observation = observation_event(&revision("b9"), &hash("c9"), 2);
        let referenced = vec![removal.clone(), body_observation.clone()];
        validate_audit_events(&referenced).expect("a referencing body carrier validates");
        let error = validate_audit_events(std::slice::from_ref(&body_observation))
            .expect_err("a body carrier without its removal");
        assert!(
            error.contains("does not reference a removed content hash"),
            "{error}"
        );

        // Signature arms: a signature must target an admitted removal, with
        // that removal's record hash.
        let stray_signature = signature_event(&body_observation, "targets-non-removal", 3);
        let error = validate_audit_events(&[removal.clone(), stray_signature])
            .expect_err("a signature over a non-removal");
        assert!(
            error.contains("does not target a removal carrier"),
            "{error}"
        );

        let mut forged = signature_event(&removal, "wrong-record-hash", 4);
        {
            let mut payload: EventSignatureRecordedPayload =
                serde_json::from_value(forged.payload.clone()).expect("decode signature payload");
            payload.target_event_record_hash = body_observation
                .event_record_hash()
                .expect("hash observation");
            forged.payload = serde_json::to_value(&payload).expect("encode signature payload");
        }
        let error = validate_audit_events(&[removal.clone(), forged])
            .expect_err("a signature with a forged record hash");
        assert!(
            error.contains("has the wrong target record hash"),
            "{error}"
        );

        let endorsed = signature_event(&removal, "valid-endorsement", 5);
        validate_audit_events(&[removal, endorsed])
            .expect("a removal-targeting signature validates");
    }

    #[test]
    fn selected_component_validation_requires_bindings_and_proposals() {
        let head = revision("a2");
        let orphan_fact = assessment_event(
            &head,
            AssessmentId::new("assess:sha256:orphan"),
            "orphan",
            0,
        );
        let error = validate_selected_component_events(std::slice::from_ref(&orphan_fact), &head)
            .expect_err("a component member without a proposal must fail");
        assert!(error.contains("no proposal carrier"), "{error}");

        let foreign_fact = assessment_event(
            &revision("a9"),
            AssessmentId::new("assess:sha256:foreign"),
            "foreign",
            1,
        );
        let error = validate_selected_component_events(std::slice::from_ref(&foreign_fact), &head)
            .expect_err("a closure without the addressed Revision must fail");
        assert!(
            error.contains("does not contain the addressed Revision"),
            "{error}"
        );

        let unbound = unrelated_authoritative_event("unbound");
        let error = validate_selected_component_events(std::slice::from_ref(&unbound), &head)
            .expect_err("an unbound carrier must fail");
        assert!(error.contains("no Revision binding"), "{error}");
    }
}
