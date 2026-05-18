use serde::{Deserialize, Serialize};

use super::kind::EventType;
use super::payload::EventPayload;
use crate::model::{
    ReviewEndpoint, ReviewUnitId, ReviewUnitSource, RevisionId, SessionId, Side, SnapshotId,
    WorkUnitId,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewInitializedPayload {}

impl ReviewInitializedPayload {
    pub fn idempotency_key(session_id: &SessionId, work_unit_id: &WorkUnitId) -> String {
        format!(
            "review_initialized:{}:{}",
            session_id.as_str(),
            work_unit_id.as_str()
        )
    }
}

impl EventPayload for ReviewInitializedPayload {
    fn event_type(&self) -> EventType {
        EventType::ReviewInitialized
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewUnitCapturedPayload {
    pub review_unit_id: ReviewUnitId,
    pub source: ReviewUnitSource,
    pub base: ReviewEndpoint,
    pub target: ReviewEndpoint,
    pub revision_id: RevisionId,
    pub snapshot_id: SnapshotId,
    pub snapshot_artifact_content_hash: String,
}

impl EventPayload for ReviewUnitCapturedPayload {
    fn event_type(&self) -> EventType {
        EventType::ReviewUnitCaptured
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidecarSource {
    ReviewNotes,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewNoteImportedPayload {
    pub sidecar_source: SidecarSource,
    pub note_id: String,
    pub file_path: String,
    pub file_old_path: Option<String>,
    pub target: Option<ImportedNoteTarget>,
    pub title: String,
    pub body: Option<String>,
    pub body_artifact_path: Option<String>,
    pub body_byte_size: Option<usize>,
    pub tags: Vec<String>,
    pub confidence: Option<String>,
    pub external_source: Option<String>,
    pub author: Option<String>,
    pub created_at: Option<String>,
    pub sidecar_content_hash: String,
}

impl EventPayload for ReviewNoteImportedPayload {
    fn event_type(&self) -> EventType {
        EventType::ReviewNoteImported
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedNoteTarget {
    pub side: Side,
    pub start_line: u32,
    pub end_line: u32,
}
