// Document builder for `pointbreak review-history`.
use crate::documents::DiagnosticDocument;
use crate::session::{ReviewHistoryEntry, ReviewHistoryFilters, ReviewHistoryResult};

/// Documented body for `pointbreak.review-history`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    event_set_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    projection_stamp: Option<String>,
    event_count: usize,
    history_count: usize,
    filters: ReviewHistoryFilters,
    entries: Vec<ReviewHistoryEntry>,
    /// Opaque continuation token for the next page when a window was applied and
    /// entries remain; `null` for an unwindowed or final read. Additive.
    next_cursor: Option<String>,
}

/// Build the `pointbreak.review-history` document from a history result.
pub fn history_document(result: ReviewHistoryResult) -> DiagnosticDocument<HistoryBody> {
    history_document_with_identity(result, None)
}

/// Build the same history document from a validated derived projection. The
/// projection stamp is a freshness/version identity, not a full-set hash, so
/// the two fields are mutually exclusive on the wire.
#[doc(hidden)]
pub fn derived_history_document(
    result: ReviewHistoryResult,
    projection_stamp: String,
) -> DiagnosticDocument<HistoryBody> {
    history_document_with_identity(result, Some(projection_stamp))
}

fn history_document_with_identity(
    result: ReviewHistoryResult,
    projection_stamp: Option<String>,
) -> DiagnosticDocument<HistoryBody> {
    let history_count = result.history_count();
    let event_set_hash = projection_stamp.is_none().then_some(result.event_set_hash);
    DiagnosticDocument::new(
        "pointbreak.review-history",
        HistoryBody {
            event_set_hash,
            projection_stamp,
            event_count: result.event_count,
            history_count,
            filters: result.filters,
            entries: result.entries,
            next_cursor: result.next_cursor,
        },
        result.diagnostics,
    )
}
