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
    if crate::session::activated_store_capability_for_repo(repo)?.is_some() {
        let state = change_reader_state_for_repo(repo)?;
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
        let (store, _) = crate::session::store::resolution::resolve_change_read_store(repo)?;
        Ok((store, events))
    } else {
        let store = crate::session::store::resolution::resolve_read_store(repo)?;
        let events = crate::session::EventStore::from_backend(store.backend()).list_events()?;
        Ok((store, events))
    }
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
    list_attention,
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
pub use change_read::{ChangeReaderReadyV1, ChangeReaderStateV1, change_reader_state_for_repo};
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
    BaseEntry, BaseHistoryProjection, BaseProjectionConfig, DistinctValues, EVENT_QUERY_FIELDS,
    EventRecordExtras, HistoryCursor, HistoryOrder, HistoryPage, HistoryQuery, KNOWN_QUERY_KEYS,
    ParsedQuery, QueriedHistory, QueryClause, QueryDiagnostic, QueryDiagnosticCode, QuerySurface,
    RANGE_ANCHOR_FIELD, REVISION_ATTENTION_VALUES, REVISION_QUERY_FIELDS, ReviewHistoryEntry,
    ReviewHistoryFilters, ReviewHistoryOptions, ReviewHistoryResult, ReviewHistorySummary,
    SearchRecord, apply_history_query, build_haystack, count_new_since,
    default_history_page_projection, history_base_projection, matches_query, parse_search_query,
    parse_search_query_for, redact_history_bodies, review_history,
};
pub(crate) use history::{history_entries_from_selected_events, tag_completion_key};
#[cfg(any(test, feature = "bench"))]
pub(in crate::session) use ingest::ingest_events_with_clock;
pub use ingest::{
    ImportEventOptions, IngestEventsOptions, IngestEventsResult, import_event, ingest_events,
};
#[cfg(any(test, feature = "bench"))]
pub(crate) use ingest::{carrier_target_full_scan_count, reset_carrier_target_full_scan_count};
pub use input_request::{
    InputRequestFetchOptions, InputRequestFetchResult, InputRequestListOptions,
    InputRequestListResult, InputRequestOpenOptions, InputRequestOpenResult,
    InputRequestRespondOptions, InputRequestRespondResult, InputRequestResponseView,
    InputRequestStatus, InputRequestStatusFilter, InputRequestTargetSelector, InputRequestView,
    fetch_input_request, list_input_requests, open_input_request, respond_input_request,
};
pub use landing::{LandCommitOptions, LandCommitResultV1, land_commit};
pub use observation::{
    ObservationAddOptions, ObservationAddResult, ObservationListOptions, ObservationListResult,
    ObservationStatus, ObservationTargetSelector, ObservationView, list_observations,
    record_observation, validated_track_id,
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
    ValidationListOptions, ValidationListResult, list_validation_checks, record_validation_check,
};
pub(crate) use validation::{
    ValidationCheckProjectionOptions, annotate_validation_supersession, project_validation_checks,
};
