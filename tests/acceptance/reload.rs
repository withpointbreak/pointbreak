use shore::dump::{CurrentVerdictStatusName, DumpDocument, DumpInputSource};
use shore::model::{ResolutionStatus, ReviewRowKind};
use shore::session::event::VerdictDecision;
use shore::session::{
    ImportNotesOptions, PublishOptions, PublishVerdictOptions, ReloadDiagnosticCode, import_notes,
    publish_verdict, publish_worktree_review, reload_session,
};
use shore::stream::ORPHAN_SECTION_PATH;

use crate::support::dump_repo;

#[test]
fn acceptance_reload_marks_verdicts_stale_after_revision_shift() {
    let repo = dump_repo();

    let initial = publish_worktree_review(PublishOptions::new(repo.path())).unwrap();
    let verdict = publish_verdict(
        PublishVerdictOptions::new(repo.path())
            .with_decision(VerdictDecision::Pass)
            .with_summary("ship it"),
    )
    .unwrap();

    repo.write("src/lib.rs", "pub fn value() -> u32 { 42 }\n");
    let next = publish_worktree_review(PublishOptions::new(repo.path())).unwrap();
    assert_ne!(initial.revision_id, next.revision_id);

    let outcome = reload_session(repo.path(), || DumpDocument::from_repo(repo.path())).unwrap();

    assert!(
        outcome
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == ReloadDiagnosticCode::VerdictStale),
        "expected VerdictStale diagnostics, got {:?}",
        outcome.diagnostics
    );
    let section = outcome
        .document
        .review_artifacts
        .expect("review artifacts are present");
    let rendered_verdict = section
        .verdicts
        .iter()
        .find(|candidate| candidate.id == verdict.review_artifact_id.as_str())
        .expect("published verdict remains visible");
    assert!(
        rendered_verdict.stale,
        "verdict should render stale after reload"
    );
    assert_eq!(
        section.current_verdict.status,
        CurrentVerdictStatusName::None
    );
}

#[test]
fn acceptance_reload_marks_notes_orphan_after_file_removed() {
    let repo = dump_repo();
    repo.write("src/untracked.rs", "pub fn untracked() -> u32 { 3 }\n");
    publish_worktree_review(PublishOptions::new(repo.path())).unwrap();
    let sidecar = repo.write_fixture(
        "review-notes.json",
        native_review_notes_json("src/untracked.rs"),
    );
    import_notes(ImportNotesOptions::new(repo.path()).with_review_notes(&sidecar)).unwrap();

    repo.remove("src/untracked.rs");
    publish_worktree_review(PublishOptions::new(repo.path())).unwrap();

    let outcome = reload_session(repo.path(), || DumpDocument::from_repo(repo.path())).unwrap();

    assert_eq!(outcome.document.input.source, DumpInputSource::Durable);
    assert!(
        outcome
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == ReloadDiagnosticCode::NoteOrphaned),
        "expected NoteOrphaned diagnostics, got {:?}",
        outcome.diagnostics
    );
    assert_eq!(outcome.document.notes.len(), 1);
    assert_eq!(
        outcome.document.notes[0].anchor.status,
        ResolutionStatus::Orphaned
    );
}

#[test]
fn acceptance_reload_emits_stale_note_row_after_anchor_misses() {
    let repo = dump_repo();
    publish_worktree_review(PublishOptions::new(repo.path())).unwrap();
    let sidecar = repo.write_fixture(
        "review-notes.json",
        r#"{
  "schema": "shore.review-notes",
  "version": 1,
  "files": [
    {
      "path": "src/lib.rs",
      "notes": [
        {
          "title": "Changed return value",
          "target": { "side": "new", "startLine": 99, "endLine": 99 }
        }
      ]
    }
  ]
}"#,
    );
    import_notes(ImportNotesOptions::new(repo.path()).with_review_notes(&sidecar)).unwrap();

    let outcome = reload_session(repo.path(), || DumpDocument::from_repo(repo.path())).unwrap();

    let stale_note = outcome
        .document
        .notes
        .iter()
        .find(|note| note.anchor.status == ResolutionStatus::Stale)
        .expect("a stale-resolution note is preserved after the reload");

    let stale_row = outcome
        .document
        .stream
        .rows
        .iter()
        .find(|row| matches!(row.kind, ReviewRowKind::StaleNote { .. }))
        .expect("stream contains a stale-note row");

    match &stale_row.kind {
        ReviewRowKind::StaleNote {
            note_id,
            resolution_status,
            target_path,
            ..
        } => {
            assert_eq!(note_id, &stale_note.id);
            assert_eq!(*resolution_status, ResolutionStatus::Stale);
            assert_eq!(target_path, "src/lib.rs");
        }
        other => panic!("expected StaleNote, got {other:?}"),
    }
}

#[test]
fn acceptance_reload_emits_orphan_section_after_file_removed() {
    let repo = dump_repo();
    repo.write("src/untracked.rs", "pub fn untracked() -> u32 { 3 }\n");
    publish_worktree_review(PublishOptions::new(repo.path())).unwrap();
    let sidecar = repo.write_fixture(
        "review-notes.json",
        native_review_notes_json("src/untracked.rs"),
    );
    import_notes(ImportNotesOptions::new(repo.path()).with_review_notes(&sidecar)).unwrap();

    repo.remove("src/untracked.rs");
    publish_worktree_review(PublishOptions::new(repo.path())).unwrap();

    let outcome = reload_session(repo.path(), || DumpDocument::from_repo(repo.path())).unwrap();

    let rows = &outcome.document.stream.rows;
    let orphan_header_pos = rows
        .iter()
        .position(
            |row| matches!(&row.kind, ReviewRowKind::FileHeader { path, .. } if path == ORPHAN_SECTION_PATH),
        )
        .expect("synthetic orphan header present");
    let orphan_row = &rows[orphan_header_pos + 1];
    match &orphan_row.kind {
        ReviewRowKind::StaleNote {
            resolution_status,
            target_path,
            ..
        } => {
            assert_eq!(*resolution_status, ResolutionStatus::Orphaned);
            assert_eq!(target_path, "src/untracked.rs");
        }
        other => panic!("expected orphan StaleNote, got {other:?}"),
    }
}

fn native_review_notes_json(path: &str) -> String {
    format!(
        r#"{{
  "schema": "shore.review-notes",
  "version": 1,
  "files": [
    {{
      "path": "{path}",
      "notes": [
        {{
          "title": "Changed return value",
          "target": {{ "side": "new", "startLine": 1, "endLine": 1 }}
        }}
      ]
    }}
  ]
}}"#
    )
}
