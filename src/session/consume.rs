use std::collections::BTreeMap;
use std::path::{Component, Path};

use serde::Deserialize;

use crate::error::Result;
use crate::git::git_worktree_root;
use crate::session::event::{EventType, ReviewNoteImportedPayload};
use crate::sidecar::{
    ParsedReviewNotes, ReviewNoteEntry, ReviewNoteTarget, ReviewNotesFile, ReviewNotesSidecar,
};

#[derive(Deserialize)]
struct NoteBodyArtifact {
    schema: String,
    version: u32,
    body: String,
}

/// Replay a single durable `review_note_imported` event payload back into a `ReviewNoteEntry`.
///
/// Loads artifact-backed note bodies from `.shore/artifacts/notes/` when available. Falls back to
/// inline body if no artifact path is set.
#[allow(dead_code)]
pub(crate) fn replay_note_entry(
    payload: &ReviewNoteImportedPayload,
    shore_dir: &Path,
) -> Result<ReviewNoteEntry> {
    let target = payload
        .target
        .as_ref()
        .map(|imported_target| ReviewNoteTarget {
            side: imported_target.side,
            start_line: imported_target.start_line,
            end_line: imported_target.end_line,
        });

    // Load artifact body if path is set; otherwise use inline body
    let body = if let Some(artifact_path) = &payload.body_artifact_path {
        load_note_body_artifact(shore_dir, artifact_path)?
    } else {
        payload.body.clone()
    };

    Ok(ReviewNoteEntry {
        id: Some(payload.note_id.clone()),
        title: Some(payload.title.clone()),
        body,
        target,
        tags: payload.tags.clone(),
        confidence: payload.confidence.clone(),
        source: payload.external_source.clone(),
        author: payload.author.clone(),
        created_at: payload.created_at.clone(),
    })
}

fn load_note_body_artifact(shore_dir: &Path, artifact_path: &str) -> Result<Option<String>> {
    if !artifact_path.starts_with("artifacts/notes/")
        || Path::new(artifact_path).components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(crate::error::ShoreError::Message(format!(
            "Invalid artifact path: {}",
            artifact_path
        )));
    }

    let full_path = shore_dir.join(artifact_path);
    let artifact_bytes = std::fs::read(&full_path).map_err(|e| {
        crate::error::ShoreError::Message(format!(
            "Failed to read artifact {}: {}",
            artifact_path, e
        ))
    })?;
    let artifact: NoteBodyArtifact = serde_json::from_slice(&artifact_bytes)?;
    if artifact.schema != "shore.note-body" || artifact.version != 1 {
        return Err(crate::error::ShoreError::Message(format!(
            "Unsupported note body artifact schema/version: {} v{}",
            artifact.schema, artifact.version
        )));
    }
    Ok(Some(artifact.body))
}

/// Replay a collection of durable `review_note_imported` event payloads into a `ParsedReviewNotes`.
///
/// Groups notes by file path, deterministically ordered. The grouping is synthetic and does not
/// claim that the original transport file order is authoritative. Event order is not used for
/// file ordering.
#[allow(dead_code)]
pub(crate) fn parsed_review_notes_from_imports(
    payloads: &[ReviewNoteImportedPayload],
    shore_dir: &Path,
) -> Result<ParsedReviewNotes> {
    // Build a map of file_path -> (old_path, notes) to group notes by file
    let mut file_map: BTreeMap<String, (Option<String>, Vec<ReviewNoteEntry>)> = BTreeMap::new();

    for payload in payloads {
        let entry = replay_note_entry(payload, shore_dir)?;

        file_map
            .entry(payload.file_path.clone())
            .or_insert((payload.file_old_path.clone(), Vec::new()))
            .1
            .push(entry);
    }

    // Convert the map into an ordered list of ReviewNotesFile, with notes sorted by line range
    let files = file_map
        .into_iter()
        .map(|(path, (old_path, mut notes))| {
            // Sort notes by start_line for deterministic ordering within each file
            notes.sort_by_key(|note| {
                (
                    note.target.map(|t| t.start_line).unwrap_or(0),
                    note.id.clone().unwrap_or_default(),
                )
            });
            ReviewNotesFile {
                path,
                old_path,
                summary: None,
                notes,
            }
        })
        .collect();

    Ok(ParsedReviewNotes {
        sidecar: ReviewNotesSidecar {
            schema: Some("shore.review-notes".to_owned()),
            version: 1,
            summary: None,
            files,
        },
        diagnostics: Vec::new(),
    })
}

/// Load durable notes for a repository by replaying imported-note events.
///
/// Discovers `.shore/` at the worktree root. Returns `Ok(None)` if the store does not exist
/// (no durable state to load) or if no imported notes exist in the store.
///
/// This is a read-only operation and does not create directories or perform any mutations.
pub fn load_durable_notes_for_repo(repo: impl AsRef<Path>) -> Result<Option<ParsedReviewNotes>> {
    let repo_path = repo.as_ref();
    let worktree_root = git_worktree_root(repo_path)?;
    let shore_dir = worktree_root.join(".shore");

    // If .shore doesn't exist, there's no durable state to load
    if !shore_dir.exists() {
        return Ok(None);
    }

    // Read all events from the store
    let events = crate::session::read_events(&worktree_root)?;

    // Filter to ReviewNoteImported events and deserialize payloads
    let mut imported_payloads = Vec::new();
    for event in events {
        if event.event_type != EventType::ReviewNoteImported {
            continue;
        }

        let payload: ReviewNoteImportedPayload = serde_json::from_value(event.payload)?;
        imported_payloads.push(payload);
    }

    // Return None if no imported notes exist
    if imported_payloads.is_empty() {
        return Ok(None);
    }

    // Replay the imported notes into ParsedReviewNotes
    let parsed = parsed_review_notes_from_imports(&imported_payloads, &shore_dir)?;

    Ok(Some(parsed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Side;
    use crate::session::event::ImportedNoteTarget;

    fn test_payload_for(file_path: &str, note_id: &str) -> ReviewNoteImportedPayload {
        ReviewNoteImportedPayload {
            sidecar_source: crate::session::event::SidecarSource::ReviewNotes,
            note_id: note_id.to_owned(),
            file_path: file_path.to_owned(),
            file_old_path: None,
            target: Some(ImportedNoteTarget {
                side: Side::New,
                start_line: 2,
                end_line: 3,
            }),
            title: format!("Title for {}", note_id),
            body: Some(format!("Body for {}", note_id)),
            body_artifact_path: None,
            body_byte_size: None,
            tags: vec!["tag".to_owned()],
            confidence: Some("high".to_owned()),
            external_source: Some("tool".to_owned()),
            author: Some("reviewer".to_owned()),
            created_at: Some("2026-05-10T00:00:00Z".to_owned()),
            sidecar_content_hash: "sha256:abc".to_owned(),
        }
    }

    #[test]
    fn imported_note_payload_converts_to_review_note_entry() {
        let payload = ReviewNoteImportedPayload {
            sidecar_source: crate::session::event::SidecarSource::ReviewNotes,
            note_id: "note:123".to_owned(),
            file_path: "src/lib.rs".to_owned(),
            file_old_path: None,
            target: Some(ImportedNoteTarget {
                side: Side::New,
                start_line: 2,
                end_line: 3,
            }),
            title: "Durable title".to_owned(),
            body: Some("Durable body".to_owned()),
            body_artifact_path: None,
            body_byte_size: None,
            tags: vec!["tag".to_owned()],
            confidence: Some("high".to_owned()),
            external_source: Some("tool".to_owned()),
            author: Some("reviewer".to_owned()),
            created_at: Some("2026-05-10T00:00:00Z".to_owned()),
            sidecar_content_hash: "sha256:abc".to_owned(),
        };

        let entry = replay_note_entry(&payload, Path::new(".shore")).expect("entry builds");

        assert_eq!(entry.id.as_deref(), Some("note:123"));
        assert_eq!(entry.title.as_deref(), Some("Durable title"));
        assert_eq!(entry.body.as_deref(), Some("Durable body"));
        assert_eq!(entry.target.unwrap().start_line, 2);
    }

    #[test]
    fn imported_notes_group_by_file_without_using_event_order_as_file_order() {
        let events = vec![
            test_payload_for("b.rs", "note:b"),
            test_payload_for("a.rs", "note:a"),
        ];

        let parsed =
            parsed_review_notes_from_imports(&events, Path::new(".shore")).expect("parses");

        assert_eq!(parsed.sidecar.files.len(), 2);
        assert_eq!(
            parsed
                .sidecar
                .files
                .iter()
                .map(|f| f.notes.len())
                .sum::<usize>(),
            2
        );
        // Files should be in sorted order (a.rs before b.rs) despite event order being reversed
        assert_eq!(parsed.sidecar.files[0].path, "a.rs");
        assert_eq!(parsed.sidecar.files[1].path, "b.rs");
    }

    #[test]
    fn artifact_backed_note_body_is_loaded_from_artifact() {
        let shore_dir = tempfile::tempdir().expect("create shore dir");
        let artifact_path = "artifacts/notes/note-abc.json";
        let artifact_file = shore_dir.path().join(artifact_path);
        std::fs::create_dir_all(artifact_file.parent().expect("artifact parent"))
            .expect("create artifact dir");
        std::fs::write(
            &artifact_file,
            r#"{"schema":"shore.note-body","version":1,"body":"Artifact body"}"#,
        )
        .expect("write artifact");

        let payload = ReviewNoteImportedPayload {
            sidecar_source: crate::session::event::SidecarSource::ReviewNotes,
            note_id: "note:with-artifact".to_owned(),
            file_path: "src/lib.rs".to_owned(),
            file_old_path: None,
            target: Some(ImportedNoteTarget {
                side: Side::New,
                start_line: 5,
                end_line: 6,
            }),
            title: "Note with artifact body".to_owned(),
            body: None,
            body_artifact_path: Some(artifact_path.to_owned()),
            body_byte_size: Some(256),
            tags: vec![],
            confidence: None,
            external_source: None,
            author: None,
            created_at: None,
            sidecar_content_hash: "sha256:abc".to_owned(),
        };

        let entry = replay_note_entry(&payload, shore_dir.path()).expect("entry builds");

        assert_eq!(entry.body.as_deref(), Some("Artifact body"));
    }

    #[test]
    fn artifact_body_path_must_stay_under_artifacts_notes() {
        let payload = ReviewNoteImportedPayload {
            sidecar_source: crate::session::event::SidecarSource::ReviewNotes,
            note_id: "note:escape".to_owned(),
            file_path: "src/lib.rs".to_owned(),
            file_old_path: None,
            target: None,
            title: "Escaped".to_owned(),
            body: None,
            body_artifact_path: Some("../outside.json".to_owned()),
            body_byte_size: None,
            tags: vec![],
            confidence: None,
            external_source: None,
            author: None,
            created_at: None,
            sidecar_content_hash: "sha256:abc".to_owned(),
        };

        let error =
            replay_note_entry(&payload, Path::new(".shore")).expect_err("path should be rejected");
        assert!(error.to_string().contains("Invalid artifact path"));
    }

    #[test]
    fn artifact_body_schema_must_match() {
        let shore_dir = tempfile::tempdir().expect("create shore dir");
        let artifact_path = "artifacts/notes/note-abc.json";
        let artifact_file = shore_dir.path().join(artifact_path);
        std::fs::create_dir_all(artifact_file.parent().expect("artifact parent"))
            .expect("create artifact dir");
        std::fs::write(
            &artifact_file,
            r#"{"schema":"wrong.schema","version":2,"body":"Artifact body"}"#,
        )
        .expect("write artifact");

        let payload = ReviewNoteImportedPayload {
            sidecar_source: crate::session::event::SidecarSource::ReviewNotes,
            note_id: "note:wrong-schema".to_owned(),
            file_path: "src/lib.rs".to_owned(),
            file_old_path: None,
            target: None,
            title: "Wrong schema".to_owned(),
            body: None,
            body_artifact_path: Some(artifact_path.to_owned()),
            body_byte_size: None,
            tags: vec![],
            confidence: None,
            external_source: None,
            author: None,
            created_at: None,
            sidecar_content_hash: "sha256:abc".to_owned(),
        };

        let error =
            replay_note_entry(&payload, shore_dir.path()).expect_err("schema should be rejected");
        assert!(
            error
                .to_string()
                .contains("Unsupported note body artifact schema/version")
        );
    }
}
