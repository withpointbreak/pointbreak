use std::collections::BTreeMap;

use crate::error::Result;
use crate::model::{DiffSnapshot, ResolutionStatus};
use crate::session::event::{EventType, ImportedNoteTarget, ReviewNoteImportedPayload, ShoreEvent};
use crate::session::projection::body_content::{
    BodyContentState, BodyRemovalLens, resolve_body_content,
};
use crate::session::store::backend::StoreBackend;
use crate::sidecar::{
    ReviewNoteEntry, ReviewNoteTarget, ReviewNotesFile, ReviewNotesSidecar, resolve_notes,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterNoteView {
    pub id: String,
    pub title: String,
    pub body: Option<String>,
    pub body_content_state: BodyContentState,
    /// The removal key when the body is removed. The imported-note payload
    /// carries no body content hash, so this is the surface's twin of the
    /// snapshot result's removed-content-hash field; `None` while present.
    pub removed_body_content_hash: Option<String>,
    pub target: Option<ImportedNoteTarget>,
    pub status: AdapterNoteStatus,
    pub file_path: String,
    pub file_old_path: Option<String>,
    pub tags: Vec<String>,
    pub confidence: Option<String>,
    pub external_source: Option<String>,
    pub author: Option<String>,
    pub created_at: Option<String>,
    pub sidecar_content_hash: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterNoteStatus {
    Exact,
    Relocated,
    FileLevel,
    Stale,
    Orphaned,
    Unresolved,
}

impl AdapterNoteStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Relocated => "relocated",
            Self::FileLevel => "file_level",
            Self::Stale => "stale",
            Self::Orphaned => "orphaned",
            Self::Unresolved => "unresolved",
        }
    }
}

pub(super) fn project_adapter_notes(
    events: &[ShoreEvent],
    backend: &StoreBackend,
    snapshot: &DiffSnapshot,
    include_body: bool,
    removal_lens: &BodyRemovalLens<'_>,
) -> Result<Vec<AdapterNoteView>> {
    let mut payloads = Vec::new();
    for event in events
        .iter()
        .filter(|event| event.event_type == EventType::ReviewNoteImported)
    {
        payloads.push(serde_json::from_value::<ReviewNoteImportedPayload>(
            event.payload.clone(),
        )?);
    }

    let statuses = adapter_note_statuses(snapshot, &payloads);
    let mut views = payloads
        .iter()
        .map(|payload| {
            let content = resolve_body_content(
                backend,
                removal_lens,
                include_body,
                payload.body.clone(),
                payload.body_artifact_path.as_deref(),
            )?;
            let body_content_state = content.state();
            let removed_body_content_hash = content.removed_content_hash().map(str::to_owned);
            let body = content.into_text();
            Ok(AdapterNoteView {
                id: payload.note_id.clone(),
                title: payload.title.clone(),
                body,
                body_content_state,
                removed_body_content_hash,
                target: payload.target.clone(),
                status: statuses
                    .get(&payload.note_id)
                    .copied()
                    .unwrap_or(AdapterNoteStatus::Unresolved),
                file_path: payload.file_path.clone(),
                file_old_path: payload.file_old_path.clone(),
                tags: payload.tags.clone(),
                confidence: payload.confidence.clone(),
                external_source: payload.external_source.clone(),
                author: payload.author.clone(),
                created_at: payload.created_at.clone(),
                sidecar_content_hash: payload.sidecar_content_hash.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;

    views.sort_by(|left, right| {
        left.file_path
            .cmp(&right.file_path)
            .then_with(|| {
                left.target
                    .as_ref()
                    .map(|target| target.start_line)
                    .cmp(&right.target.as_ref().map(|target| target.start_line))
            })
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(views)
}

fn adapter_note_statuses(
    snapshot: &DiffSnapshot,
    payloads: &[ReviewNoteImportedPayload],
) -> BTreeMap<String, AdapterNoteStatus> {
    let sidecar = ReviewNotesSidecar {
        schema: Some("shore.review-notes".to_owned()),
        version: 1,
        summary: None,
        files: payloads
            .iter()
            .map(|payload| ReviewNotesFile {
                path: payload.file_path.clone(),
                old_path: payload.file_old_path.clone(),
                summary: None,
                notes: vec![review_note_entry_from_payload(payload, None)],
            })
            .collect(),
    };
    resolve_notes(&snapshot.files, &sidecar)
        .notes
        .into_iter()
        .map(|note| {
            (
                note.id.as_str().to_owned(),
                adapter_note_status(&note.anchor.status),
            )
        })
        .collect()
}

fn review_note_entry_from_payload(
    payload: &ReviewNoteImportedPayload,
    body: Option<String>,
) -> ReviewNoteEntry {
    ReviewNoteEntry {
        id: Some(payload.note_id.clone()),
        title: Some(payload.title.clone()),
        body,
        target: payload.target.as_ref().map(imported_note_target),
        tags: payload.tags.clone(),
        confidence: payload.confidence.clone(),
        source: payload.external_source.clone(),
        author: payload.author.clone(),
        created_at: payload.created_at.clone(),
    }
}

fn imported_note_target(target: &ImportedNoteTarget) -> ReviewNoteTarget {
    ReviewNoteTarget {
        side: target.side,
        start_line: target.start_line,
        end_line: target.end_line,
    }
}

pub(super) fn adapter_note_status(status: &ResolutionStatus) -> AdapterNoteStatus {
    match status {
        ResolutionStatus::Exact => AdapterNoteStatus::Exact,
        ResolutionStatus::Relocated => AdapterNoteStatus::Relocated,
        ResolutionStatus::FileLevel => AdapterNoteStatus::FileLevel,
        ResolutionStatus::Stale => AdapterNoteStatus::Stale,
        ResolutionStatus::Orphaned => AdapterNoteStatus::Orphaned,
        ResolutionStatus::Unresolved => AdapterNoteStatus::Unresolved,
    }
}
