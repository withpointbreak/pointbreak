mod support;

use std::process::Output;

use serde_json::Value;
use support::{dump_repo, shore};

#[test]
fn dump_emits_stale_note_row_when_anchor_misses_line_range() {
    let repo = dump_repo();
    let repo_arg = repo.path().to_str().unwrap();
    let sidecar_dir = tempfile::tempdir().unwrap();
    let sidecar_path = sidecar_dir.path().join("review-notes.json");
    std::fs::write(&sidecar_path, review_notes_json("src/lib.rs", 99, 99)).unwrap();

    let output = shore([
        "dump",
        "--repo",
        repo_arg,
        "--review-notes",
        sidecar_path.to_str().unwrap(),
    ]);
    let json = parse_json(&output);

    let stale_rows = stream_rows_of_kind(&json, "stale_note");
    assert_eq!(
        stale_rows.len(),
        1,
        "expected exactly one stale_note row; got json: {json:#?}",
    );
    let row = stale_rows[0];
    assert_eq!(row["title"], "Review note");
    assert_eq!(row["resolution_status"], "stale");
    assert_eq!(row["target_path"], "src/lib.rs");
    assert_eq!(row["target_line_range"]["start"], 99);
    assert_eq!(row["target_line_range"]["end"], 99);
    assert!(
        row["note_id"].as_str().is_some(),
        "stale_note row must carry a note_id",
    );
}

#[test]
fn dump_emits_orphan_section_when_note_file_absent_from_snapshot() {
    let repo = dump_repo();
    let repo_arg = repo.path().to_str().unwrap();
    let sidecar_dir = tempfile::tempdir().unwrap();
    let sidecar_path = sidecar_dir.path().join("review-notes.json");
    std::fs::write(&sidecar_path, review_notes_json("src/gone.rs", 1, 1)).unwrap();

    let output = shore([
        "dump",
        "--repo",
        repo_arg,
        "--review-notes",
        sidecar_path.to_str().unwrap(),
    ]);
    let json = parse_json(&output);

    let rows = json["stream"]["rows"].as_array().unwrap();
    let orphan_header_pos = rows.iter().position(|row| {
        row["kind"]
            .as_object()
            .and_then(|kind| kind.get("file_header"))
            .is_some_and(|header| header["path"] == "<orphaned notes>")
    });
    assert!(
        orphan_header_pos.is_some(),
        "expected synthetic orphan header; got: {json:#?}",
    );

    let next_row = &rows[orphan_header_pos.unwrap() + 1];
    let stale_note = next_row["kind"]
        .as_object()
        .and_then(|kind| kind.get("stale_note"))
        .expect("orphan row should be stale_note");
    assert_eq!(stale_note["resolution_status"], "orphaned");
    assert_eq!(stale_note["target_path"], "src/gone.rs");
}

#[test]
fn dump_omits_orphan_section_when_no_orphan_notes() {
    let repo = dump_repo();
    let repo_arg = repo.path().to_str().unwrap();

    let output = shore(["dump", "--repo", repo_arg]);
    let json = parse_json(&output);

    let rows = json["stream"]["rows"].as_array().unwrap();
    let has_orphan_header = rows.iter().any(|row| {
        row["kind"]
            .as_object()
            .and_then(|kind| kind.get("file_header"))
            .is_some_and(|header| header["path"] == "<orphaned notes>")
    });
    assert!(
        !has_orphan_header,
        "no orphan header expected; got: {json:#?}"
    );
}

fn stream_rows_of_kind<'a>(json: &'a Value, kind: &str) -> Vec<&'a Value> {
    json["stream"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["kind"].as_object()?.get(kind))
        .collect()
}

fn parse_json(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn review_notes_json(path: &str, start_line: u32, end_line: u32) -> String {
    format!(
        r#"{{
  "schema": "shore.review-notes",
  "version": 1,
  "files": [
    {{
      "path": "{path}",
      "notes": [
        {{
          "title": "Review note",
          "target": {{ "side": "new", "startLine": {start_line}, "endLine": {end_line} }}
        }}
      ]
    }}
  ]
}}"#
    )
}
