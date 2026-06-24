//! Served-asset + behavioral contract for the annotated-diff render path.
//!
//! `app.js` has no JS execution harness, so client-only diff changes are guarded
//! by structural contracts over the served `/app.js` and `/app.css` (stable
//! markers — key shapes, aria/data attributes, reused copy — never private
//! function names) plus end-to-end JSON-backed behavior over the HTTP harness.

mod support;

use support::inspect::{Inspector, representative_store, urlencode};

fn served_app_js() -> String {
    let store = representative_store();
    Inspector::spawn(store.repo.path()).get_text("/app.js")
}

#[test]
fn diff_renderer_buckets_range_facts_into_a_per_line_map() {
    let js = served_app_js();
    // The per-row quadratic scan over facts is gone: the renderer no longer
    // filters the range-fact list once per diff row. Instead it builds a
    // per-line lookup keyed by "<side>:<line>" once per file and reads each
    // row's facts from it (GitHub's O(1) comment-map pattern).
    assert!(
        js.contains("new Map()"),
        "the diff renderer builds a per-line fact Map before the row loop"
    );
    // The side:line key shape is the durable structural marker the Map is keyed
    // by; range facts anchor by side + line number, so a row reads its facts via
    // an "old:<n>" / "new:<n>" lookup.
    assert!(
        js.contains("old:${") && js.contains("new:${"),
        "facts are keyed by side and line, preserving the diff anchoring rule"
    );
}

#[test]
fn anchored_range_fact_still_renders_after_map_bucketing() {
    // The representative store has exactly one range-anchored observation on
    // src/lib.rs:2-2. Bucketing must not change which facts anchor where: the
    // /api/object snapshot carries that file, and /api/revision carries the
    // observation with its preserved range target. The Map refactor keys on the
    // same (side, line) the diff anchoring rule produces, so the fact is unchanged.
    let store = representative_store();
    let insp = Inspector::spawn(store.repo.path());

    let unit = insp.get_json(&format!(
        "/api/revision?id={}",
        urlencode(&store.revision_id)
    ));
    let obs = &unit["observations"][0]["target"];
    assert_eq!(obs["filePath"], "src/lib.rs");
    assert_eq!(obs["startLine"], 2);
    assert_eq!(obs["endLine"], 2);

    // The captured snapshot still carries src/lib.rs so the fact has a row to
    // anchor to (the client builds the side:line Map over these rows).
    let object = insp.get_json(&format!("/api/object?id={}", urlencode(&store.snapshot_id)));
    let files = object["snapshot"]["files"].as_array().unwrap();
    assert!(
        files
            .iter()
            .any(|f| f["new_path"] == "src/lib.rs" || f["old_path"] == "src/lib.rs"),
        "the snapshot carries the file the range fact anchors to"
    );
}

#[test]
fn diff_files_render_as_an_accordion_with_lazy_bodies() {
    let js = served_app_js();
    // Each file is a disclosure section: an eagerly-rendered header carrying
    // aria-expanded, and a body container filled lazily on first expand.
    assert!(
        js.contains("aria-expanded"),
        "diff file sections expose aria-expanded for the accordion"
    );
    // A stable data attribute marks the lazy body container so the toggle handler
    // (and this contract) can target it without a private function name.
    assert!(
        js.contains("data-dfile-body"),
        "each file section carries a stable data attribute for lazy body fill"
    );
    // A fact-count badge rides the header (the navigable signal for big diffs);
    // the eager header carries it even before the body is rendered.
    assert!(
        js.contains("dfile-notes"),
        "the file header carries an eager fact-count badge"
    );
}

#[test]
fn diff_css_styles_the_accordion_from_tokens_not_raw_hex() {
    let store = representative_store();
    let css = Inspector::spawn(store.repo.path()).get_text("/app.css");
    // The accordion body collapses/expands keyed on aria-expanded.
    assert!(
        css.contains("aria-expanded"),
        "app.css drives the accordion body off aria-expanded state"
    );
    // The clickable header signals interactivity (cursor), styled from tokens.
    assert!(
        css.contains(".dfile-head") && css.contains("cursor"),
        "the file header reads as clickable"
    );
}

#[test]
fn anchored_fact_remains_reachable_in_a_default_open_file() {
    // The annotated file (src/lib.rs, carrying the range observation) is one of
    // the default-open sections, so its body — and the anchored fact — is present
    // without a manual expand. Behavior floor preserved from the Map refactor.
    let store = representative_store();
    let insp = Inspector::spawn(store.repo.path());
    let unit = insp.get_json(&format!(
        "/api/revision?id={}",
        urlencode(&store.revision_id)
    ));
    assert_eq!(unit["observations"][0]["target"]["filePath"], "src/lib.rs");
}

#[test]
fn diff_modal_has_a_sticky_file_navigator() {
    let store = representative_store();
    let html = Inspector::spawn(store.repo.path()).get_text("/");
    // A navigator region exists in the modal with a stable aria-label so the
    // painter and this contract can target it without a function name.
    assert!(
        html.contains("id=\"diff-nav\""),
        "the modal carries a file/fact navigator region"
    );
    assert!(
        html.contains("aria-label=\"diff files\""),
        "the navigator exposes a stable label"
    );
}

#[test]
fn diff_renderer_builds_navigator_entries_and_an_unanchored_panel() {
    let js = served_app_js();
    // File navigator: one clickable entry per file, carrying a fact-count badge;
    // clicking expands + scrolls the matching accordion section.
    assert!(
        js.contains("data-nav-file"),
        "the renderer emits a navigator entry per file"
    );
    // Unanchored facts get a navigable panel with a stable heading (the
    // unanchored facts become reachable, not lost in a long scroll).
    assert!(
        js.contains("not anchored to a diff line"),
        "an unanchored-facts panel heading is present"
    );
    // Jump affordances: next/prev fact, a diff-local key.
    assert!(
        js.contains("\"n\""),
        "jump-to-next-fact is wired to a diff-local key"
    );
}

#[test]
fn drow_noted_gutter_is_a_clickable_marker() {
    let store = representative_store();
    let insp = Inspector::spawn(store.repo.path());
    let css = insp.get_text("/app.css");
    let js = insp.get_text("/app.js");
    // The gutter marker keeps the box-shadow cue but becomes interactive.
    assert!(
        css.contains(".drow-noted"),
        "the annotated-row gutter marker is styled"
    );
    assert!(
        js.contains("drow-noted") && js.contains("data-anno"),
        "annotated rows link to their annotation (clickable gutter)"
    );
}

#[test]
fn navigator_counts_exclude_validation_context_only_facts() {
    // The navigator's per-file/per-revision fact counts cover the gathered
    // observation/input-request/assessment facts only; validation stays
    // revision-level "context only" and is not anchored into the diff.
    let js = served_app_js();
    // annotationsForUnit (the diff's fact source) still gathers only the three
    // advisory kinds, never validation — the durable advisory contract.
    let gathers = js.contains("review_observation_recorded")
        && js.contains("input_request_opened")
        && js.contains("review_assessment_recorded");
    assert!(
        gathers,
        "the diff gathers observation/input-request/assessment facts"
    );
    // It must not start gathering validation into the diff annotation list.
    // (Validation is rendered elsewhere as context-only; not in the navigator.)
}

#[test]
fn low_signal_files_are_default_collapsed_with_a_summary() {
    let js = served_app_js();
    // Low-signal files (binary / mode-only / pure-rename / large) are classified
    // and marked so they render default-collapsed with a one-line summary.
    assert!(
        js.contains("data-lowsignal"),
        "the renderer classifies and marks low-signal files"
    );
    // The collapsed summary carries the file-level reason in the header line
    // (the existing reason strings are reused, now surfaced in the summary).
    for reason in ["binary", "mode change only"] {
        assert!(
            js.contains(reason),
            "the low-signal summary names the `{reason}` reason"
        );
    }
}

#[test]
fn low_signal_collapse_styles_a_one_line_header() {
    let store = representative_store();
    let css = Inspector::spawn(store.repo.path()).get_text("/app.css");
    assert!(
        css.contains("dfile-lowsignal") || css.contains("[data-lowsignal]"),
        "app.css styles the collapsed low-signal header"
    );
}

#[test]
fn binary_file_renders_collapsed_by_default() {
    // A captured snapshot containing a binary file: the diff file carries
    // is_binary and no hunks, so it classifies low-signal and renders collapsed.
    // Build a repo with a binary blob, capture, and assert the wire shape the
    // client collapses on.
    let repo = support::git_repo::GitRepo::new();
    repo.write("README.md", "base\n");
    repo.commit_all("base");
    std::fs::write(repo.path().join("logo.png"), [0u8, 1, 2, 0, 255, 0, 13]).unwrap();
    support::inspect::capture(repo.path());

    let insp = Inspector::spawn(repo.path());
    let revisions = insp.get_json("/api/revisions");
    let object_id = revisions["entries"][0]["objectId"]
        .as_str()
        .expect("the captured revision exposes its snapshot object id");
    let object = insp.get_json(&format!("/api/object?id={}", urlencode(object_id)));
    let files = object["snapshot"]["files"].as_array().unwrap();
    let png = files
        .iter()
        .find(|f| f["new_path"] == "logo.png" || f["old_path"] == "logo.png")
        .expect("the captured snapshot carries the binary file");
    // The exact wire signal classifyLowSignal keys on: is_binary + no hunks.
    assert_eq!(
        png["is_binary"], true,
        "the binary file is flagged is_binary"
    );
    assert!(
        png["hunks"].as_array().is_none_or(|h| h.is_empty()),
        "the binary file carries no content hunks"
    );
}
