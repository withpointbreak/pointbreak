//! Guard: the retired review-unit work-object vocabulary must not reappear in
//! user-facing clap help/about text. In this CLI the retired noun is "unit" (as
//! in "review unit" / "the captured unit"); the domain word is "revision".
//! Post-sweep there are zero legitimate `unit`/`units` tokens in clap help, so
//! this rejects the bare token — catching the whole retired family and any future
//! reintroduction. Complements the sibling identifier guard under `tests/` (which
//! rejects the retired snake/Pascal Rust identifier in `src/`), catching instead
//! the display strings surfaced in `--help` that the identifier guard cannot see.

use clap::CommandFactory;

/// True if `text` contains a whole-word `unit`/`units` token (case-insensitive).
/// Word-split avoids matching substrings such as "reunite".
fn has_retired_unit_token(text: &str) -> bool {
    text.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|token| token == "unit" || token == "units")
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
fn help_text_is_free_of_retired_unit_vocabulary() {
    let cmd = super::Cli::command();
    let mut texts = Vec::new();
    collect_help_text(&cmd, &mut texts);

    let offenders: Vec<String> = texts
        .iter()
        .filter(|(_, text)| has_retired_unit_token(text))
        .map(|(where_, text)| format!("{where_}: {text}"))
        .collect();
    assert!(
        offenders.is_empty(),
        "retired review-unit vocabulary (unit/units token) in clap help/about text:\n{}",
        offenders.join("\n")
    );
}
