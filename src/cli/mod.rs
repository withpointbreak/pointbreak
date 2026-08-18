use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

#[cfg(feature = "longitudinal-counting")]
use base64::Engine as _;
#[cfg(feature = "longitudinal-counting")]
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use clap::error::ErrorKind;
use clap::{Parser, Subcommand};

use crate::cli_tracing::TracingArgs;

mod assessment;
mod association;
mod attention;
mod capture;
mod change;
pub(crate) mod common;
mod derived_read;
mod diff;
mod endorse;
mod fact;
mod history;
mod id_resolver;
mod identity;
mod input_request;
mod inspect;
mod json;
mod key;
mod observation;
mod output;
mod revision;
mod store;
mod theme;
mod validation;
mod version;

#[cfg(test)]
mod about_bleed_guard;
#[cfg(test)]
mod help_hygiene_guard;
#[cfg(test)]
mod help_vocab_guard;
#[cfg(test)]
mod reference_coverage;
#[cfg(test)]
mod workflow_help_guard;

/// The root `--help` narrative: the five-stage review model over the existing
/// flattened families, the short first-review path, and direct recovery
/// pointers. Narrative only — it adds no command, alias, flag, or default, and
/// `workflow_help_guard` pins both the rendered content and that boundary.
const ROOT_LONG_ABOUT: &str = "\
Durable, local-first review record for code changes that humans and coding \
agents build together.

A review moves through five stages: Work -> Claims -> Evidence -> Questions -> Call.

  Work — what changed: capture, change, revision, inspect
  Claims — what an author or reviewer asserts: observation
  Evidence — what was checked: validation
  Questions — what still needs judgment: input-request
  Call — the current assessment: assessment

Across the stages, attention lists the outstanding judgment, and association
records where the reviewed work landed.

First review, from a real tracked change in a Git repository:

  pointbreak capture --summary \"<what changed>\"
  pointbreak inspect --open

That opens the local, read-only Review. The Getting Started guide continues
with the complete paired author/reviewer loop:
https://github.com/withpointbreak/pointbreak/blob/main/docs/getting-started.md

Recovery:
  wrong repository or store    pointbreak store paths --repo <repo> --format text
  migration required           inspect state with pointbreak change profile --repo <repo>
  find legacy captured work    pointbreak revision list, then pass --revision <id>
  select exact current work    pointbreak change select <change-id>
  replace an earlier call      pointbreak assessment add --replaces <assessment-id>
  commit landed after review   pointbreak change select <change-id> --revision <revision-id>
                               --source commit:<oid>, then association land with that cursor";

#[derive(Debug, Parser)]
#[command(
    name = "pointbreak",
    bin_name = "pointbreak",
    version = pointbreak::documents::VERSION_DISPLAY,
    about = "Durable, local-first review record for code changes",
    long_about = ROOT_LONG_ABOUT
)]
struct Cli {
    #[command(flatten)]
    tracing: TracingArgs,

    #[cfg(feature = "longitudinal-counting")]
    #[arg(long, global = true, hide = true)]
    longitudinal_counting: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[cfg(feature = "longitudinal-counting")]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LongitudinalCliCountingRequest {
    run_identity: String,
    context: pointbreak::bench_support::longitudinal::LongitudinalCounterReceiptContextV1,
    receipt_path: std::path::PathBuf,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Record and read the current review call (the Call stage)
    Assessment(Box<assessment::AssessmentArgs>),
    /// Record where reviewed work landed: commit and ref associations on the same revision
    Association(Box<association::AssociationArgs>),
    /// List what still needs an actor's judgment across the review record
    Attention(attention::AttentionArgs),
    Capture(capture::CaptureArgs),
    /// Read stable Changes and their exact captured Revisions
    Change(change::ChangeArgs),
    Diff(diff::DiffArgs),
    Endorse(endorse::EndorseArgs),
    /// Record explicit context continuity between exact Revisions.
    Fact(fact::FactArgs),
    History(history::HistoryArgs),
    /// Inspect actor identity, delegation, and attestations
    Identity(identity::IdentityArgs),
    /// Open, read, and respond to durable review questions (the Questions stage)
    InputRequest(Box<input_request::InputRequestArgs>),
    Inspect(inspect::InspectArgs),
    /// Manage signing keys, enrollment, and trust staging
    Key(key::KeyArgs),
    /// Record and read review claims (the Claims stage)
    Observation(Box<observation::ObservationArgs>),
    /// List and show captured revisions (the Work stage)
    Revision(revision::RevisionArgs),
    /// Inspect and manage the resolved Pointbreak store
    Store(store::StoreArgs),
    /// Record and read validation evidence (the Evidence stage)
    Validation(validation::ValidationArgs),
    Version(version::VersionArgs),
}

pub(crate) fn run_main() -> ExitCode {
    with_process_io(|stdout, stderr| run_with_io(std::env::args_os(), stdout, stderr))
}

fn with_process_io<T>(run: impl FnOnce(&mut dyn Write, &mut dyn Write) -> T) -> T {
    let mut stdout = std::io::stdout().lock();
    // Derived-access workers may emit diagnostics while command-local runtime
    // handles are being dropped. Let stderr lock per write so a worker can
    // finish before the main thread joins it during shutdown.
    let mut stderr = std::io::stderr();
    run(&mut stdout, &mut stderr)
}

fn run_with_io<I, S>(args: I, stdout: &mut dyn Write, stderr: &mut dyn Write) -> ExitCode
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let invalid_subcommand_hint = invalid_subcommand_hint(&args);
    let cli = match Cli::try_parse_from(args.clone()) {
        Ok(cli) => cli,
        Err(error) => {
            let exit = if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) {
                let _ = write!(stdout, "{error}");
                ExitCode::SUCCESS
            } else {
                let _ = writeln!(stderr, "{error}");
                if error.kind() == ErrorKind::InvalidSubcommand
                    && let Some(hint) = invalid_subcommand_hint
                {
                    let _ = writeln!(stderr, "\n{hint}");
                }
                ExitCode::FAILURE
            };
            return exit;
        }
    };

    #[cfg(feature = "longitudinal-counting")]
    {
        let mut cli = cli;
        if let Some(encoded) = cli.longitudinal_counting.take() {
            return run_counted_cli(cli, stdout, stderr, &encoded, &args);
        }
        result_exit_code(run_cli(cli, stdout, stderr, &args), stderr)
    }

    #[cfg(not(feature = "longitudinal-counting"))]
    match run_cli(cli, stdout, stderr, &args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(stderr, "{error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod process_io_tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn command_lifetime_does_not_hold_the_process_stderr_lock() {
        let (acquired_tx, acquired_rx) = mpsc::channel();
        let (worker, acquired_while_command_io_was_live) = with_process_io(|_, _| {
            let worker = std::thread::spawn(move || {
                let stderr = std::io::stderr();
                let _lock = stderr.lock();
                acquired_tx.send(()).expect("report acquired stderr lock");
            });
            let acquired = acquired_rx.recv_timeout(Duration::from_secs(2)).is_ok();
            (worker, acquired)
        });

        worker.join().expect("stderr-lock worker completes");
        assert!(
            acquired_while_command_io_was_live,
            "the command I/O lifetime must not hold stderr across a worker join"
        );
    }
}

/// Fence ordinary public product commands at the L2 capability boundary.
///
/// The `change` family owns the typed profile/migration-plan responses and its
/// writers already require L2. Store placement/key/version operations do not
/// consume Journal semantics. Inspector exposes the typed v2 profile while its
/// semantic routes apply their own capability gates. Every other public command
/// must refuse L0/M1 before its adapter can read or mutate domain state.
fn preflight_public_store_capability(
    cli: &Cli,
    args: &[OsString],
) -> Result<(), Box<dyn std::error::Error>> {
    let exempt = match &cli.command {
        Command::Change(_)
        | Command::Identity(_)
        | Command::Inspect(_)
        | Command::Key(_)
        | Command::Version(_) => true,
        Command::Store(args) => args.is_capability_exempt(),
        _ => false,
    };
    if exempt {
        return Ok(());
    }

    let repo = args
        .windows(2)
        .find(|window| window[0] == "--repo")
        .map(|window| std::path::PathBuf::from(&window[1]))
        .or_else(|| {
            args.iter().find_map(|arg| {
                arg.to_str()
                    .and_then(|arg| arg.strip_prefix("--repo="))
                    .map(std::path::PathBuf::from)
            })
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let Some(capability) = pointbreak::session::activated_store_capability_for_repo(repo)? else {
        return Err(
            "migration_required; this command requires an explicit completed store migration"
                .into(),
        );
    };
    match capability.status {
        pointbreak::session::StoreCapabilityStatus::Ready { .. } => Ok(()),
        pointbreak::session::StoreCapabilityStatus::MigrationRequired => Err(
            "migration_required; this command requires an explicit completed store migration"
                .into(),
        ),
        pointbreak::session::StoreCapabilityStatus::MigrationInProgress { .. } => {
            Err("migration_in_progress; this command refuses partial Change authority".into())
        }
    }
}

#[cfg(feature = "longitudinal-counting")]
fn result_exit_code(
    result: Result<(), Box<dyn std::error::Error>>,
    stderr: &mut dyn Write,
) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(stderr, "{error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(feature = "longitudinal-counting")]
fn run_counted_cli(
    cli: Cli,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    encoded: &str,
    raw_args: &[OsString],
) -> ExitCode {
    let request = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|error| error.to_string())
        .and_then(|bytes| {
            serde_json::from_slice::<LongitudinalCliCountingRequest>(&bytes)
                .map_err(|error| error.to_string())
        });
    let request = match request {
        Ok(request) => request,
        Err(error) => {
            let _ = writeln!(stderr, "invalid longitudinal counting request: {error}");
            return ExitCode::FAILURE;
        }
    };
    if !request.receipt_path.is_absolute() {
        let _ = writeln!(
            stderr,
            "longitudinal counting receipt path must be absolute"
        );
        return ExitCode::FAILURE;
    }
    let scope = match pointbreak::bench_support::longitudinal::LongitudinalCountingScopeV1::new(
        request.run_identity,
    ) {
        Ok(scope) => scope,
        Err(error) => {
            let _ = writeln!(stderr, "{error}");
            return ExitCode::FAILURE;
        }
    };
    let _guard = scope.enter();
    let mut counting_stdout = LongitudinalCountingWriter { inner: stdout };
    let result = run_cli(cli, &mut counting_stdout, stderr, raw_args);
    let mut context = request.context;
    context.success = result.is_ok();
    let receipt = scope
        .receipt(context)
        .map_err(|error| error.to_string())
        .and_then(|receipt| write_counting_receipt(&request.receipt_path, &receipt));
    if let Err(error) = receipt {
        let _ = writeln!(
            stderr,
            "could not write longitudinal counting receipt: {error}"
        );
        return ExitCode::FAILURE;
    }
    result_exit_code(result, stderr)
}

#[cfg(feature = "longitudinal-counting")]
fn write_counting_receipt(
    path: &std::path::Path,
    receipt: &pointbreak::bench_support::longitudinal::LongitudinalCounterReceiptV1,
) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    serde_json::to_writer(&mut file, receipt).map_err(|error| error.to_string())?;
    file.write_all(b"\n").map_err(|error| error.to_string())
}

#[cfg(feature = "longitudinal-counting")]
struct LongitudinalCountingWriter<'a> {
    inner: &'a mut dyn Write,
}

#[cfg(feature = "longitudinal-counting")]
impl Write for LongitudinalCountingWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(bytes)?;
        pointbreak::bench_support::longitudinal::record_response_bytes(written);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// A predicate over the raw argv that recognizes an invalid command path.
enum HintPredicate {
    /// The command path immediately after the program name. Unlike an adjacent
    /// window, this cannot match later argument values.
    LeadingPath(&'static [&'static str]),
    /// Two or three adjacent argv tokens, e.g. `["review", "revisions"]`.
    AdjacentWindow(&'static [&'static str]),
    /// The first non-flag argv token — the attempted subcommand. Used for the
    /// bare-family retirements, e.g. a stale `pointbreak review …`.
    LeadingToken(&'static str),
}

impl HintPredicate {
    fn matches(&self, tokens: &[&str]) -> bool {
        match self {
            HintPredicate::LeadingPath(path) => tokens
                .get(1..)
                .is_some_and(|command_args| command_args.starts_with(path)),
            HintPredicate::AdjacentWindow(seq) => tokens
                .windows(seq.len())
                .any(|window| window.iter().zip(seq.iter()).all(|(a, b)| a == b)),
            HintPredicate::LeadingToken(name) => tokens
                .iter()
                .skip(1) // skip the program name
                .find(|token| !token.starts_with('-'))
                .is_some_and(|token| token == name),
        }
    }
}

/// Invalid-subcommand recovery hints, evaluated in order (first match wins).
/// Keep specific path/window rows before general `LeadingToken` rows so a stale
/// `pointbreak review <verb>` gets the verb-specific hint rather than the family hint.
/// Family/rename tasks append rows; they never change this mechanism.
const INVALID_SUBCOMMAND_HINTS: &[(HintPredicate, &str)] = &[
    (
        HintPredicate::LeadingPath(&["assessment", "replace"]),
        "Use `pointbreak assessment add --replaces <assessment-id>`.",
    ),
    (
        HintPredicate::AdjacentWindow(&["identity", "enroll"]),
        "Use `pointbreak identity delegate <AGENT> --principal <P>`.",
    ),
    (
        HintPredicate::AdjacentWindow(&["review", "assessment"]),
        "Use `pointbreak assessment` instead of `pointbreak review assessment`.",
    ),
    // The association compounds collapsed to `record`/`withdraw`; the four
    // verb-specific triples must precede the family pair so they win first.
    (
        HintPredicate::AdjacentWindow(&["review", "association", "associate-commit"]),
        "Use `pointbreak association record --commit <oid>` (or `--ref <name> --head <oid>`).",
    ),
    (
        HintPredicate::AdjacentWindow(&["review", "association", "associate-ref"]),
        "Use `pointbreak association record --ref <name> --head <oid>` (or `--commit <oid>`).",
    ),
    (
        HintPredicate::AdjacentWindow(&["review", "association", "withdraw-commit"]),
        "Use `pointbreak association withdraw <ASSOCIATION_ID>`.",
    ),
    (
        HintPredicate::AdjacentWindow(&["review", "association", "withdraw-ref"]),
        "Use `pointbreak association withdraw <ASSOCIATION_ID>`.",
    ),
    (
        HintPredicate::AdjacentWindow(&["review", "association"]),
        "The `association` family is now top-level; use \
         `pointbreak association record|withdraw|list`.",
    ),
    (
        HintPredicate::AdjacentWindow(&["review", "capture"]),
        "Use `pointbreak capture` instead of `pointbreak review capture`.",
    ),
    (
        HintPredicate::AdjacentWindow(&["review", "endorse"]),
        "Use `pointbreak endorse` instead of `pointbreak review endorse`.",
    ),
    (
        HintPredicate::AdjacentWindow(&["review", "history"]),
        "Use `pointbreak history` instead of `pointbreak review history`.",
    ),
    (
        HintPredicate::AdjacentWindow(&["review", "input-request", "fetch"]),
        "Use `pointbreak input-request show <ID>`.",
    ),
    (
        HintPredicate::AdjacentWindow(&["input-request", "fetch"]),
        "Use `pointbreak input-request show <ID>`.",
    ),
    (
        HintPredicate::AdjacentWindow(&["review", "input-request"]),
        "The `input-request` family is now top-level; use \
         `pointbreak input-request open|list|show|respond`.",
    ),
    (
        HintPredicate::AdjacentWindow(&["review", "observation"]),
        "Use `pointbreak observation` instead of `pointbreak review observation`.",
    ),
    (
        HintPredicate::AdjacentWindow(&["review", "revisions"]),
        "Use `pointbreak revision list`.",
    ),
    (
        HintPredicate::AdjacentWindow(&["review", "show"]),
        "Use `pointbreak revision show [REVISION]`.",
    ),
    (
        HintPredicate::AdjacentWindow(&["review", "validation"]),
        "Use `pointbreak validation` instead of `pointbreak review validation`.",
    ),
    (
        HintPredicate::AdjacentWindow(&["review", "intervention"]),
        "Use `pointbreak input-request` instead of `pointbreak review intervention`.",
    ),
    (
        HintPredicate::AdjacentWindow(&["review", "lineage"]),
        "`pointbreak review lineage` is removed; use stable Change cursors with \
         `pointbreak capture --review-cursor <token> --advance replace|parallel`.",
    ),
    (
        HintPredicate::AdjacentWindow(&["review", "unit"]),
        "`pointbreak review unit` is removed; list with `pointbreak revision list` \
         and show one with `pointbreak revision show <revision>`.",
    ),
    // The catch-all for the retired `review` namespace; must stay LAST among the
    // review rows so every verb-specific window above wins first.
    (
        HintPredicate::LeadingToken("review"),
        "The `review` family flattened to the top level. Use `pointbreak capture`, \
         `pointbreak revision list`, `pointbreak revision show`, `pointbreak observation …`, etc.",
    ),
    (
        HintPredicate::LeadingToken("keys"),
        "The `keys` family is now `key`. Use `pointbreak key <sub>`.",
    ),
    // The legacy working-tree surfaces, retired end-to-end (ADR-0030 second
    // amendment). Bare `show` stays unassigned per ADR-0030 Decision 3.
    (
        HintPredicate::LeadingToken("dump"),
        "`pointbreak dump` is retired. Read a captured revision's diff with `pointbreak diff`, \
         inspect deeply with `pointbreak inspect`, or read the review record with \
         `pointbreak revision show` (add `--format text` for the digest).",
    ),
    (
        HintPredicate::LeadingToken("show"),
        "`pointbreak show` is retired. Read a captured revision's diff with `pointbreak diff`, \
         inspect deeply with `pointbreak inspect`, or read the review record with \
         `pointbreak revision show` (add `--format text` for the digest).",
    ),
    (
        HintPredicate::LeadingToken("notes"),
        "The `notes` family is retired and sidecar notes are no longer imported. \
         Record review facts with `pointbreak observation add` and read them with \
         `pointbreak revision show` or `pointbreak inspect`.",
    ),
];

/// A recovery hint surfaced after clap's invalid-subcommand error.
fn invalid_subcommand_hint(args: &[OsString]) -> Option<&'static str> {
    let tokens: Vec<&str> = args.iter().filter_map(|arg| arg.to_str()).collect();
    INVALID_SUBCOMMAND_HINTS
        .iter()
        .find(|(predicate, _)| predicate.matches(&tokens))
        .map(|(_, hint)| *hint)
}

fn run_cli(
    cli: Cli,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    raw_args: &[OsString],
) -> Result<(), Box<dyn std::error::Error>> {
    // The single validation boundary for the git-backend selector: every
    // subcommand flows through here, so an invalid `POINTBREAK_GIT_BACKEND`
    // surfaces one actionable error before any git operation runs.
    pointbreak::git::validate_backend_selector()?;
    crate::cli_tracing::init_tracing(&cli.tracing)?;
    preflight_public_store_capability(&cli, raw_args)?;

    let result = match cli.command {
        Command::Assessment(args) => assessment::run(*args, stdout, stderr),
        Command::Association(args) => association::run(*args, stdout, stderr),
        Command::Attention(args) => attention::run(args, stdout),
        Command::Capture(args) => capture::run(args, &cli.tracing, stdout, stderr),
        Command::Change(args) => change::run(args, stdout, stderr),
        Command::Diff(args) => diff::run(args, stdout),
        Command::Endorse(args) => endorse::run(args, stdout, stderr),
        Command::Fact(args) => fact::run(args, stdout, stderr),
        Command::History(args) => history::run(args, stdout),
        Command::Identity(args) => identity::run(args, stdout, stderr),
        Command::InputRequest(args) => input_request::run(*args, stdout, stderr),
        Command::Inspect(args) => inspect::run(args, stdout),
        Command::Key(args) => key::run(args, stdout),
        Command::Observation(args) => observation::run(*args, stdout, stderr),
        Command::Revision(args) => revision::run(args, stdout),
        Command::Store(args) => store::run(args, stdout, stderr),
        Command::Validation(args) => validation::run(args, stdout, stderr),
        Command::Version(args) => version::run(args, stdout),
    };
    for diagnostic in pointbreak::session::take_derived_write_diagnostics() {
        let _ = writeln!(stderr, "advisory: {}", diagnostic.message);
    }
    result
}

#[cfg(test)]
mod change_reader_cli_tests {
    use super::*;

    #[test]
    fn change_reader_commands_are_a_distinct_cold_cli_family() {
        for args in [
            vec!["pointbreak", "change", "profile"],
            vec!["pointbreak", "change", "list"],
            vec!["pointbreak", "change", "show", "change:sha256:one"],
            vec!["pointbreak", "change", "select", "change:sha256:one"],
            vec![
                "pointbreak",
                "change",
                "revision",
                "change:sha256:one",
                "rev:sha256:one",
                "--artifact-hash",
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ],
        ] {
            Cli::try_parse_from(args).expect("Change-capable command parses");
        }
    }

    #[test]
    fn cold_change_read_on_l0_emits_only_the_typed_migration_document() {
        let repo = tempfile::tempdir().unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(repo.path())
                .status()
                .unwrap()
                .success()
        );
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit = run_with_io(
            [
                "pointbreak",
                "change",
                "list",
                "--repo",
                repo.path().to_str().unwrap(),
                "--format",
                "json",
            ],
            &mut stdout,
            &mut stderr,
        );
        assert_eq!(
            exit,
            ExitCode::SUCCESS,
            "{}",
            String::from_utf8_lossy(&stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
        assert_eq!(value["schema"], "pointbreak.store-migration-required");
        assert_eq!(value["state"], "migration_required");
        assert_eq!(stdout.iter().filter(|byte| **byte == b'\n').count(), 1);
    }

    #[test]
    fn exact_review_command_refuses_l0_without_mutation() {
        let repo = tempfile::tempdir().unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(repo.path())
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("git")
                .args(["config", "user.name", "Pointbreak Test"])
                .current_dir(repo.path())
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("git")
                .args(["config", "user.email", "pointbreak@example.test"])
                .current_dir(repo.path())
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("git")
                .args(["config", "commit.gpgsign", "false"])
                .current_dir(repo.path())
                .status()
                .unwrap()
                .success()
        );
        std::fs::write(repo.path().join("sample.txt"), "base\n").unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["add", "sample.txt"])
                .current_dir(repo.path())
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("git")
                .args(["commit", "--quiet", "-m", "base"])
                .current_dir(repo.path())
                .status()
                .unwrap()
                .success()
        );
        std::fs::write(repo.path().join("sample.txt"), "changed\n").unwrap();
        let before = pointbreak::session::store_capability_for_repo(repo.path())
            .unwrap()
            .cursor;
        let revision = format!("rev:sha256:{}", "1".repeat(64));
        let args = vec![
            "pointbreak".to_owned(),
            "observation".to_owned(),
            "list".to_owned(),
            "--repo".to_owned(),
            repo.path().display().to_string(),
            "--exact-revision".to_owned(),
            revision,
        ];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert_eq!(
            run_with_io(args, &mut stdout, &mut stderr),
            ExitCode::FAILURE
        );
        assert!(stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&stderr).contains("migration_required"),
            "{}",
            String::from_utf8_lossy(&stderr)
        );
        let after = pointbreak::session::store_capability_for_repo(repo.path())
            .unwrap()
            .cursor;
        assert_eq!(before, after);
    }
}

#[cfg(all(test, feature = "longitudinal-counting"))]
mod longitudinal_counting_tests {
    use super::*;

    #[test]
    fn final_cli_writer_counts_only_successfully_emitted_stdout_bytes() {
        let scope = pointbreak::bench_support::longitudinal::LongitudinalCountingScopeV1::new(
            "9".repeat(64),
        )
        .expect("valid scope");
        let _guard = scope.enter();
        let mut output = Vec::new();
        let mut writer = LongitudinalCountingWriter { inner: &mut output };

        writer.write_all(b"pointbreak").expect("write output");
        writer.flush().expect("flush output");

        assert_eq!(output, b"pointbreak");
        assert_eq!(
            scope.snapshot().counters.response_bytes,
            b"pointbreak".len() as u64
        );
    }

    #[test]
    fn hidden_cli_transport_writes_a_disjoint_receipt_after_the_final_stdout_boundary() {
        let directory = tempfile::tempdir().expect("temporary receipt directory");
        let receipt_path = directory.path().join("receipt.json");
        let request = serde_json::json!({
            "runIdentity": "1".repeat(64),
            "context": {
                "rootIdentity": "2".repeat(64),
                "operation": "VERSION",
                "phase": "cold",
                "baseExecutionIdentitySha256": "3".repeat(64),
                "derivativeExecutionIdentitySha256": "4".repeat(64),
                "manifestSha256": "5".repeat(64),
                "scheduleSha256": "6".repeat(64),
                "success": false,
                "semanticResultSha256": "7".repeat(64),
                "includeCapacityOwnership": false
            },
            "receiptPath": receipt_path
        });
        let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&request).expect("request JSON"));
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit = run_with_io(
            [
                "pointbreak",
                "--longitudinal-counting",
                encoded.as_str(),
                "version",
                "--format",
                "json",
            ],
            &mut stdout,
            &mut stderr,
        );

        assert_eq!(
            exit,
            ExitCode::SUCCESS,
            "{}",
            String::from_utf8_lossy(&stderr)
        );
        let receipt: pointbreak::bench_support::longitudinal::LongitudinalCounterReceiptV1 =
            serde_json::from_slice(&std::fs::read(receipt_path).expect("receipt bytes"))
                .expect("receipt JSON");
        assert!(receipt.success);
        assert_eq!(receipt.operation, "VERSION");
        assert_eq!(receipt.counters.response_bytes, stdout.len() as u64);
        assert!(receipt.capacity_ownership.is_none());
    }
}
