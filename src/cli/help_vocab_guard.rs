//! Guard: two categories of internal vocabulary must never reach user-facing
//! clap help/about text. First, the retired review-unit work-object noun ("unit"
//! / "units", as in "review unit" / "the captured unit"); the domain word is
//! "revision", and post-sweep there are zero legitimate `unit`/`units` tokens in
//! clap help. Second, the substrate layering vocabulary that stays internal-only
//! per `docs/substrate-language.md`: `Engagement` is out of the command surface
//! entirely, and `WorkObjectProposed` (the sole abstract wire event name) and its
//! `TaskAttempt` payload arm name a cross-domain generative-move abstraction, not
//! a user-facing description of any one command's job. This rejects the bare
//! tokens — catching the whole retired family and any future reintroduction.
//! Complements the sibling identifier guard under `tests/` (which rejects the
//! retired snake/Pascal Rust identifier in `src/`), catching instead the display
//! strings surfaced in `--help` that the identifier guard cannot see.

use clap::CommandFactory;

/// Whole-word tokens (case-insensitive, alphanumeric-run split) that must never
/// appear in clap help/about text: the retired review-unit noun, plus the
/// substrate layering vocabulary that stays internal-only per
/// `docs/substrate-language.md` (`Engagement`) and the sole abstract wire event
/// name and its task-domain payload arm (`WorkObjectProposed`, `TaskAttempt`).
const RETIRED_VOCAB_TOKENS: &[&str] = &[
    "unit",
    "units",
    "engagement",
    "engagements",
    "workobjectproposed",
    "taskattempt",
];

/// True if `text` contains a whole-word retired-vocabulary token
/// (case-insensitive). Word-splitting on non-alphanumeric characters avoids
/// matching substrings such as "reunite"; the compound PascalCase tokens
/// (`WorkObjectProposed`, `TaskAttempt`) survive the split intact because they
/// carry no internal punctuation — each lowercases to a single contiguous run.
fn has_retired_vocab_token(text: &str) -> bool {
    text.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|token| RETIRED_VOCAB_TOKENS.contains(&token))
}

fn collect_help_text(cmd: &clap::Command, out: &mut Vec<(String, String)>) {
    let name = cmd.get_name().to_owned();
    if let Some(about) = cmd.get_about() {
        out.push((name.clone(), about.to_string()));
    }
    if let Some(long) = cmd.get_long_about() {
        out.push((name.clone(), long.to_string()));
    }
    for arg in cmd.get_arguments() {
        if let Some(help) = arg.get_help() {
            out.push((format!("{name} --{}", arg.get_id()), help.to_string()));
        }
        if let Some(help) = arg.get_long_help() {
            out.push((format!("{name} --{}", arg.get_id()), help.to_string()));
        }
    }
    for sub in cmd.get_subcommands() {
        collect_help_text(sub, out);
    }
}

#[test]
fn help_text_is_free_of_retired_vocabulary() {
    let cmd = super::Cli::command();
    let mut texts = Vec::new();
    collect_help_text(&cmd, &mut texts);

    let offenders: Vec<String> = texts
        .iter()
        .filter(|(_, text)| has_retired_vocab_token(text))
        .map(|(where_, text)| format!("{where_}: {text}"))
        .collect();
    assert!(
        offenders.is_empty(),
        "retired review-unit vocabulary (unit/units token) in clap help/about text:\n{}",
        offenders.join("\n")
    );
}
