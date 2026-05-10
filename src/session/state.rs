use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::error::{Result, ShoreError};
use crate::model::{ReviewId, RevisionId, SnapshotId, WorkUnitId};
use crate::session::event::{
    EventType, RevisionPublishedPayload, ShoreEvent, SnapshotObservedPayload,
};

const STATE_SCHEMA: &str = "shore.state";
const STATE_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionState {
    pub schema: String,
    pub version: u32,
    pub review_id: ReviewId,
    pub work_unit_id: WorkUnitId,
    pub current_revision_id: Option<RevisionId>,
    pub current_snapshot_id: Option<SnapshotId>,
    pub event_count: usize,
    pub sidecar_count: usize,
    pub note_count: usize,
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

impl SessionState {
    pub fn from_events(events: &[ShoreEvent]) -> Result<Self> {
        let mut reducer = StateReducer::default();
        for event in events {
            reducer.apply(event)?;
        }
        reducer.finish(events.len())
    }

    pub fn validate_schema_version(&self) -> Result<()> {
        if self.schema == STATE_SCHEMA && self.version == STATE_VERSION {
            return Ok(());
        }

        Err(ShoreError::UnsupportedStateSchemaVersion {
            schema: self.schema.clone(),
            version: self.version,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionDiagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Debug)]
struct StateReducer {
    review_id: ReviewId,
    work_unit_id: WorkUnitId,
    published_revision_ids: BTreeSet<RevisionId>,
    superseded_revision_ids: BTreeSet<RevisionId>,
    snapshots_by_revision_id: BTreeMap<RevisionId, SnapshotId>,
    sidecar_count: usize,
    note_count: usize,
}

impl Default for StateReducer {
    fn default() -> Self {
        Self {
            review_id: ReviewId::new("review:default"),
            work_unit_id: WorkUnitId::new("work:default"),
            published_revision_ids: BTreeSet::new(),
            superseded_revision_ids: BTreeSet::new(),
            snapshots_by_revision_id: BTreeMap::new(),
            sidecar_count: 0,
            note_count: 0,
        }
    }
}

impl StateReducer {
    fn apply(&mut self, event: &ShoreEvent) -> Result<()> {
        event.validate_schema_version()?;

        if event.event_type == EventType::ReviewInitialized {
            self.review_id = event.target.review_id.clone();
            self.work_unit_id = event.target.work_unit_id.clone();
            return Ok(());
        }

        self.set_identity_from_event_if_default(event);

        match event.event_type {
            EventType::ReviewInitialized => {}
            EventType::RevisionPublished => self.apply_revision_published(event)?,
            EventType::SnapshotObserved => self.apply_snapshot_observed(event)?,
            EventType::SidecarObserved => {
                self.sidecar_count += 1;
            }
            EventType::ReviewNoteImported => {
                self.note_count += 1;
            }
        }

        Ok(())
    }

    fn set_identity_from_event_if_default(&mut self, event: &ShoreEvent) {
        if self.review_id.as_str() == "review:default" {
            self.review_id = event.target.review_id.clone();
        }
        if self.work_unit_id.as_str() == "work:default" {
            self.work_unit_id = event.target.work_unit_id.clone();
        }
    }

    fn apply_revision_published(&mut self, event: &ShoreEvent) -> Result<()> {
        let payload: RevisionPublishedPayload = serde_json::from_value(event.payload.clone())?;
        self.published_revision_ids.insert(payload.revision_id);
        for revision_id in payload.supersedes_revision_ids {
            self.superseded_revision_ids.insert(revision_id);
        }
        Ok(())
    }

    fn apply_snapshot_observed(&mut self, event: &ShoreEvent) -> Result<()> {
        let payload: SnapshotObservedPayload = serde_json::from_value(event.payload.clone())?;
        self.snapshots_by_revision_id
            .insert(payload.revision_id, payload.snapshot_id);
        Ok(())
    }

    fn finish(self, event_count: usize) -> Result<SessionState> {
        let mut diagnostics = Vec::new();
        let unsuperseded_revision_ids = self
            .published_revision_ids
            .difference(&self.superseded_revision_ids)
            .cloned()
            .collect::<BTreeSet<_>>();
        let current_revision_id = match unsuperseded_revision_ids.len() {
            0 => None,
            1 => unsuperseded_revision_ids.iter().next().cloned(),
            _ => {
                diagnostics.push(ProjectionDiagnostic {
                    code: "ambiguous_current_revision".to_owned(),
                    message: "multiple unsuperseded revisions remain current".to_owned(),
                });
                None
            }
        };
        let current_snapshot_id = current_revision_id
            .as_ref()
            .and_then(|revision_id| self.snapshots_by_revision_id.get(revision_id))
            .cloned();

        Ok(SessionState {
            schema: STATE_SCHEMA.to_owned(),
            version: STATE_VERSION,
            review_id: self.review_id,
            work_unit_id: self.work_unit_id,
            current_revision_id,
            current_snapshot_id,
            event_count,
            sidecar_count: self.sidecar_count,
            note_count: self.note_count,
            diagnostics,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ReviewId, RevisionId, Side, SnapshotId, WorkUnitId};
    use crate::session::event::{ImportedNoteTarget, ReviewNoteImportedPayload};
    use crate::session::{
        EventTarget, EventType, ReviewInitializedPayload, RevisionPublishedPayload, ShoreEvent,
        SidecarObservedPayload, SidecarSource, SnapshotObservedPayload, Writer,
    };

    #[test]
    fn projection_tracks_current_revision_snapshot_and_sidecar_count_without_event_history() {
        let events = vec![
            review_initialized("review:default", "work:default"),
            revision_published("rev:worktree:sha256:one", vec![]),
            snapshot_observed("snap:git:sha256:one", "rev:worktree:sha256:one"),
            sidecar_observed("review_notes", "sha256:sidecar"),
        ];

        let projection = SessionState::from_events(&events).expect("projection builds");
        let json = serde_json::to_value(&projection).expect("projection serializes");

        assert_eq!(json["schema"], "shore.state");
        assert_eq!(json["version"], 1);
        assert_eq!(
            projection
                .current_revision_id
                .as_ref()
                .map(RevisionId::as_str),
            Some("rev:worktree:sha256:one")
        );
        assert_eq!(
            projection
                .current_snapshot_id
                .as_ref()
                .map(SnapshotId::as_str),
            Some("snap:git:sha256:one")
        );
        assert_eq!(projection.event_count, 4);
        assert_eq!(projection.sidecar_count, 1);
        assert_eq!(projection.note_count, 0);
        assert!(json.get("events").is_none());
    }

    #[test]
    fn projection_tracks_note_count_without_embedded_note_history() {
        let events = vec![
            review_initialized("review:default", "work:default"),
            review_note_imported("note:abc"),
            review_note_imported("note:def"),
        ];

        let projection = SessionState::from_events(&events).expect("projection builds");
        let json = serde_json::to_value(&projection).expect("projection serializes");

        assert_eq!(projection.note_count, 2);
        assert_eq!(json["noteCount"], 2);
        assert!(json.get("notes").is_none());
    }

    #[test]
    fn projection_uses_explicit_supersession_not_timestamp_ordering() {
        let events = vec![
            revision_published("rev:worktree:sha256:one", vec![]),
            revision_published("rev:worktree:sha256:two", vec!["rev:worktree:sha256:one"]),
        ];

        let projection = SessionState::from_events(&events).expect("projection builds");

        assert_eq!(
            projection
                .current_revision_id
                .as_ref()
                .map(RevisionId::as_str),
            Some("rev:worktree:sha256:two")
        );
    }

    #[test]
    fn projection_reports_ambiguous_current_revision() {
        let events = vec![
            revision_published("rev:worktree:sha256:one", vec![]),
            revision_published("rev:worktree:sha256:two", vec![]),
        ];

        let projection = SessionState::from_events(&events).expect("projection still builds");

        assert_eq!(projection.current_revision_id, None);
        assert!(
            projection
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "ambiguous_current_revision")
        );
    }

    #[test]
    fn projection_rejects_unsupported_event_schema_version() {
        let mut event = revision_published("rev:worktree:sha256:one", vec![]);
        event.version = 2;

        let error = SessionState::from_events(&[event]).expect_err("unsupported event rejected");

        assert!(
            error
                .to_string()
                .contains("unsupported event schema/version")
        );
    }

    #[test]
    fn projection_has_typed_state_schema_version_validation() {
        let mut projection =
            SessionState::from_events(&[revision_published("rev:worktree:sha256:one", vec![])])
                .expect("projection builds");
        projection.version = 2;

        let error = projection
            .validate_schema_version()
            .expect_err("version 2 is unsupported");

        assert!(matches!(
            error,
            ShoreError::UnsupportedStateSchemaVersion { .. }
        ));
    }

    fn review_initialized(review_id: &str, work_unit_id: &str) -> ShoreEvent {
        ShoreEvent::new(
            EventType::ReviewInitialized,
            format!("review_initialized:{review_id}:{work_unit_id}"),
            target(review_id, work_unit_id),
            Writer::shore_local_author("0.1.0"),
            ReviewInitializedPayload {},
            "2026-05-09T20:42:45Z",
        )
        .expect("review initialized event builds")
    }

    fn revision_published(revision_id: &str, supersedes: Vec<&str>) -> ShoreEvent {
        ShoreEvent::new(
            EventType::RevisionPublished,
            format!("revision_published:explicit:work:default:{revision_id}"),
            target("review:default", "work:default"),
            Writer::shore_local_author("0.1.0"),
            RevisionPublishedPayload {
                revision_id: RevisionId::new(revision_id),
                supersedes_revision_ids: supersedes.into_iter().map(RevisionId::new).collect(),
            },
            "2026-05-09T20:42:45Z",
        )
        .expect("revision published event builds")
    }

    fn snapshot_observed(snapshot_id: &str, revision_id: &str) -> ShoreEvent {
        ShoreEvent::new(
            EventType::SnapshotObserved,
            format!("snapshot_observed:work:default:{revision_id}:{snapshot_id}"),
            target("review:default", "work:default"),
            Writer::shore_local_author("0.1.0"),
            SnapshotObservedPayload {
                snapshot_id: SnapshotId::new(snapshot_id),
                revision_id: RevisionId::new(revision_id),
            },
            "2026-05-09T20:42:45Z",
        )
        .expect("snapshot observed event builds")
    }

    fn sidecar_observed(source: &str, content_hash: &str) -> ShoreEvent {
        let mut diagnostic_levels = BTreeMap::new();
        diagnostic_levels.insert("warning".to_owned(), 0);

        ShoreEvent::new(
            EventType::SidecarObserved,
            format!("sidecar_observed:{source}:{content_hash}"),
            target("review:default", "work:default"),
            Writer::shore_local_author("0.1.0"),
            SidecarObservedPayload {
                source: match source {
                    "review_notes" => SidecarSource::ReviewNotes,
                    "legacy_hunk_agent_context" => SidecarSource::LegacyHunkAgentContext,
                    other => panic!("unknown sidecar source: {other}"),
                },
                path: "review-notes.json".to_owned(),
                byte_size: 2,
                content_hash: content_hash.to_owned(),
                schema: Some("shore.review-notes".to_owned()),
                imported_schema: None,
                version: Some(1),
                diagnostic_count: 0,
                diagnostic_levels,
            },
            "2026-05-09T20:42:45Z",
        )
        .expect("sidecar observed event builds")
    }

    fn review_note_imported(note_id: &str) -> ShoreEvent {
        ShoreEvent::new(
            EventType::ReviewNoteImported,
            format!("review_note_imported:review_notes:work:default:{note_id}"),
            target("review:default", "work:default"),
            Writer::shore_local_author("0.1.0"),
            ReviewNoteImportedPayload {
                sidecar_source: SidecarSource::ReviewNotes,
                note_id: note_id.to_owned(),
                file_path: "src/lib.rs".to_owned(),
                file_old_path: None,
                target: Some(ImportedNoteTarget {
                    side: Side::New,
                    start_line: 1,
                    end_line: 1,
                }),
                title: "Imported note".to_owned(),
                body: Some("Body".to_owned()),
                body_artifact_path: None,
                body_byte_size: None,
                tags: vec![],
                confidence: None,
                external_source: Some("external".to_owned()),
                author: Some("reviewer".to_owned()),
                created_at: Some("2026-05-10T00:00:00Z".to_owned()),
                sidecar_content_hash: "sha256:sidecar".to_owned(),
            },
            "2026-05-09T20:42:45Z",
        )
        .expect("review note imported event builds")
    }

    fn target(review_id: &str, work_unit_id: &str) -> EventTarget {
        EventTarget::new(ReviewId::new(review_id), WorkUnitId::new(work_unit_id))
    }
}
