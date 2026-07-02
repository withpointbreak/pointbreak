use serde::Serialize;

use super::options::ReviewHistoryFilters;
use super::summary::ReviewHistoryEntry;
use crate::session::state::ProjectionDiagnostic;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewHistoryResult {
    pub event_set_hash: String,
    pub event_count: usize,
    pub filters: ReviewHistoryFilters,
    pub entries: Vec<ReviewHistoryEntry>,
    /// An opaque continuation token for the next page when a window was applied
    /// and entries remain after it; `null` for an unwindowed or final page.
    pub next_cursor: Option<String>,
    /// Read/skip diagnostics describe the full replayed event set, not only
    /// filtered entries. Body-content removal diagnostics are the exception:
    /// they describe the rendered entries only — state resolution happens only
    /// for entries that survive filtering/windowing, so a removed body outside
    /// the window yields no diagnostic on that page.
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

impl ReviewHistoryResult {
    pub fn history_count(&self) -> usize {
        self.entries.len()
    }
}
