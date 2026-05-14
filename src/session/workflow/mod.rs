mod capture;
mod history;
mod import;
mod reload;
mod review_unit_projection;

pub(in crate::session) mod disposition;
pub(in crate::session) mod intervention;
pub(in crate::session) mod observation;

pub use capture::{CaptureOptions, CaptureResult, capture_worktree_review};
pub use disposition::{
    CurrentDispositionStatus, CurrentDispositionView, DispositionAddOptions, DispositionAddResult,
    DispositionOverrideSelector, DispositionRecordStatus, DispositionShowFilters,
    DispositionShowOptions, DispositionShowResult, DispositionTargetSelector, DispositionView,
    record_disposition, show_dispositions,
};
pub use history::{
    ReviewHistoryEntry, ReviewHistoryFilters, ReviewHistoryOptions, ReviewHistoryResult,
    ReviewHistorySummary, review_history,
};
pub use import::{ImportNotesOptions, ImportNotesResult, import_notes};
pub use intervention::{
    InterventionFetchOptions, InterventionFetchResult, InterventionListFilters,
    InterventionListOptions, InterventionListResult, InterventionRequestOptions,
    InterventionRequestResult, InterventionResolutionView, InterventionResolveOptions,
    InterventionResolveResult, InterventionStatus, InterventionStatusFilter,
    InterventionTargetSelector, InterventionView, fetch_intervention, list_interventions,
    request_intervention, resolve_intervention,
};
pub use observation::{
    ObservationAddOptions, ObservationAddResult, ObservationListFilters, ObservationListOptions,
    ObservationListResult, ObservationStatus, ObservationTargetSelector, ObservationView,
    list_observations, record_observation,
};
pub(crate) use reload::reload_diagnostics_for_document;
pub use reload::{ReloadDiagnostic, ReloadDiagnosticCode, ReloadOutcome, reload_session};
pub use review_unit_projection::{
    AdapterNoteView, ReviewUnitProjectionIdentity, ReviewUnitProjectionRow,
    ReviewUnitProjectionSummary, ReviewUnitShowFilters, ReviewUnitShowOptions,
    ReviewUnitShowResult, SnapshotOrder, show_review_unit,
};
