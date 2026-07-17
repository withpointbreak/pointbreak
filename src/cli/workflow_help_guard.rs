//! Guard: the root command's rendered long help carries the public review
//! workflow map — the five stages in order, their flattened-command mapping,
//! the short first-review path, and direct recovery pointers — while the short
//! `-h` form stays compact and the command inventory stays exactly the shipped
//! flat grammar. Complements `help_vocab_guard.rs` (retired vocabulary),
//! `help_hygiene_guard.rs` (flag help hygiene), and `about_bleed_guard.rs`
//! (leaf about integrity); this guard owns the root workflow narrative.

use clap::CommandFactory;

fn rendered_long_help() -> String {
    super::Cli::command().render_long_help().to_string()
}

fn rendered_short_help() -> String {
    super::Cli::command().render_help().to_string()
}

#[track_caller]
fn assert_ordered_anchors(text: &str, anchors: &[&str]) {
    let mut last_index = 0;
    let mut last_anchor = "start of help";
    for anchor in anchors {
        let found = text[last_index..]
            .find(anchor)
            .unwrap_or_else(|| panic!("missing help anchor {anchor:?} after {last_anchor:?}"));
        last_index += found + anchor.len();
        last_anchor = anchor;
    }
}

#[test]
fn long_help_maps_the_five_stages_in_order() {
    let help = rendered_long_help();
    assert!(
        help.contains("Work -> Claims -> Evidence -> Questions -> Call"),
        "root long help names the five stages as one ordered chain:\n{help}"
    );
    assert_ordered_anchors(
        &help,
        &[
            "Work — what changed: capture, revision, inspect",
            "Claims — what an author or reviewer asserts: observation",
            "Evidence — what was checked: validation",
            "Questions — what still needs judgment: input-request",
            "Call — the current assessment: assessment",
        ],
    );
    assert!(
        help.contains("attention"),
        "root long help explains the attention read:\n{help}"
    );
    assert!(
        help.contains("association"),
        "root long help explains commit association:\n{help}"
    );
}

#[test]
fn long_help_shows_the_short_path_into_getting_started() {
    let help = rendered_long_help();
    assert_ordered_anchors(
        &help,
        &[
            "capture --summary",
            "inspect --open",
            "docs/getting-started.md",
        ],
    );
    assert!(
        help.contains("read-only"),
        "root long help presents Review as read-only:\n{help}"
    );
}

#[test]
fn long_help_gives_direct_recovery_pointers() {
    let help = rendered_long_help();
    for pointer in [
        "store paths",
        "revision list",
        "--revision",
        "--replaces",
        "association record",
    ] {
        assert!(
            help.contains(pointer),
            "root long help missing recovery pointer {pointer:?}:\n{help}"
        );
    }
    assert!(
        help.contains("never a recapture"),
        "root long help states the same-revision landing rule:\n{help}"
    );
}

#[test]
fn short_help_stays_compact() {
    let help = rendered_short_help();
    assert!(
        !help.contains("Work — what changed"),
        "-h keeps the one-line about; the stage map belongs to --help:\n{help}"
    );
    assert!(
        help.contains("Usage: pointbreak"),
        "short help renders:\n{help}"
    );
}

#[test]
fn command_inventory_is_exactly_the_shipped_flat_grammar() {
    let cmd = super::Cli::command();
    let mut names: Vec<String> = cmd
        .get_subcommands()
        .map(|sub| sub.get_name().to_owned())
        .filter(|name| name != "help")
        .collect();
    names.sort();
    let expected = [
        "assessment",
        "association",
        "attention",
        "capture",
        "diff",
        "endorse",
        "history",
        "identity",
        "input-request",
        "inspect",
        "key",
        "observation",
        "revision",
        "store",
        "validation",
        "version",
    ];
    assert_eq!(
        names, expected,
        "the workflow help must not add, remove, or nest commands"
    );
    assert!(
        cmd.find_subcommand("review").is_none(),
        "no nested `review` family returns"
    );
    let aliased: Vec<String> = cmd
        .get_subcommands()
        .filter(|sub| sub.get_visible_aliases().next().is_some())
        .map(|sub| sub.get_name().to_owned())
        .collect();
    assert!(
        aliased.is_empty(),
        "the workflow help must not introduce visible aliases: {aliased:?}"
    );
}

#[test]
fn long_help_claims_no_writes_editors_or_release_surface() {
    let help = rendered_long_help().to_lowercase();
    for absent in ["vs code", "vscode", "release artifact", "installer"] {
        assert!(
            !help.contains(absent),
            "root long help stays inside the local workflow; found {absent:?}"
        );
    }
}
