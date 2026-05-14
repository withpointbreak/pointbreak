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
    /// Diagnostics describe the full replayed event set, not only filtered entries.
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

impl ReviewHistoryResult {
    pub fn history_count(&self) -> usize {
        self.entries.len()
    }
}
