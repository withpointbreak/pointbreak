mod artifact_removal;
mod artifact_transfer;
pub(in crate::session) mod assessment;
mod association;
pub(in crate::session) mod attention;
mod capture;
mod change;
mod change_migration;
mod change_read;
mod commit_range_liveness;
mod event_signature;
mod fact_port;
mod history;
mod ingest;
mod landing;
mod review_cursor;
mod revision_list;
mod revision_projection;
mod store_family;
mod store_identity;
mod store_link;
mod store_migrate_common_dir;
mod store_status;
pub(in crate::session) mod util;
pub(in crate::session) mod validation;

pub(in crate::session) mod input_request;
pub(in crate::session) mod observation;

fn capable_read_store_and_events(
    repo: &std::path::Path,
) -> crate::error::Result<(
    crate::session::store::resolution::ReadStore,
    Vec<crate::session::event::ShoreEvent>,
)> {
    let activated = {
        #[cfg(any(test, feature = "longitudinal-counting"))]
        let _phase = crate::bench_support::longitudinal::enter_derived_access_phase_v1(
            crate::bench_support::longitudinal::LongitudinalDerivedAccessPhaseV1::WorkflowActivatedCapabilityProbe,
        );
        crate::session::activated_store_capability_for_repo(repo)?
    };
    if activated.is_some() {
        let state = {
            #[cfg(any(test, feature = "longitudinal-counting"))]
            let _phase = crate::bench_support::longitudinal::enter_derived_access_phase_v1(
                crate::bench_support::longitudinal::LongitudinalDerivedAccessPhaseV1::WorkflowChangeReaderReplayH3,
            );
            change_reader_state_for_repo(repo)?
        };
        let ready =
            state
                .ready()
                .ok_or_else(|| {
                    crate::error::ShoreError::WorkflowInputInvalid {
                reason:
                    "migration_in_progress; normal product reads require complete Change authority"
                        .to_owned(),
            }
                })?;
        let events = ready.events().to_vec();
        let (store, _) = {
            #[cfg(any(test, feature = "longitudinal-counting"))]
            let _phase = crate::bench_support::longitudinal::enter_derived_access_phase_v1(
                crate::bench_support::longitudinal::LongitudinalDerivedAccessPhaseV1::WorkflowChangeStoreReopenInspection,
            );
            crate::session::store::resolution::resolve_change_read_store(repo)?
        };
        Ok((store, events))
    } else {
        let store = {
            #[cfg(any(test, feature = "longitudinal-counting"))]
            let _phase = crate::bench_support::longitudinal::enter_derived_access_phase_v1(
                crate::bench_support::longitudinal::LongitudinalDerivedAccessPhaseV1::OrdinaryReadStoreResolutionH2,
            );
            crate::session::store::resolution::resolve_read_store(repo)?
        };
        let events = crate::session::EventStore::from_backend(store.backend()).list_events()?;
        Ok((store, events))
    }
}

#[cfg(any(test, feature = "longitudinal-counting"))]
fn record_authoritative_replay_state() {
    if let Some(scope) = crate::bench_support::longitudinal::LongitudinalCountingScopeV1::current()
    {
        scope.record_observed_route_state_once(
            crate::bench_support::longitudinal::InteractionObservedRouteStateV1::AuthoritativeReplay,
        );
    }
}

#[cfg(any(test, feature = "longitudinal-counting"))]
fn record_derived_current_state() {
    if let Some(scope) = crate::bench_support::longitudinal::LongitudinalCountingScopeV1::current()
    {
        scope.record_observed_route_state_once(
            crate::bench_support::longitudinal::InteractionObservedRouteStateV1::DerivedCurrent,
        );
    }
}

#[cfg(any(test, feature = "longitudinal-counting"))]
fn record_fact_authoritative_fallback() {
    crate::bench_support::longitudinal::record_authoritative_fallback();
    crate::bench_support::longitudinal::record_full_history_fallback();
}

#[cfg(any(test, feature = "longitudinal-counting"))]
fn record_unlabeled_authoritative_fallback_state() {
    if let Some(scope) = crate::bench_support::longitudinal::LongitudinalCountingScopeV1::current()
    {
        scope.record_observed_route_state_once(
            crate::bench_support::longitudinal::InteractionObservedRouteStateV1::UnlabeledFallbackToAuthoritative,
        );
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

fn complete_current_derived_fact_projection_v1<T>(
    context: crate::session::PublicReadCommandContextV1,
    project: impl FnOnce(&crate::session::store::resolution::ReadStore) -> crate::error::Result<T>,
) -> crate::error::Result<T> {
    #[cfg(any(test, feature = "longitudinal-counting"))]
    let projection_phase = crate::bench_support::longitudinal::enter_derived_access_phase_v1(
        crate::bench_support::longitudinal::LongitudinalDerivedAccessPhaseV1::FactWorkflowProjection,
    );
    let result = project(context.read_store());
    #[cfg(any(test, feature = "longitudinal-counting"))]
    drop(projection_phase);
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            #[cfg(any(test, feature = "longitudinal-counting"))]
            record_derived_selection_failed_closed_state();
            return Err(error);
        }
    };
    if let Err(error) = context.postflight() {
        #[cfg(any(test, feature = "longitudinal-counting"))]
        record_derived_selection_failed_closed_state();
        return Err(error);
    }
    #[cfg(any(test, feature = "longitudinal-counting"))]
    record_derived_current_state();
    Ok(result)
}

fn complete_unavailable_fact_fallback_v1<T>(
    context: crate::session::PublicReadCommandContextV1,
    repo: &std::path::Path,
    project: impl FnOnce(
        &crate::session::store::resolution::ReadStore,
        &[crate::session::event::ShoreEvent],
    ) -> crate::error::Result<T>,
) -> crate::error::Result<T> {
    #[cfg(any(test, feature = "longitudinal-counting"))]
    record_fact_authoritative_fallback();
    let reader = change_read::public_read_change_reader_v1(context, repo)?;
    let result = project(reader.read_store(), reader.events())?;
    reader.postflight()?;
    #[cfg(any(test, feature = "longitudinal-counting"))]
    record_unlabeled_authoritative_fallback_state();
    Ok(result)
}

pub use artifact_removal::{
    CompactOptions, CompactResult, RemoveOptions, RemoveResult, RemoveSelector, RemovedContent,
    SkippedRemoval, SweepOutcome, SweptBlob, compact_store, remove_content,
};
pub(crate) use artifact_transfer::selected_support_content_hashes;
pub use artifact_transfer::{
    ArtifactKind, ArtifactRef, ImportArtifactOptions, ImportArtifactOutcome, ImportArtifactResult,
    export_artifact, import_artifact, referenced_artifacts,
};
pub use assessment::{
    AssessmentAddOptions, AssessmentAddResult, AssessmentRecordStatus, AssessmentShowFilters,
    AssessmentShowOptions, AssessmentShowResult, AssessmentTargetSelector, AssessmentView,
    CurrentAssessmentStatus, CurrentAssessmentView, record_assessment, show_assessments,
    show_assessments_with_public_read_context,
};
pub use association::{
    AssociateCommitOptions, AssociateCommitResult, AssociateRefOptions, AssociateRefResult,
    AssociationAxis, ListAssociationsOptions, ListAssociationsResult, WithdrawCommitOptions,
    WithdrawCommitResult, WithdrawRefOptions, WithdrawRefResult, associate_commit, associate_ref,
    list_associations, withdraw_commit, withdraw_ref,
};
pub use attention::{
    AttentionAssessmentRecord, AttentionDetail, AttentionFreshness, AttentionFreshnessState,
    AttentionItem, AttentionListOptions, AttentionListResult, AttentionProjection, AttentionTier,
    PublicReadAttentionFallbackV1, PublicReadAttentionRouteV1,
    complete_public_read_attention_fallback_v1, list_attention,
    list_attention_with_public_read_context,
};
pub use capture::{
    CaptureDiffstat, CaptureOptions, CaptureResult, CommitRangeSpec, RootCommitSpec, StagedSpec,
    UnstagedSpec, WorktreeSpec, capture_review, capture_worktree_review, diffstat_from_files,
};
pub use change::{
    CHANGE_OPERATION_SCHEMA_V1, ChangeAdvanceV1, ChangeCaptureOptions, ChangeCaptureReceiptV1,
    ChangeCaptureRevisionV1, ChangeCreateOptions, ChangeLinkOptions, ChangeMembershipOptions,
    ChangeMembershipWithdrawalOptions, ChangeOperationEventOutcomeV1,
    ChangeOperationEventReceiptV1, ChangeOperationReceiptV1, ChangeRelationOptions,
    ChangeRelationWithdrawalOptions, assert_change_revision_relation, capture_change_revision,
    create_change, join_revision_to_change, link_changes, withdraw_change_revision_relation,
    withdraw_revision_from_change,
};
pub use change_migration::{
    BULK_ADOPTION_BACKUP_MANIFEST_FILE_V1, BULK_ADOPTION_BACKUP_RECEIPT_FILE_V1,
    BULK_ADOPTION_BACKUP_RESTORE_RECEIPT_SCHEMA_V1, BULK_ADOPTION_DRY_RUN_SCHEMA_V1,
    BULK_ADOPTION_EXECUTION_PLAN_FILE_V1, BULK_ADOPTION_MIGRATION_RECEIPT_SCHEMA_V1,
    BULK_ADOPTION_MINIMUM_READER_PROFILE_V1, BULK_ADOPTION_OWNER_DECISIONS_SCHEMA_V1,
    BulkAdoptionBackupRestoreDispositionV1, BulkAdoptionBackupRestoreReceiptV1,
    BulkAdoptionDryRunAnomalyV1, BulkAdoptionDryRunChangeV1, BulkAdoptionDryRunDocumentV1,
    BulkAdoptionDryRunOptions, BulkAdoptionDryRunRootV1, BulkAdoptionMigrationDispositionV1,
    BulkAdoptionMigrationOptions, BulkAdoptionMigrationReceiptV1,
    BulkAdoptionOverlapIdentityDecisionV1, BulkAdoptionOwnerDecisionManifestV1,
    BulkAdoptionRetainedAllocationV1, BulkAdoptionRetainedManifestV1, dry_run_bulk_adoption,
    migrate_bulk_adoption, restore_bulk_adoption_backup,
};
#[cfg(feature = "bench")]
pub(crate) use change_migration::{
    activate_empty_store_for_qualification, expected_empty_store_qualification_status,
};
pub use change_read::{
    ChangeReaderPresentationV1, ChangeReaderReadyV1, ChangeReaderStateV1,
    change_reader_state_for_repo,
};
pub use commit_range_liveness::{
    CommitGraphCondition, CommitLiveness, LivenessEnrichment, REF_REWRITTEN_CODE, RefContinuity,
    RefContinuityReport, RefContinuityView, Retention, commit_graph_stamp, diagnose_ref_continuity,
    effective_integration_ref, enrich_liveness, resolve_default_integration_ref,
};
pub use event_signature::{
    EventSignatureRecordOptions, EventSignatureRecordResult, record_event_signature,
};
pub use fact_port::{FactPortOptions, FactPortResultV1, port_review_fact};
#[cfg(test)]
pub(crate) use history::history_base_from_events;
pub use history::{
    BaseEntry, BaseHistoryProjection, BaseProjectionConfig, CHANGE_TIMELINE_QUERY_FIELDS,
    DistinctValues, EVENT_QUERY_FIELDS, EventRecordExtras, HistoryCursor, HistoryOrder,
    HistoryPage, HistoryQuery, KNOWN_QUERY_KEYS, ParsedQuery, QueriedHistory, QueryClause,
    QueryDiagnostic, QueryDiagnosticCode, QuerySurface, RANGE_ANCHOR_FIELD,
    REVISION_ATTENTION_VALUES, REVISION_QUERY_FIELDS, ReviewHistoryEntry, ReviewHistoryFilters,
    ReviewHistoryOptions, ReviewHistoryResult, ReviewHistorySummary, SearchRecord,
    apply_history_query, build_haystack, count_new_since, default_history_page_projection,
    event_history_search_record, history_base_projection, matches_query, parse_search_query,
    parse_search_query_for, redact_history_bodies, review_history,
};
pub(crate) use history::{
    MatchKind, history_entries_from_selected_events, match_kind_for, range_bound,
    resolve_assessment_value, resolve_type_value, tag_completion_key,
};
pub use ingest::{
    ImportEventOptions, IngestEventsOptions, IngestEventsResult, import_event, ingest_events,
};
#[cfg(feature = "bench")]
pub(in crate::session) use ingest::{
    IngestBatchSession, ingest_events_with_clock, prepare_events_for_ingest,
};
#[cfg(feature = "bench")]
pub(crate) use ingest::{carrier_target_full_scan_count, reset_carrier_target_full_scan_count};
pub use input_request::{
    InputRequestFetchOptions, InputRequestFetchResult, InputRequestListOptions,
    InputRequestListResult, InputRequestOpenOptions, InputRequestOpenResult,
    InputRequestRespondOptions, InputRequestRespondResult, InputRequestResponseView,
    InputRequestStatus, InputRequestStatusFilter, InputRequestTargetSelector, InputRequestView,
    fetch_input_request, list_input_requests, list_input_requests_with_public_read_context,
    open_input_request, respond_input_request,
};
pub use landing::{LandCommitOptions, LandCommitResultV1, land_commit};
pub use observation::{
    ObservationAddOptions, ObservationAddResult, ObservationListOptions, ObservationListResult,
    ObservationStatus, ObservationTargetSelector, ObservationView, list_observations,
    list_observations_with_public_read_context, record_observation, validated_track_id,
};
pub use review_cursor::{
    CommitProofStateV1, CommitSourceStateV1, REVIEW_CURSOR_SCHEMA_V1, ReviewCursorRefusalV1,
    ReviewCursorSelectionV1, ReviewCursorV1, ReviewSourceBindingV1, ReviewSourceFingerprintV1,
    ReviewSourcePathStateV1, ReviewSourceRequestV1, WorktreeSourceStateV1, change_graph_token,
    review_source_binding, select_review_cursor, validate_review_cursor_for_write,
};
pub(crate) use review_cursor::{
    exact_revision_from_review_cursor, exact_revision_from_transition_cursor,
    validate_review_cursor_for_transition,
};
pub(crate) use revision_list::list_revisions_from_selected_events;
pub use revision_list::{
    RefFilterMode, RevisionListEntry, RevisionListOptions, RevisionListResult,
    UnreachableVisibility, list_revisions, list_units_for_ref,
};
pub use revision_projection::{
    MemberReadback, RevisionOverview, RevisionOverviewsOptions, RevisionProjectionIdentity,
    RevisionProjectionRow, RevisionProjectionSummary, RevisionRecordInputs, RevisionSearchRecord,
    RevisionShowFilters, RevisionShowOptions, RevisionShowResult, SnapshotContentState,
    SnapshotOrder, SnapshotSummaryCache, SnapshotSummaryCounts, ValidationCheckDisposition,
    ValidationContinuitySummary, ValidationContinuityView, build_revision_search_record,
    classify_validation_continuity, current_assessment_includes_follow_up, show_revision,
    show_revision_for_change_reader, show_revision_for_change_reader_ready,
    show_revision_for_inspector, show_revision_overviews, stale_review_fact_count,
};
pub(crate) use revision_projection::{
    revision_overviews_from_selected_events, show_revision_from_selected_events,
};
pub use store_family::{
    StoreForgetOptions, StoreForgetResult, StoreListEntry, StoreListResult, forget_family_store,
    list_family_stores,
};
pub use store_identity::{
    StoreFamily, StoreIdentity, StoreIdentityOptions, StorePlacement, store_identity,
};
pub use store_link::{
    StoreLinkOptions, StoreLinkPreview, StoreLinkResult, StoreUnlinkOptions, StoreUnlinkResult,
    link_store_to_family, preview_link_to_family, unlink_store_from_family,
};
pub use store_migrate_common_dir::{
    MigrateToCommonDirOptions, MigrateToCommonDirResult, migrate_store_to_common_dir,
};
pub use store_status::{
    StoreSensitivityPathGroup, StoreStatusArtifactInventory, StoreStatusInventory,
    StoreStatusOptions, StoreStatusResult, StoreStatusRevisionObject, StoreStatusSensitivity,
    StoreStatusSensitivityExcludeGlob, StoreStatusSensitivityFinding, explain_store_sensitivity,
    store_status,
};
pub use validation::{
    ValidationAddOptions, ValidationAddResult, ValidationCheckView, ValidationListFilters,
    ValidationListOptions, ValidationListResult, list_validation_checks,
    list_validation_checks_with_public_read_context, record_validation_check,
};
pub(crate) use validation::{
    ValidationCheckProjectionOptions, annotate_validation_supersession, project_validation_checks,
};

#[cfg(test)]
mod interaction_attribution_tests {
    use std::path::Path;
    use std::process::Command;

    use super::*;
    use crate::bench_support::longitudinal::{
        InteractionActorV1, InteractionObservedRouteStateV1, LongitudinalCountingScopeV1,
        LongitudinalCountingSnapshotV1, LongitudinalDerivedAccessPhaseV1 as Phase,
    };
    use crate::crypto::{EventSigner, TestEd25519Signer};
    use crate::session::derived_access::lifecycle::{DerivedAccessLifecycle, LifecycleControl};
    use crate::session::derived_access::product_contract::DerivedAccessProfile;
    use crate::session::store::capabilities::REVIEW_CHANGE_REVISION_COHORT_V1;

    #[test]
    fn all_four_authoritative_routes_record_one_state_and_the_inactive_shared_path() {
        let (repo, _) = captured_repo();

        for snapshot in [
            observe(|| show_assessments(AssessmentShowOptions::new(repo.path()))),
            observe(|| list_input_requests(InputRequestListOptions::new(repo.path()))),
            observe(|| list_observations(ObservationListOptions::new(repo.path()))),
            observe(|| list_validation_checks(ValidationListOptions::new(repo.path()))),
        ] {
            assert_eq!(
                snapshot.observed_route_states,
                vec![InteractionObservedRouteStateV1::AuthoritativeReplay]
            );
            let phases = snapshot
                .derived_access_phases
                .iter()
                .map(|sample| sample.phase)
                .collect::<Vec<_>>();
            assert_eq!(
                phases,
                vec![
                    Phase::WorkflowActivatedCapabilityProbe,
                    Phase::OrdinaryReadStoreResolutionH2,
                    Phase::GitContextResolution,
                    Phase::RouteRevisionSelection,
                    Phase::RouteProjectionFold,
                ]
            );
        }
    }

    #[test]
    fn activated_shared_path_preserves_each_probe_h3_and_reopen_invocation() {
        let (repo, _) = captured_repo();
        migrate_to_ready(repo.path());
        let counting = LongitudinalCountingScopeV1::new("a".repeat(64)).unwrap();
        counting.record_execution_actor_once(InteractionActorV1::RequestReader);
        let _guard = counting.enter();

        capable_read_store_and_events(repo.path()).unwrap();
        capable_read_store_and_events(repo.path()).unwrap();

        let phases = counting
            .snapshot()
            .derived_access_phases
            .iter()
            .map(|sample| sample.phase)
            .filter(|phase| {
                matches!(
                    phase,
                    Phase::WorkflowActivatedCapabilityProbe
                        | Phase::WorkflowChangeReaderReplayH3
                        | Phase::WorkflowChangeStoreReopenInspection
                        | Phase::OrdinaryReadStoreResolutionH2
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            phases,
            vec![
                Phase::WorkflowActivatedCapabilityProbe,
                Phase::WorkflowChangeReaderReplayH3,
                Phase::WorkflowChangeStoreReopenInspection,
                Phase::WorkflowActivatedCapabilityProbe,
                Phase::WorkflowChangeReaderReplayH3,
                Phase::WorkflowChangeStoreReopenInspection,
            ]
        );
    }

    #[test]
    fn five_qualified_fact_cells_consume_one_context_without_duplicate_reader_work() {
        let (repo, revision_id) = captured_repo();
        migrate_to_ready(repo.path());
        let cursor = crate::session::store_capability_for_repo(repo.path())
            .unwrap()
            .cursor;
        let context = || crate::session::prepare_public_read_command_context_v1(repo.path());
        let snapshots = [
            observe(|| {
                show_assessments_with_public_read_context(
                    AssessmentShowOptions::new(repo.path())
                        .with_exact_revision_id(revision_id.clone())
                        .with_track("agent:reviewer"),
                    context()?,
                )
            }),
            observe(|| {
                show_assessments_with_public_read_context(
                    AssessmentShowOptions::new(repo.path())
                        .with_exact_revision_id(revision_id.clone())
                        .with_track("agent:reviewer")
                        .with_include_summary(true),
                    context()?,
                )
            }),
            observe(|| {
                list_input_requests_with_public_read_context(
                    InputRequestListOptions::new(repo.path())
                        .with_exact_revision_id(revision_id.clone()),
                    context()?,
                )
            }),
            observe(|| {
                list_observations_with_public_read_context(
                    ObservationListOptions::new(repo.path())
                        .with_exact_revision_id(revision_id.clone())
                        .with_track("agent:reviewer"),
                    context()?,
                )
            }),
            observe(|| {
                list_validation_checks_with_public_read_context(
                    ValidationListOptions::new(repo.path())
                        .with_exact_revision_id(revision_id.clone())
                        .with_track("agent:reviewer"),
                    context()?,
                )
            }),
        ];

        for snapshot in snapshots {
            assert_eq!(
                snapshot.counters.directory_entries_walked,
                cursor.journal_record_count
            );
            assert_eq!(
                snapshot.counters.carrier_opens,
                cursor.journal_record_count + 2
            );
            assert_eq!(snapshot.counters.change_capability_carriers_opened, 2);
            assert_eq!(snapshot.counters.event_decodes, cursor.event_count);
            assert_eq!(snapshot.counters.event_validations, cursor.event_count);
            assert_eq!(
                snapshot.observed_route_states,
                vec![InteractionObservedRouteStateV1::UnlabeledFallbackToAuthoritative]
            );
            assert_eq!(
                snapshot
                    .derived_access_phases
                    .iter()
                    .map(|sample| sample.phase)
                    .collect::<Vec<_>>(),
                vec![
                    Phase::WorkflowChangeReaderReplayH3,
                    Phase::GitContextResolution,
                    Phase::RouteRevisionSelection,
                    Phase::RouteProjectionFold,
                ]
            );
        }
    }

    #[test]
    fn first_three_qualified_fact_cells_use_current_exact_revision_facts_with_parity() {
        let (repo, revision_id) = captured_repo();
        let observation = record_observation(
            ObservationAddOptions::new(repo.path())
                .with_exact_revision_id(revision_id.clone())
                .with_track("agent:reviewer")
                .with_title("related observation"),
        )
        .unwrap();
        let open_options = || {
            InputRequestOpenOptions::new(repo.path())
                .with_exact_revision_id(revision_id.clone())
                .with_track("agent:reviewer")
                .with_title("qualified open request")
                .with_body("request body ".repeat(1000))
                .with_reason_code(
                    crate::session::event::InputRequestReasonCode::ManualDecisionRequired,
                )
        };
        let request = open_input_request(open_options().with_idempotency_key("request-a")).unwrap();
        let answered = open_input_request(
            InputRequestOpenOptions::new(repo.path())
                .with_exact_revision_id(revision_id.clone())
                .with_track("human:operator")
                .with_title("answered advisory request")
                .with_body("answered body ".repeat(1000))
                .with_assertion_mode(crate::session::event::AssertionMode::Advisory)
                .with_reason_code(
                    crate::session::event::InputRequestReasonCode::InsufficientEvidence,
                ),
        )
        .unwrap();
        respond_input_request(
            InputRequestRespondOptions::new(repo.path(), answered.input_request_id)
                .with_outcome(crate::session::event::InputRequestResponseOutcome::Approved)
                .with_reason("response reason ".repeat(1000)),
        )
        .unwrap();
        let replaced = record_assessment(
            AssessmentAddOptions::new(repo.path())
                .with_exact_revision_id(revision_id.clone())
                .with_track("agent:reviewer")
                .with_assessment(crate::session::event::ReviewAssessment::NeedsChanges)
                .with_summary("superseded summary"),
        )
        .unwrap();
        record_assessment(
            AssessmentAddOptions::new(repo.path())
                .with_exact_revision_id(revision_id.clone())
                .with_track("human:operator")
                .with_assessment(crate::session::event::ReviewAssessment::AcceptedWithFollowUp)
                .with_summary("cross-track replacement")
                .replacing(replaced.assessment_id),
        )
        .unwrap();
        let assessment_options_for_write = || {
            AssessmentAddOptions::new(repo.path())
                .with_exact_revision_id(revision_id.clone())
                .with_track("agent:reviewer")
                .with_assessment(crate::session::event::ReviewAssessment::Accepted)
                .with_summary("qualified assessment summary ".repeat(1000))
                .related_observation(observation.observation_id.clone())
                .related_input_request(request.input_request_id.clone())
        };
        let assessment =
            record_assessment(assessment_options_for_write().with_idempotency_key("assessment-a"))
                .unwrap();
        assert!(
            assessment
                .assessment_id
                .as_str()
                .starts_with("assess:sha256:")
        );
        let inline_removal_signer = TestEd25519Signer::from_seed([0x41; 32]);
        let inline_removal_signer_id = inline_removal_signer.signer_id().clone();
        remove_content(
            RemoveOptions::new(repo.path(), RemoveSelector::Revision(revision_id.clone()))
                .sign_with(inline_removal_signer),
        )
        .unwrap();
        let store_dir = crate::git::git_common_dir(repo.path())
            .unwrap()
            .join("pointbreak");
        let removal_event_id = crate::session::EventStore::open(&store_dir)
            .list_events()
            .unwrap()
            .into_iter()
            .find(|event| event.event_type == crate::session::event::EventType::ArtifactRemoved)
            .unwrap()
            .event_id;
        let detached_removal_signer = TestEd25519Signer::from_seed([0x42; 32]);
        let detached_removal_signer_id = detached_removal_signer.signer_id().clone();
        record_event_signature(crate::session::EventSignatureRecordOptions::new(
            repo.path(),
            removal_event_id,
            detached_removal_signer,
        ))
        .unwrap();
        let trust = crate::session::event_signature_trust_set(serde_json::json!({
            "allowedSigners": {
                "actor:git-email:pointbreak@example.test": [
                    inline_removal_signer_id.as_str(),
                    detached_removal_signer_id.as_str()
                ]
            }
        }))
        .unwrap();
        std::fs::write(repo.path().join("file.txt"), "three\n").unwrap();
        let unrelated_revision = capture_worktree_review(
            CaptureOptions::new(repo.path()).with_supersedes(vec![revision_id.clone()]),
        )
        .unwrap();
        let unrelated_request = open_input_request(
            InputRequestOpenOptions::new(repo.path())
                .with_exact_revision_id(unrelated_revision.revision_id)
                .with_track("agent:unrelated")
                .with_title("unrelated duplicate request")
                .with_reason_code(
                    crate::session::event::InputRequestReasonCode::ManualDecisionRequired,
                ),
        )
        .unwrap();
        migrate_to_ready(repo.path());

        let assessment_options = || {
            AssessmentShowOptions::new(repo.path())
                .with_exact_revision_id(revision_id.clone())
                .with_track("agent:reviewer")
        };
        let assessment_summary_options = || {
            assessment_options()
                .with_include_summary(true)
                .with_removal_policy(crate::session::RemovalPolicy::Advisory)
        };
        let removed_summary_options = || {
            assessment_options()
                .with_include_summary(true)
                .with_trust_set(trust.clone())
                .with_removal_policy(crate::session::RemovalPolicy::TrustedStrict)
        };
        let request_options = || {
            InputRequestListOptions::new(repo.path())
                .with_exact_revision_id(revision_id.clone())
                .with_trust_set(trust.clone())
                .with_removal_policy(crate::session::RemovalPolicy::TrustedStrict)
        };
        let lifecycle = publish_current_derived_generation(repo.path());
        append_semantic_duplicate(repo.path(), &lifecycle, &request.event_id, "request-b");
        append_semantic_duplicate(
            repo.path(),
            &lifecycle,
            &assessment.event_id,
            "assessment-b",
        );
        append_semantic_duplicate(
            repo.path(),
            &lifecycle,
            &unrelated_request.event_id,
            "unrelated-request-b",
        );

        let authoritative_context = || {
            crate::session::prepare_public_read_command_context_v1(repo.path())
                .unwrap()
                .with_derived_access_profile_for_test(DerivedAccessProfile::Off)
        };
        let authoritative_assessment = show_assessments_with_public_read_context(
            assessment_options(),
            authoritative_context(),
        )
        .unwrap();
        let authoritative_summary = show_assessments_with_public_read_context(
            assessment_summary_options(),
            authoritative_context(),
        )
        .unwrap();
        let authoritative_removed_summary = show_assessments_with_public_read_context(
            removed_summary_options(),
            authoritative_context(),
        )
        .unwrap();
        let authoritative_requests = list_input_requests_with_public_read_context(
            request_options(),
            authoritative_context(),
        )
        .unwrap();

        let counting = LongitudinalCountingScopeV1::new("d".repeat(64)).unwrap();
        counting.record_execution_actor_once(InteractionActorV1::RequestReader);
        let guard = counting.enter();
        let actual_assessment = show_assessments_with_public_read_context(
            assessment_options(),
            crate::session::prepare_public_read_command_context_v1(repo.path()).unwrap(),
        )
        .unwrap();
        drop(guard);
        assert_eq!(actual_assessment, authoritative_assessment);
        assert!(actual_assessment.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains(unrelated_request.input_request_id.as_str())
        }));
        let snapshot = counting.snapshot();
        assert_current_fact_route(&snapshot);
        assert_eq!(snapshot.counters.body_artifact_reads, 0);

        let counting = LongitudinalCountingScopeV1::new("e".repeat(64)).unwrap();
        counting.record_execution_actor_once(InteractionActorV1::RequestReader);
        let guard = counting.enter();
        let actual_summary = show_assessments_with_public_read_context(
            assessment_summary_options(),
            crate::session::prepare_public_read_command_context_v1(repo.path()).unwrap(),
        )
        .unwrap();
        drop(guard);
        assert_eq!(actual_summary, authoritative_summary);
        let snapshot = counting.snapshot();
        assert_current_fact_route(&snapshot);
        assert!(snapshot.counters.body_artifact_reads > 0);

        let counting = LongitudinalCountingScopeV1::new("1".repeat(64)).unwrap();
        counting.record_execution_actor_once(InteractionActorV1::RequestReader);
        let guard = counting.enter();
        let actual_removed_summary = show_assessments_with_public_read_context(
            removed_summary_options(),
            crate::session::prepare_public_read_command_context_v1(repo.path()).unwrap(),
        )
        .unwrap();
        drop(guard);
        assert_eq!(actual_removed_summary, authoritative_removed_summary);
        assert!(
            actual_removed_summary
                .assessments
                .iter()
                .any(|assessment| assessment.summary_content_state.is_removed())
        );
        let snapshot = counting.snapshot();
        assert_current_fact_route(&snapshot);
        assert_eq!(snapshot.counters.body_artifact_reads, 0);

        let counting = LongitudinalCountingScopeV1::new("f".repeat(64)).unwrap();
        counting.record_execution_actor_once(InteractionActorV1::RequestReader);
        let guard = counting.enter();
        let actual_requests = list_input_requests_with_public_read_context(
            request_options(),
            crate::session::prepare_public_read_command_context_v1(repo.path()).unwrap(),
        )
        .unwrap();
        drop(guard);
        assert_eq!(actual_requests, authoritative_requests);
        assert!(
            actual_requests
                .input_requests
                .iter()
                .any(|request| request.body_content_state.is_removed())
        );
        let snapshot = counting.snapshot();
        assert_current_fact_route(&snapshot);
        assert_eq!(snapshot.counters.body_artifact_reads, 0);
    }

    #[test]
    fn last_two_qualified_fact_cells_use_current_exact_revision_facts_with_parity() {
        let (repo, revision_id) = captured_repo();
        let original = record_observation(
            ObservationAddOptions::new(repo.path())
                .with_exact_revision_id(revision_id.clone())
                .with_track("agent:reviewer")
                .with_title("reviewer finding")
                .with_body("observation body ".repeat(1000))
                .with_target(ObservationTargetSelector::file("file.txt"))
                .with_tag("correctness")
                .with_confidence("high")
                .with_idempotency_key("observation-a"),
        )
        .unwrap();
        let cross_track = record_observation(
            ObservationAddOptions::new(repo.path())
                .with_exact_revision_id(revision_id.clone())
                .with_track("human:operator")
                .with_title("cross-track correction and response")
                .superseding(original.observation_id.clone())
                .responding_to(original.observation_id.clone()),
        )
        .unwrap();
        let active = record_observation(
            ObservationAddOptions::new(repo.path())
                .with_exact_revision_id(revision_id.clone())
                .with_track("agent:reviewer")
                .with_title("active reviewer finding")
                .with_tag("usability"),
        )
        .unwrap();

        let log_hash = format!("sha256:{}", "7".repeat(64));
        let validation = record_validation_check(
            ValidationAddOptions::new(repo.path())
                .with_exact_revision_id(revision_id.clone())
                .with_track("agent:reviewer")
                .with_check_name("cargo test")
                .with_command("cargo test --locked")
                .with_status(crate::model::ValidationStatus::Passed)
                .with_exit_code(0)
                .with_source_fingerprint("source:sha256:task-3-2")
                .with_summary("validation summary ".repeat(1000))
                .with_started_at("2026-08-24T19:00:00Z")
                .with_completed_at("2026-08-24T19:01:00Z")
                .with_log_artifact_content_hash(log_hash.clone())
                .with_idempotency_key("validation-a"),
        )
        .unwrap();
        record_validation_check(
            ValidationAddOptions::new(repo.path())
                .with_exact_revision_id(revision_id.clone())
                .with_track("human:operator")
                .with_check_name("manual inspection")
                .with_status(crate::model::ValidationStatus::Failed)
                .with_exit_code(1)
                .with_completed_at("2026-08-24T19:00:30Z"),
        )
        .unwrap();

        let inline_removal_signer = TestEd25519Signer::from_seed([0x51; 32]);
        let inline_removal_signer_id = inline_removal_signer.signer_id().clone();
        remove_content(
            RemoveOptions::new(repo.path(), RemoveSelector::Revision(revision_id.clone()))
                .sign_with(inline_removal_signer),
        )
        .unwrap();
        let store_dir = crate::git::git_common_dir(repo.path())
            .unwrap()
            .join("pointbreak");
        let event_store = crate::session::EventStore::open(&store_dir);
        let log_removal = crate::session::event::ShoreEvent::new(
            crate::session::event::EventType::ArtifactRemoved,
            crate::session::event::ArtifactRemovedPayload::idempotency_key(&log_hash),
            crate::session::event::EventTarget::for_journal(crate::model::JournalId::new(format!(
                "{}:default",
                crate::model::id_prefix::JOURNAL
            ))),
            crate::session::event::Writer::shore_local("task-3-2-test"),
            crate::session::event::ArtifactRemovedPayload {
                content_hash: log_hash,
            },
            "2026-08-24T19:02:00Z",
        )
        .unwrap();
        event_store.record_event_once(&log_removal).unwrap();
        let detached_removal_signer = TestEd25519Signer::from_seed([0x52; 32]);
        let detached_removal_signer_id = detached_removal_signer.signer_id().clone();
        record_event_signature(crate::session::EventSignatureRecordOptions::new(
            repo.path(),
            log_removal.event_id,
            detached_removal_signer,
        ))
        .unwrap();
        let trust = crate::session::event_signature_trust_set(serde_json::json!({
            "allowedSigners": {
                "actor:git-email:pointbreak@example.test": [
                    inline_removal_signer_id.as_str(),
                    detached_removal_signer_id.as_str()
                ]
            }
        }))
        .unwrap();

        migrate_to_ready(repo.path());
        let lifecycle = publish_current_derived_generation(repo.path());
        append_semantic_duplicate(repo.path(), &lifecycle, &original.event_id, "observation-b");
        append_semantic_duplicate(
            repo.path(),
            &lifecycle,
            &validation.event_id,
            "validation-b",
        );

        let observation_options = || {
            ObservationListOptions::new(repo.path())
                .with_exact_revision_id(revision_id.clone())
                .with_track("agent:reviewer")
                .with_trust_set(trust.clone())
                .with_removal_policy(crate::session::RemovalPolicy::TrustedStrict)
        };
        let validation_options = || {
            ValidationListOptions::new(repo.path())
                .with_exact_revision_id(revision_id.clone())
                .with_track("agent:reviewer")
                .with_trust_set(trust.clone())
                .with_removal_policy(crate::session::RemovalPolicy::TrustedStrict)
        };
        let authoritative_context = || {
            crate::session::prepare_public_read_command_context_v1(repo.path())
                .unwrap()
                .with_derived_access_profile_for_test(DerivedAccessProfile::Off)
        };
        let authoritative_observations = list_observations_with_public_read_context(
            observation_options(),
            authoritative_context(),
        )
        .unwrap();
        let authoritative_validations = list_validation_checks_with_public_read_context(
            validation_options(),
            authoritative_context(),
        )
        .unwrap();

        let observation_counting = LongitudinalCountingScopeV1::new("4".repeat(64)).unwrap();
        observation_counting.record_execution_actor_once(InteractionActorV1::RequestReader);
        let guard = observation_counting.enter();
        let actual_observations = list_observations_with_public_read_context(
            observation_options(),
            crate::session::prepare_public_read_command_context_v1(repo.path()).unwrap(),
        )
        .unwrap();
        drop(guard);
        assert_eq!(actual_observations, authoritative_observations);
        let original_view = actual_observations
            .observations
            .iter()
            .find(|view| view.id == original.observation_id)
            .unwrap();
        assert_eq!(original_view.status, ObservationStatus::Superseded);
        assert_eq!(original_view.responded_by, vec![cross_track.observation_id]);
        assert!(original_view.body.is_none());
        assert!(original_view.body_content_state.is_removed());
        assert!(actual_observations.observations.iter().any(|view| view.id
            == active.observation_id
            && view.status == ObservationStatus::Active));
        assert!(actual_observations.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == crate::session::state::DUPLICATE_SEMANTIC_OBSERVATION_EVENT_CODE
        }));
        let observation_snapshot = observation_counting.snapshot();
        assert_eq!(observation_snapshot.counters.body_artifact_reads, 0);

        let validation_counting = LongitudinalCountingScopeV1::new("5".repeat(64)).unwrap();
        validation_counting.record_execution_actor_once(InteractionActorV1::RequestReader);
        let guard = validation_counting.enter();
        let actual_validations = list_validation_checks_with_public_read_context(
            validation_options(),
            crate::session::prepare_public_read_command_context_v1(repo.path()).unwrap(),
        )
        .unwrap();
        drop(guard);
        assert_eq!(actual_validations, authoritative_validations);
        let validation_view = actual_validations
            .validation_checks
            .iter()
            .find(|view| view.id == validation.validation_check_id)
            .unwrap();
        assert_eq!(
            validation_view.command.as_deref(),
            Some("cargo test --locked")
        );
        assert_eq!(validation_view.exit_code, Some(0));
        assert_eq!(
            validation_view.completed_at.as_deref(),
            Some("2026-08-24T19:01:00Z")
        );
        assert_eq!(validation_view.log_artifact_content_hashes.len(), 1);
        assert!(validation_view.summary.is_none());
        assert!(validation_view.summary_content_state.is_removed());
        assert!(actual_validations.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == crate::session::state::DUPLICATE_SEMANTIC_VALIDATION_EVENT_CODE
        }));
        let validation_snapshot = validation_counting.snapshot();
        assert_eq!(validation_snapshot.counters.body_artifact_reads, 0);

        assert_current_fact_route(&observation_snapshot);
        assert_current_fact_route(&validation_snapshot);
    }

    fn assert_current_fact_route(snapshot: &LongitudinalCountingSnapshotV1) {
        assert_eq!(
            snapshot.observed_route_states,
            vec![InteractionObservedRouteStateV1::DerivedCurrent]
        );
        assert_eq!(snapshot.counters.directory_entries_walked, 0);
        assert_eq!(snapshot.counters.strict_journal_inspections, 0);
        assert_eq!(snapshot.counters.change_semantic_constructions, 0);
        assert_eq!(snapshot.counters.change_projection_constructions, 0);
        assert_eq!(snapshot.counters.full_history_fallbacks, 0);
        assert!(
            snapshot
                .derived_access_phases
                .iter()
                .all(|sample| sample.phase != Phase::WorkflowChangeReaderReplayH3)
        );
    }

    #[test]
    fn current_assessment_summary_corruption_is_ignored_until_requested_and_never_falls_back() {
        let (repo, revision_id) = captured_repo();
        let assessment = record_assessment(
            AssessmentAddOptions::new(repo.path())
                .with_exact_revision_id(revision_id.clone())
                .with_track("agent:reviewer")
                .with_assessment(crate::session::event::ReviewAssessment::Accepted)
                .with_summary("corruptible summary ".repeat(1000)),
        )
        .unwrap();
        migrate_to_ready(repo.path());
        publish_current_derived_generation(repo.path());

        let store_dir = crate::git::git_common_dir(repo.path())
            .unwrap()
            .join("pointbreak");
        let assessment_event = crate::session::EventStore::open(&store_dir)
            .list_events()
            .unwrap()
            .into_iter()
            .find(|event| event.event_id == assessment.event_id)
            .unwrap();
        let payload: crate::session::event::ReviewAssessmentRecordedPayload =
            serde_json::from_value(assessment_event.payload).unwrap();
        let artifact_path = store_dir.join(payload.summary_artifact_path.unwrap());
        let mut artifact: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&artifact_path).unwrap()).unwrap();
        artifact["body"] = serde_json::json!("corrupt summary bytes");
        std::fs::write(&artifact_path, serde_json::to_vec(&artifact).unwrap()).unwrap();

        let result_only = AssessmentShowOptions::new(repo.path())
            .with_exact_revision_id(revision_id.clone())
            .with_track("agent:reviewer");
        let counting = LongitudinalCountingScopeV1::new("2".repeat(64)).unwrap();
        counting.record_execution_actor_once(InteractionActorV1::RequestReader);
        let guard = counting.enter();
        let result = show_assessments_with_public_read_context(
            result_only.clone(),
            crate::session::prepare_public_read_command_context_v1(repo.path()).unwrap(),
        )
        .unwrap();
        drop(guard);
        assert!(result.assessments[0].summary.is_none());
        let snapshot = counting.snapshot();
        assert_current_fact_route(&snapshot);
        assert_eq!(snapshot.counters.body_artifact_reads, 0);

        let include_summary = result_only.with_include_summary(true);
        let counting = LongitudinalCountingScopeV1::new("3".repeat(64)).unwrap();
        counting.record_execution_actor_once(InteractionActorV1::RequestReader);
        let guard = counting.enter();
        let derived_error = show_assessments_with_public_read_context(
            include_summary.clone(),
            crate::session::prepare_public_read_command_context_v1(repo.path()).unwrap(),
        )
        .unwrap_err()
        .to_string();
        drop(guard);
        let snapshot = counting.snapshot();
        assert!(
            snapshot
                .derived_access_phases
                .iter()
                .all(|sample| sample.phase != Phase::WorkflowChangeReaderReplayH3)
        );
        assert_eq!(
            snapshot.observed_route_states,
            vec![InteractionObservedRouteStateV1::DerivedSelectionFailedClosed]
        );

        let authoritative_error = show_assessments_with_public_read_context(
            include_summary,
            crate::session::prepare_public_read_command_context_v1(repo.path())
                .unwrap()
                .with_derived_access_profile_for_test(DerivedAccessProfile::Off),
        )
        .unwrap_err()
        .to_string();
        assert_eq!(derived_error, authoritative_error);
        assert!(
            derived_error.contains("content hash mismatch"),
            "{derived_error}"
        );
    }

    #[test]
    fn observation_and_validation_selected_or_support_corruption_never_falls_back() {
        let (validation_repo, validation_revision) = captured_repo();
        let validation = record_validation_check(
            ValidationAddOptions::new(validation_repo.path())
                .with_exact_revision_id(validation_revision.clone())
                .with_track("agent:reviewer")
                .with_check_name("cargo test")
                .with_status(crate::model::ValidationStatus::Passed),
        )
        .unwrap();
        migrate_to_ready(validation_repo.path());
        publish_current_derived_generation(validation_repo.path());
        let validation_context =
            crate::session::prepare_public_read_command_context_v1(validation_repo.path()).unwrap();
        let validation_store_dir = crate::git::git_common_dir(validation_repo.path())
            .unwrap()
            .join("pointbreak");
        let validation_store = crate::session::EventStore::open(&validation_store_dir);
        let validation_event = validation_store
            .list_events()
            .unwrap()
            .into_iter()
            .find(|event| event.event_id == validation.event_id)
            .unwrap();
        let validation_path =
            validation_store.event_path_for_idempotency_key(&validation_event.idempotency_key);
        let validation_counting = LongitudinalCountingScopeV1::new("6".repeat(64)).unwrap();
        validation_counting.record_execution_actor_once(InteractionActorV1::RequestReader);
        let guard = validation_counting.enter();
        let validation_error = validation::list_validation_checks_with_public_read_context_and_fact_hook(
            ValidationListOptions::new(validation_repo.path())
                .with_exact_revision_id(validation_revision)
                .with_track("agent:reviewer"),
            validation_context,
            |boundary| {
                if boundary
                    == crate::session::derived_access::fact_reads::ExactRevisionFactReadBoundary::Selected
                {
                    std::fs::write(
                        &validation_path,
                        br#"{"not":"a valid selected Shore event"}"#,
                    )
                    .unwrap();
                }
            },
        )
        .unwrap_err()
        .to_string();
        drop(guard);
        assert!(!validation_error.is_empty());
        assert_failed_fact_route_never_fell_back(&validation_counting.snapshot());

        let (observation_repo, observation_revision) = captured_repo();
        let observation = record_observation(
            ObservationAddOptions::new(observation_repo.path())
                .with_exact_revision_id(observation_revision.clone())
                .with_track("agent:reviewer")
                .with_title("support corruption")
                .with_body("support body ".repeat(1000)),
        )
        .unwrap();
        remove_content(RemoveOptions::new(
            observation_repo.path(),
            RemoveSelector::Revision(observation_revision.clone()),
        ))
        .unwrap();
        migrate_to_ready(observation_repo.path());
        publish_current_derived_generation(observation_repo.path());
        let observation_context =
            crate::session::prepare_public_read_command_context_v1(observation_repo.path())
                .unwrap();
        let observation_store_dir = crate::git::git_common_dir(observation_repo.path())
            .unwrap()
            .join("pointbreak");
        let observation_store = crate::session::EventStore::open(&observation_store_dir);
        let body_hash = observation.body_content_hash.unwrap();
        let removal_event = observation_store
            .list_events()
            .unwrap()
            .into_iter()
            .find(|event| {
                event.event_type == crate::session::event::EventType::ArtifactRemoved
                    && event.payload["contentHash"].as_str() == Some(&body_hash)
            })
            .unwrap();
        let observation_path =
            observation_store.event_path_for_idempotency_key(&removal_event.idempotency_key);
        let observation_counting = LongitudinalCountingScopeV1::new("7".repeat(64)).unwrap();
        observation_counting.record_execution_actor_once(InteractionActorV1::RequestReader);
        let guard = observation_counting.enter();
        let observation_error = observation::list_observations_with_public_read_context_and_fact_hook(
            ObservationListOptions::new(observation_repo.path())
                .with_exact_revision_id(observation_revision)
                .with_track("agent:reviewer"),
            observation_context,
            |boundary| {
                if boundary
                    == crate::session::derived_access::fact_reads::ExactRevisionFactReadBoundary::SupportPlanned
                {
                    std::fs::write(
                        &observation_path,
                        br#"{"not":"a valid support Shore event"}"#,
                    )
                    .unwrap();
                }
            },
        )
        .unwrap_err()
        .to_string();
        drop(guard);
        assert!(!observation_error.is_empty());
        assert_failed_fact_route_never_fell_back(&observation_counting.snapshot());
    }

    fn assert_failed_fact_route_never_fell_back(snapshot: &LongitudinalCountingSnapshotV1) {
        assert_eq!(
            snapshot.observed_route_states,
            vec![InteractionObservedRouteStateV1::DerivedSelectionFailedClosed]
        );
        assert_eq!(snapshot.counters.directory_entries_walked, 0);
        assert_eq!(snapshot.counters.strict_journal_inspections, 0);
        assert_eq!(snapshot.counters.full_history_fallbacks, 0);
        assert!(
            snapshot
                .derived_access_phases
                .iter()
                .all(|sample| sample.phase != Phase::WorkflowChangeReaderReplayH3)
        );
    }

    #[test]
    fn qualified_consumers_reject_shape_and_repository_misuse_before_projection() {
        let (repo, revision_id) = captured_repo();
        migrate_to_ready(repo.path());
        let other = captured_repo().0;
        let context =
            || crate::session::prepare_public_read_command_context_v1(repo.path()).unwrap();

        assert!(
            show_assessments_with_public_read_context(
                AssessmentShowOptions::new(repo.path())
                    .with_exact_revision_id(revision_id.clone())
                    .with_track("agent:reviewer")
                    .with_all(true),
                context(),
            )
            .is_err()
        );
        assert!(
            list_input_requests_with_public_read_context(
                InputRequestListOptions::new(repo.path())
                    .with_exact_revision_id(revision_id.clone())
                    .with_include_body(true),
                context(),
            )
            .is_err()
        );
        assert!(
            list_observations_with_public_read_context(
                ObservationListOptions::new(repo.path())
                    .with_exact_revision_id(revision_id.clone())
                    .with_track("agent:reviewer")
                    .with_file("src/lib.rs"),
                context(),
            )
            .is_err()
        );
        assert!(
            list_validation_checks_with_public_read_context(
                ValidationListOptions::new(repo.path())
                    .with_exact_revision_id(revision_id.clone())
                    .with_track("agent:reviewer")
                    .with_status(crate::model::ValidationStatus::Passed),
                context(),
            )
            .is_err()
        );
        assert!(
            show_assessments_with_public_read_context(
                AssessmentShowOptions::new(other.path())
                    .with_exact_revision_id(revision_id)
                    .with_track("agent:reviewer"),
                context(),
            )
            .is_err()
        );
    }

    #[test]
    fn unrelated_unknown_control_reaches_only_the_strict_consumer_and_never_falls_back() {
        let (repo, revision_id) = captured_repo();
        migrate_to_ready(repo.path());
        let store =
            crate::session::store::resolution::resolve_change_read_backend(repo.path()).unwrap();
        store
            .backend()
            .journal()
            .create_record_once(
                "unrelated-future-control",
                br#"{"schema":"pointbreak.future-control","version":1}"#,
            )
            .unwrap();
        let context = crate::session::prepare_public_read_command_context_v1(repo.path()).unwrap();
        let counting = LongitudinalCountingScopeV1::new("c".repeat(64)).unwrap();
        counting.record_execution_actor_once(InteractionActorV1::RequestReader);
        let _guard = counting.enter();

        let error = show_assessments_with_public_read_context(
            AssessmentShowOptions::new(repo.path())
                .with_exact_revision_id(revision_id)
                .with_track("agent:reviewer"),
            context,
        )
        .unwrap_err()
        .to_string();
        let snapshot = counting.snapshot();

        assert!(error.contains("unknown Journal record schema"), "{error}");
        assert_eq!(snapshot.counters.event_decodes, 0);
        assert_eq!(snapshot.counters.projection_rebuilds, 0);
        assert!(snapshot.observed_route_states.is_empty());
        assert_eq!(
            snapshot
                .derived_access_phases
                .iter()
                .map(|sample| sample.phase)
                .collect::<Vec<_>>(),
            vec![Phase::WorkflowChangeReaderReplayH3]
        );
    }

    fn observe<T>(run: impl FnOnce() -> crate::error::Result<T>) -> LongitudinalCountingSnapshotV1 {
        let counting = LongitudinalCountingScopeV1::new("b".repeat(64)).unwrap();
        counting.record_execution_actor_once(InteractionActorV1::RequestReader);
        let _guard = counting.enter();
        run().unwrap();
        counting.snapshot()
    }

    fn migrate_to_ready(repo: &Path) {
        let dry_run = dry_run_bulk_adoption(BulkAdoptionDryRunOptions::new(repo)).unwrap();
        let backup = tempfile::tempdir().unwrap();
        migrate_bulk_adoption(
            BulkAdoptionMigrationOptions::new(
                repo,
                dry_run.clone(),
                dry_run.manifest_hash.clone(),
                dry_run.roots[0].cohort_manifest_hash.clone().unwrap(),
                backup.path().join("task-3-1-backup"),
                "task-3-1-attribution-test",
            )
            .with_minimum_reader_ack(REVIEW_CHANGE_REVISION_COHORT_V1)
            .with_legacy_reader_unsupported_ack()
            .sign_with(TestEd25519Signer::from_seed([0x31; 32]))
            .with_fixed_occurred_at("2026-08-21T20:45:00Z")
            .with_derived_enabled(false),
        )
        .unwrap();
    }

    fn publish_current_derived_generation(repo: &Path) -> DerivedAccessLifecycle {
        let store_dir = crate::git::git_common_dir(repo).unwrap().join("pointbreak");
        let store_identity =
            crate::session::store::resolution::opaque_path_identity("store", &store_dir).unwrap();
        let lifecycle = DerivedAccessLifecycle::new(
            DerivedAccessProfile::SqliteWalBodylessV1,
            &store_dir,
            store_identity,
        )
        .unwrap();
        lifecycle.rebuild(|_| LifecycleControl::Continue).unwrap();
        lifecycle
    }

    fn append_semantic_duplicate(
        repo: &Path,
        lifecycle: &DerivedAccessLifecycle,
        source_event_id: &crate::model::EventId,
        idempotency_key: &str,
    ) {
        use crate::session::EventStore;
        use crate::session::derived_access::writer::DerivedWriteCoordinator;
        use crate::session::event::{
            EventType, InputRequestOpenedPayload, ReviewAssessmentRecordedPayload,
            ReviewObservationRecordedPayload, ShoreEvent, ValidationCheckRecordedPayload,
        };

        let read_store =
            crate::session::store::resolution::resolve_change_read_backend(repo).unwrap();
        let original = EventStore::from_backend(read_store.backend())
            .list_events()
            .unwrap()
            .into_iter()
            .find(|event| &event.event_id == source_event_id)
            .unwrap();
        let mut duplicate = match original.event_type {
            EventType::ReviewAssessmentRecorded => ShoreEvent::new(
                original.event_type,
                idempotency_key,
                original.target.clone(),
                original.writer.clone(),
                serde_json::from_value::<ReviewAssessmentRecordedPayload>(original.payload.clone())
                    .unwrap(),
                original.occurred_at.clone(),
            )
            .unwrap(),
            EventType::InputRequestOpened => ShoreEvent::new(
                original.event_type,
                idempotency_key,
                original.target.clone(),
                original.writer.clone(),
                serde_json::from_value::<InputRequestOpenedPayload>(original.payload.clone())
                    .unwrap(),
                original.occurred_at.clone(),
            )
            .unwrap(),
            EventType::ReviewObservationRecorded => ShoreEvent::new(
                original.event_type,
                idempotency_key,
                original.target.clone(),
                original.writer.clone(),
                serde_json::from_value::<ReviewObservationRecordedPayload>(
                    original.payload.clone(),
                )
                .unwrap(),
                original.occurred_at.clone(),
            )
            .unwrap(),
            EventType::ValidationCheckRecorded => ShoreEvent::new(
                original.event_type,
                idempotency_key,
                original.target.clone(),
                original.writer.clone(),
                serde_json::from_value::<ValidationCheckRecordedPayload>(original.payload.clone())
                    .unwrap(),
                original.occurred_at.clone(),
            )
            .unwrap(),
            other => panic!("unsupported semantic duplicate family: {other:?}"),
        };
        duplicate.assertion_mode = original.assertion_mode;
        let store = EventStore::from_backend(read_store.backend())
            .with_coordinator(DerivedWriteCoordinator::new(lifecycle.clone()).unwrap());
        store.record_event_once(&duplicate).unwrap();
    }

    fn captured_repo() -> (tempfile::TempDir, crate::model::RevisionId) {
        let repo = tempfile::tempdir().unwrap();
        git(repo.path(), &["init", "--quiet"]);
        git(repo.path(), &["config", "user.name", "Pointbreak Test"]);
        git(
            repo.path(),
            &["config", "user.email", "pointbreak@example.test"],
        );
        std::fs::write(repo.path().join("file.txt"), "one\n").unwrap();
        git(repo.path(), &["add", "file.txt"]);
        git(repo.path(), &["commit", "--quiet", "-m", "base"]);
        std::fs::write(repo.path().join("file.txt"), "two\n").unwrap();
        let capture = capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();
        (repo, capture.revision_id)
    }

    fn git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
