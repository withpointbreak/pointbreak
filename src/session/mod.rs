pub mod adapter;
#[cfg(any(test, feature = "bench"))]
pub(crate) mod benchmark;
pub(crate) mod derived_access;
pub mod event;
pub mod evidence;
mod identity;
mod projection;
mod sensitivity_vocabulary;
mod signing;
mod store;
pub mod transfer;
mod workflow;

#[doc(hidden)]
pub use derived_access::attention::{DerivedAttention, DerivedAttentionRoute};
#[doc(hidden)]
pub use derived_access::history::{
    DerivedHistoryAccess, DerivedHistoryAvailability, DerivedHistoryConflictPaths,
    DerivedHistoryControl, DerivedHistoryFreshness, DerivedHistoryLifecycleReceipt,
    DerivedHistoryLifecycleStatus, DerivedHistoryNamespace, DerivedHistoryNewCount,
    DerivedHistoryPage, DerivedHistoryProgress, DerivedHistoryProgressPhase, DerivedHistoryRoute,
    DerivedHistoryStatus, DerivedHistoryTransition,
};
#[doc(hidden)]
pub use derived_access::revisions::{
    ACTIVE_REVISION_PAGE_PROFILE, AUTHORITATIVE_REVISION_PAGE_PROFILE, DerivedRevisionDetail,
    DerivedRevisionDetailRoute, DerivedRevisionPage, DerivedRevisionPageRoute,
    REVISION_PAGE_DEFAULT_LIMIT, REVISION_PAGE_MAXIMUM_LIMIT, REVISION_PAGE_SCHEMA,
    RevisionPageCursor, RevisionPageRequest, RevisionPageRequestError, RevisionPageWork,
};
#[doc(hidden)]
pub use derived_access::threads::{DerivedThreads, DerivedThreadsRoute};

/// Drain process-local, user-actionable diagnostics produced while coordinating
/// disposable derived state around authoritative writes.
#[doc(hidden)]
pub fn take_derived_write_diagnostics() -> Vec<ProjectionDiagnostic> {
    derived_access::writer::take_process_diagnostics()
        .into_iter()
        .map(|diagnostic| ProjectionDiagnostic {
            code: diagnostic.code.to_owned(),
            message: diagnostic.message,
        })
        .collect()
}
pub use event::{
    BodyContentType, IngestProvenance, IngestVia, event_signature_pre_authentication_encoding,
    event_to_be_signed,
};
pub use identity::{
    ActorAttributes, ActorAttributesMap, ActorAttributesStageOutcome, ActorAttributesWriteRecord,
    DelegationMap, DelegationRecord, DelegationStageOutcome, DelegationWriteRecord,
    PrincipalResolution, PrincipalSource, PrincipalStatus, PrincipalView, UnresolvedReason,
    actor_attributes_from_value, compare_event_instants, delegation_map_from_value,
    format_rfc3339_utc_millis, is_agent_actor_id, is_valid_actor_id, now_rfc3339_utc,
    parse_event_instant, principal_display_label, principal_resolution_for_writer,
    principal_view_for, resolve_writer_actor_id, stage_actor_attributes, stage_delegation,
};
pub(crate) use identity::{IngestClock, SystemIngestClock, current_timestamp, writer_from_options};
pub use projection::cosignature::{
    EndorsementClassification, EndorsementReadback, EndorserAttributesView,
};
pub(crate) use projection::state;
pub use projection::{
    ArtifactRemovalProjection, BodyContentState, ChangeClaimSupportV1, ChangeDocumentProjectionV1,
    ChangeLifecycleV1, ChangeLinkView, ChangeMembershipClaimViewV1, ChangeProjection,
    ChangeRelationClaimViewV1, ChangeTopologyV1, ChangeView, CommitEdgeSource,
    CommitOidGroupingProjection, ContentAvailabilityV1, CurrentCommitAssociation,
    CurrentRefAssociation, EngagementGrouping, EngagementLifecycle, EngagementView, LivenessScope,
    LivenessToken, ProjectionDiagnostic, RemovalClaim, RemovalOperativeStatus,
    RevisionClassificationFacet, RevisionCommitRangeProjection, RevisionCommitRangeView,
    RevisionRefUnavailableReasonV1, RevisionsByBase, SessionState, StoreIdIndex, SupersessionView,
    WithdrawnCommitAssociation, WithdrawnRefAssociation, change_document_projection_stamp,
    project_change_documents, project_changes, read_events, read_events_for_display, rebuild_state,
    revision_supersession_classification, store_id_index,
};
pub use sensitivity_vocabulary::{SensitivityKind, SensitivityPolicyOutcome, SensitivitySeverity};
pub use signing::{
    ArtifactAvailability, BestEffortSkipSink, COSIGNATURE_BINDING_MISMATCH_CODE,
    COSIGNATURE_INVALID_CODE, COSIGNATURE_TARGET_PENDING_CODE, COSIGNATURE_UNTRUSTED_SIGNER_CODE,
    CosignatureGateDecision, CosignatureVerification, EnrollmentDiff, EventSigningOptions,
    EventVerificationPolicy, EventVerificationView, IngestEventVerification, PrincipalPolicy,
    RemovalPolicy, TrustSet, allowed_signers_path_for_repo, enroll_signer,
    event_signature_trust_set, gate_cosignature_for_store, principal_sufficient, stage_enrollment,
    trust_set_to_value, verification_view, verify_cosignature, verify_event_signature,
};
pub(crate) use signing::{sign_event_if_requested, verify_events_for_ingest};
#[cfg(any(test, feature = "bench"))]
pub(crate) use store::backend::{
    Journal, JournalChangeCheck, JournalChangeStamp, JournalChangeVerdict, LocalJournal,
};
#[cfg(test)]
pub(crate) use store::capabilities::AUTHORITY_CURSOR_SCHEMA_V2;
#[cfg(test)]
pub(crate) use store::compute_revision_fingerprint;
#[cfg(feature = "longitudinal-counting")]
pub(crate) use store::resolution::opaque_path_identity;
pub use store::{
    AuthorityCursorV2, EventWriteOutcome, ObjectArtifact, StoreCapabilityInspection,
    StoreCapabilityStatus, StoreMode, StoreModeOutcome, StoreModeSource, StorePaths,
    activated_store_capability_for_repo, capture_worktree_fingerprint,
    change_reader_head_marker_for_repo, ensure_pointbreak_gitignore, event_log_head_marker,
    family_link_advisory, read_bound_object_artifact, read_object_artifact,
    resolve_store_mode_for_repo, set_store_mode_for_repo, store_capability_for_repo,
    store_dir_for_repo, store_paths_for_repo,
};
pub(crate) use store::{
    EventStore, RepositoryPaths, RevisionFingerprint, SkippedEvent, build_object_artifact_v2,
    sweep_stale_temp_files, worktree_fingerprint_for_files,
};
pub(in crate::session) use store::{body_artifact, fingerprint, object_artifact, store_init};
pub use workflow::{
    ArtifactKind, ArtifactRef, AssessmentAddOptions, AssessmentAddResult, AssessmentRecordStatus,
    AssessmentShowFilters, AssessmentShowOptions, AssessmentShowResult, AssessmentTargetSelector,
    AssessmentView, AssociateCommitOptions, AssociateCommitResult, AssociateRefOptions,
    AssociateRefResult, AssociationAxis, AttentionAssessmentRecord, AttentionDetail,
    AttentionFreshness, AttentionFreshnessState, AttentionItem, AttentionListOptions,
    AttentionListResult, AttentionProjection, AttentionTier, BULK_ADOPTION_BACKUP_MANIFEST_FILE_V1,
    BULK_ADOPTION_BACKUP_RECEIPT_FILE_V1, BULK_ADOPTION_BACKUP_RESTORE_RECEIPT_SCHEMA_V1,
    BULK_ADOPTION_DRY_RUN_SCHEMA_V1, BULK_ADOPTION_EXECUTION_PLAN_FILE_V1,
    BULK_ADOPTION_MIGRATION_RECEIPT_SCHEMA_V1, BULK_ADOPTION_MINIMUM_READER_PROFILE_V1,
    BULK_ADOPTION_OWNER_DECISIONS_SCHEMA_V1, BaseEntry, BaseHistoryProjection,
    BaseProjectionConfig, BulkAdoptionBackupRestoreDispositionV1,
    BulkAdoptionBackupRestoreReceiptV1, BulkAdoptionDryRunAnomalyV1, BulkAdoptionDryRunChangeV1,
    BulkAdoptionDryRunDocumentV1, BulkAdoptionDryRunOptions, BulkAdoptionDryRunRootV1,
    BulkAdoptionMigrationDispositionV1, BulkAdoptionMigrationOptions,
    BulkAdoptionMigrationReceiptV1, BulkAdoptionOverlapIdentityDecisionV1,
    BulkAdoptionOwnerDecisionManifestV1, BulkAdoptionRetainedAllocationV1,
    BulkAdoptionRetainedManifestV1, CHANGE_OPERATION_SCHEMA_V1, CHANGE_TIMELINE_QUERY_FIELDS,
    CaptureDiffstat, CaptureOptions, CaptureResult, ChangeAdvanceV1, ChangeCaptureOptions,
    ChangeCaptureReceiptV1, ChangeCaptureRevisionV1, ChangeCreateOptions, ChangeLinkOptions,
    ChangeMembershipOptions, ChangeMembershipWithdrawalOptions, ChangeOperationEventOutcomeV1,
    ChangeOperationEventReceiptV1, ChangeOperationReceiptV1, ChangeReaderPresentationV1,
    ChangeReaderReadyV1, ChangeReaderStateV1, ChangeRelationOptions,
    ChangeRelationWithdrawalOptions, CommitGraphCondition, CommitLiveness, CommitProofStateV1,
    CommitRangeSpec, CommitSourceStateV1, CompactOptions, CompactResult, CurrentAssessmentStatus,
    CurrentAssessmentView, DistinctValues, EVENT_QUERY_FIELDS, EventRecordExtras,
    EventSignatureRecordOptions, EventSignatureRecordResult, FactPortOptions, FactPortResultV1,
    HistoryCursor, HistoryOrder, HistoryPage, HistoryQuery, ImportArtifactOptions,
    ImportArtifactOutcome, ImportArtifactResult, ImportEventOptions, IngestEventsOptions,
    IngestEventsResult, InputRequestFetchOptions, InputRequestFetchResult, InputRequestListOptions,
    InputRequestListResult, InputRequestOpenOptions, InputRequestOpenResult,
    InputRequestRespondOptions, InputRequestRespondResult, InputRequestResponseView,
    InputRequestStatus, InputRequestStatusFilter, InputRequestTargetSelector, InputRequestView,
    KNOWN_QUERY_KEYS, LandCommitOptions, LandCommitResultV1, ListAssociationsOptions,
    ListAssociationsResult, LivenessEnrichment, MemberReadback, MigrateToCommonDirOptions,
    MigrateToCommonDirResult, ObservationAddOptions, ObservationAddResult, ObservationListOptions,
    ObservationListResult, ObservationStatus, ObservationTargetSelector, ObservationView,
    ParsedQuery, QueriedHistory, QueryClause, QueryDiagnostic, QueryDiagnosticCode, QuerySurface,
    RANGE_ANCHOR_FIELD, REF_REWRITTEN_CODE, REVIEW_CURSOR_SCHEMA_V1, REVISION_ATTENTION_VALUES,
    REVISION_QUERY_FIELDS, RefContinuity, RefContinuityReport, RefContinuityView, RefFilterMode,
    RemoveOptions, RemoveResult, RemoveSelector, RemovedContent, Retention, ReviewCursorRefusalV1,
    ReviewCursorSelectionV1, ReviewCursorV1, ReviewHistoryEntry, ReviewHistoryFilters,
    ReviewHistoryOptions, ReviewHistoryResult, ReviewHistorySummary, ReviewSourceBindingV1,
    ReviewSourceFingerprintV1, ReviewSourcePathStateV1, ReviewSourceRequestV1, RevisionListEntry,
    RevisionListOptions, RevisionListResult, RevisionOverview, RevisionOverviewsOptions,
    RevisionProjectionIdentity, RevisionProjectionRow, RevisionProjectionSummary,
    RevisionRecordInputs, RevisionSearchRecord, RevisionShowFilters, RevisionShowOptions,
    RevisionShowResult, RootCommitSpec, SearchRecord, SkippedRemoval, SnapshotContentState,
    SnapshotOrder, SnapshotSummaryCache, SnapshotSummaryCounts, StagedSpec, StoreFamily,
    StoreForgetOptions, StoreForgetResult, StoreIdentity, StoreIdentityOptions, StoreLinkOptions,
    StoreLinkPreview, StoreLinkResult, StoreListEntry, StoreListResult, StorePlacement,
    StoreSensitivityPathGroup, StoreStatusArtifactInventory, StoreStatusInventory,
    StoreStatusOptions, StoreStatusResult, StoreStatusRevisionObject, StoreStatusSensitivity,
    StoreStatusSensitivityExcludeGlob, StoreStatusSensitivityFinding, StoreUnlinkOptions,
    StoreUnlinkResult, SweepOutcome, SweptBlob, UnreachableVisibility, UnstagedSpec,
    ValidationAddOptions, ValidationAddResult, ValidationCheckDisposition, ValidationCheckView,
    ValidationContinuitySummary, ValidationContinuityView, ValidationListFilters,
    ValidationListOptions, ValidationListResult, WithdrawCommitOptions, WithdrawCommitResult,
    WithdrawRefOptions, WithdrawRefResult, WorktreeSourceStateV1, WorktreeSpec,
    apply_history_query, assert_change_revision_relation, associate_commit, associate_ref,
    build_haystack, build_revision_search_record, capture_change_revision, capture_review,
    capture_worktree_review, change_graph_token, change_reader_state_for_repo,
    classify_validation_continuity, commit_graph_stamp, compact_store, count_new_since,
    create_change, current_assessment_includes_follow_up, default_history_page_projection,
    diagnose_ref_continuity, diffstat_from_files, dry_run_bulk_adoption, effective_integration_ref,
    enrich_liveness, explain_store_sensitivity, export_artifact, fetch_input_request,
    forget_family_store, history_base_projection, import_artifact, import_event, ingest_events,
    join_revision_to_change, land_commit, link_changes, link_store_to_family, list_associations,
    list_attention, list_family_stores, list_input_requests, list_observations, list_revisions,
    list_units_for_ref, list_validation_checks, matches_query, migrate_bulk_adoption,
    migrate_store_to_common_dir, open_input_request, parse_search_query, parse_search_query_for,
    port_review_fact, preview_link_to_family, record_assessment, record_event_signature,
    record_observation, record_validation_check, redact_history_bodies, referenced_artifacts,
    remove_content, resolve_default_integration_ref, respond_input_request,
    restore_bulk_adoption_backup, review_history, review_source_binding, select_review_cursor,
    show_assessments, show_revision, show_revision_for_change_reader,
    show_revision_for_change_reader_ready, show_revision_for_inspector, show_revision_overviews,
    stale_review_fact_count, store_identity, store_status, unlink_store_from_family,
    validate_review_cursor_for_write, validated_track_id, withdraw_change_revision_relation,
    withdraw_commit, withdraw_ref, withdraw_revision_from_change,
};
pub(in crate::session) use workflow::{assessment, input_request, observation};
#[cfg(any(test, feature = "bench"))]
pub(crate) use workflow::{carrier_target_full_scan_count, reset_carrier_target_full_scan_count};

pub use crate::crypto::EventVerificationStatus;
