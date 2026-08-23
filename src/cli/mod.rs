use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

#[cfg(feature = "longitudinal-counting")]
use base64::Engine as _;
#[cfg(feature = "longitudinal-counting")]
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use clap::error::ErrorKind;
use clap::{Parser, Subcommand};
#[cfg(feature = "longitudinal-counting")]
use sha2::{Digest as _, Sha256};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    interaction_context:
        Option<pointbreak::bench_support::longitudinal::InteractionPerformanceExpectedContextV1>,
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

/// Total admission classification for the invocation-scoped public-read seam.
///
/// The catalog is intentionally private to the binary: it classifies parsed
/// command semantics, while the session library will only receive an opaque
/// repository-bound context after a qualified route has been selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InvocationReadCatalogV1 {
    Qualified(InvocationReadRouteV1),
    LegacyPreflight(LegacyPreflightKindV1),
    Exempt(InvocationReadExemptV1),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InvocationReadRouteV1 {
    AssessmentCurrentResult,
    AssessmentCurrentSummary,
    InputRequestOpenAllTracks,
    ObservationReviewerList,
    ValidationReviewerList,
    AttentionCurrentOrFallback,
}

#[cfg_attr(not(feature = "longitudinal-counting"), allow(dead_code))]
struct QualifiedInvocationReadV1<'a> {
    route: InvocationReadRouteV1,
    repo: &'a std::path::Path,
    revision: Option<&'a str>,
    track: Option<&'a str>,
    explicit_format: Option<output::OutputFormat>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LegacyPreflightKindV1 {
    Unqualified,
    ExplicitExhaustive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InvocationReadExemptV1 {
    VersionControl,
    OwnCapabilityBoundary,
}

fn classify_invocation_read_v1(cli: &Cli) -> InvocationReadCatalogV1 {
    use InvocationReadCatalogV1::{Exempt, LegacyPreflight, Qualified};

    if let Some(qualified) = qualified_invocation_read_v1(cli) {
        return Qualified(qualified.route);
    }
    match &cli.command {
        Command::History(_) => LegacyPreflight(LegacyPreflightKindV1::ExplicitExhaustive),
        Command::Store(args) => args.invocation_read_catalog_v1(),
        Command::Version(_) => Exempt(InvocationReadExemptV1::VersionControl),
        Command::Change(_) | Command::Identity(_) | Command::Inspect(_) | Command::Key(_) => {
            Exempt(InvocationReadExemptV1::OwnCapabilityBoundary)
        }
        Command::Assessment(_)
        | Command::Association(_)
        | Command::Attention(_)
        | Command::Capture(_)
        | Command::Diff(_)
        | Command::Endorse(_)
        | Command::Fact(_)
        | Command::InputRequest(_)
        | Command::Observation(_)
        | Command::Revision(_)
        | Command::Validation(_) => LegacyPreflight(LegacyPreflightKindV1::Unqualified),
    }
}

fn qualified_invocation_read_v1(cli: &Cli) -> Option<QualifiedInvocationReadV1<'_>> {
    match &cli.command {
        Command::Assessment(args) => args.qualified_invocation_read_v1(),
        Command::Attention(args) => args.qualified_invocation_read_v1(),
        Command::InputRequest(args) => args.qualified_invocation_read_v1(),
        Command::Observation(args) => args.qualified_invocation_read_v1(),
        Command::Validation(args) => args.qualified_invocation_read_v1(),
        _ => None,
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
) -> Result<Option<pointbreak::session::PublicReadCommandContextV1>, Box<dyn std::error::Error>> {
    let catalog = classify_invocation_read_v1(cli);
    if matches!(catalog, InvocationReadCatalogV1::Exempt(_)) {
        return Ok(None);
    }
    if matches!(
        catalog,
        InvocationReadCatalogV1::Qualified(
            InvocationReadRouteV1::AssessmentCurrentResult
                | InvocationReadRouteV1::AssessmentCurrentSummary
                | InvocationReadRouteV1::InputRequestOpenAllTracks
                | InvocationReadRouteV1::ObservationReviewerList
                | InvocationReadRouteV1::ValidationReviewerList
                | InvocationReadRouteV1::AttentionCurrentOrFallback
        )
    ) {
        let qualified = qualified_invocation_read_v1(cli)
            .expect("the typed catalog preserves the qualified read shape");
        return Ok(Some(
            pointbreak::session::prepare_public_read_command_context_v1(qualified.repo)?,
        ));
    }
    #[cfg(feature = "longitudinal-counting")]
    let _phase = pointbreak::bench_support::longitudinal::enter_derived_access_phase_v1(
        pointbreak::bench_support::longitudinal::LongitudinalDerivedAccessPhaseV1::CliCapabilityPreflightH1,
    );

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
        pointbreak::session::StoreCapabilityStatus::Ready { .. } => Ok(None),
        pointbreak::session::StoreCapabilityStatus::MigrationRequired => Err(
            "migration_required; this command requires an explicit completed store migration"
                .into(),
        ),
        pointbreak::session::StoreCapabilityStatus::MigrationInProgress { .. } => {
            Err("migration_in_progress; this command refuses partial Change authority".into())
        }
    }
}

#[cfg(test)]
mod invocation_read_catalog_tests {
    use super::*;

    fn classify(arguments: &str) -> InvocationReadCatalogV1 {
        let cli =
            Cli::try_parse_from(std::iter::once("pointbreak").chain(arguments.split_whitespace()))
                .unwrap_or_else(|error| panic!("fixture {arguments:?} must parse: {error}"));
        classify_invocation_read_v1(&cli)
    }

    #[test]
    fn exact_semantic_shapes_are_the_only_qualified_routes() {
        use InvocationReadCatalogV1::{LegacyPreflight, Qualified};
        use InvocationReadRouteV1 as Route;

        for (arguments, expected) in [
            (
                "assessment show --repo /tmp/a --exact-revision rev:one --track agent:r",
                Route::AssessmentCurrentResult,
            ),
            (
                "assessment show --include-summary --track agent:r --exact-revision rev:one --repo /tmp/b --format json-pretty",
                Route::AssessmentCurrentSummary,
            ),
            (
                "input-request list --exact-revision rev:one --status open --format text",
                Route::InputRequestOpenAllTracks,
            ),
            (
                "observation list --track agent:r --exact-revision rev:one --format json",
                Route::ObservationReviewerList,
            ),
            (
                "validation list --exact-revision rev:one --track agent:r",
                Route::ValidationReviewerList,
            ),
            (
                "attention list --revision rev:one --repo /tmp/c --format text",
                Route::AttentionCurrentOrFallback,
            ),
        ] {
            assert_eq!(classify(arguments), Qualified(expected), "{arguments}");
        }

        for arguments in [
            "assessment show --exact-revision rev:one --track agent:r --all",
            "assessment show --revision rev:one --track agent:r",
            "input-request list --exact-revision rev:one --status all",
            "input-request list --exact-revision rev:one --include-body",
            "observation list --exact-revision rev:one --track agent:r --tag security",
            "validation list --exact-revision rev:one --track agent:r --status passed",
            "attention list",
        ] {
            assert_eq!(
                classify(arguments),
                LegacyPreflight(LegacyPreflightKindV1::Unqualified),
                "{arguments}",
            );
        }
    }

    #[test]
    fn presentation_and_repository_selection_do_not_change_membership() {
        use InvocationReadCatalogV1::Qualified;
        use InvocationReadRouteV1::AssessmentCurrentResult;

        for arguments in [
            "assessment show --exact-revision rev:one --track agent:r --repo /tmp/default",
            "assessment show --exact-revision rev:one --track agent:r --repo /tmp/a --format json",
            "assessment show --exact-revision rev:one --track agent:r --repo /tmp/b --format json-pretty",
            "assessment show --exact-revision rev:one --track agent:r --repo /tmp/c --format text",
        ] {
            assert_eq!(classify(arguments), Qualified(AssessmentCurrentResult));
        }
    }

    #[test]
    fn version_exempt_and_named_exhaustive_operations_are_distinct() {
        use InvocationReadCatalogV1::{Exempt, LegacyPreflight};

        assert_eq!(
            classify("version --format text"),
            Exempt(InvocationReadExemptV1::VersionControl),
        );
        for arguments in [
            "history --repo /tmp/repo",
            "store status --repo /tmp/repo",
            "store derived status --repo /tmp/repo",
            "store derived build --repo /tmp/repo",
            "store derived rebuild --repo /tmp/repo",
            "store migrate --repo /tmp/repo",
            "store remove --repo /tmp/repo --revision rev:one",
            "store compact --repo /tmp/repo --dry-run",
        ] {
            assert_eq!(
                classify(arguments),
                LegacyPreflight(LegacyPreflightKindV1::ExplicitExhaustive),
                "{arguments}",
            );
        }
    }

    #[test]
    fn every_store_subcommand_keeps_its_existing_preflight_posture() {
        use InvocationReadCatalogV1::{Exempt, LegacyPreflight};
        use InvocationReadExemptV1::OwnCapabilityBoundary;
        use LegacyPreflightKindV1::ExplicitExhaustive;

        for arguments in [
            "store paths --repo /tmp/repo",
            "store mode show --repo /tmp/repo",
            "store list",
            "store forget slug",
            "store unlink --repo /tmp/repo",
        ] {
            assert_eq!(
                classify(arguments),
                Exempt(OwnCapabilityBoundary),
                "{arguments}",
            );
        }
        for arguments in [
            "store status",
            "store derived status",
            "store derived build",
            "store derived rebuild",
            "store migrate",
            "store link --dry-run",
            "store remove --revision rev:one",
            "store gc --dry-run",
            "store compact --dry-run",
        ] {
            assert_eq!(
                classify(arguments),
                LegacyPreflight(ExplicitExhaustive),
                "{arguments}",
            );
        }
    }

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn diagnostic_classifier_uses_the_typed_catalog_for_frozen_routes() {
        use std::collections::BTreeMap;

        use pointbreak::bench_support::longitudinal::{
            InteractionExecutionIdentityV1, InteractionPerformanceExpectedContextV1,
            InteractionRouteV1, InteractionSetupExpectationV1,
        };

        let revision = format!("rev:sha256:{}", "1".repeat(64));
        let arguments = vec![
            "assessment".to_owned(),
            "show".to_owned(),
            "--track".to_owned(),
            "agent:reviewer".to_owned(),
            "--format".to_owned(),
            "json".to_owned(),
            "--exact-revision".to_owned(),
            revision.clone(),
            "--repo".to_owned(),
            "/tmp/reordered".to_owned(),
        ];
        let expected = InteractionPerformanceExpectedContextV1 {
            execution: InteractionExecutionIdentityV1 {
                source_commit: "a".repeat(40),
                source_tree: "b".repeat(40),
                cargo_lock_sha256: "c".repeat(64),
                binary_path: "/tmp/pointbreak".to_owned(),
                binary_sha256: "d".repeat(64),
                build_profile: "debug".to_owned(),
                rustc_version: "rustc test".to_owned(),
                features: vec!["longitudinal-counting".to_owned()],
            },
            route: InteractionRouteV1::AssessmentCurrentResult,
            arguments: arguments.clone(),
            setup_expectation: InteractionSetupExpectationV1::AuthoritativeReplay,
            fixture_identity_sha256: Some("e".repeat(64)),
            revision: Some(revision),
            track: Some("agent:reviewer".to_owned()),
            domain_actor: Some("actor:agent:test".to_owned()),
            expected_child_actors: BTreeMap::new(),
        };

        assert_eq!(
            interaction_route_for_arguments(&arguments, &expected).unwrap(),
            InteractionRouteV1::AssessmentCurrentResult,
        );
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
    run_counted_cli_with_dispatch(cli, stdout, stderr, encoded, raw_args, run_cli)
}

#[cfg(feature = "longitudinal-counting")]
fn run_counted_cli_with_dispatch<F>(
    cli: Cli,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    encoded: &str,
    raw_args: &[OsString],
    dispatch: F,
) -> ExitCode
where
    F: FnOnce(
        Cli,
        &mut dyn Write,
        &mut dyn Write,
        &[OsString],
    ) -> Result<(), Box<dyn std::error::Error>>,
{
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
    match request.receipt_path.try_exists() {
        Ok(false) => {}
        Ok(true) => {
            let _ = writeln!(stderr, "longitudinal counting receipt path already exists");
            return ExitCode::FAILURE;
        }
        Err(error) => {
            let _ = writeln!(
                stderr,
                "could not inspect longitudinal counting receipt path: {error}"
            );
            return ExitCode::FAILURE;
        }
    }
    let interaction = match request.interaction_context {
        Some(expected) => match validate_interaction_request(expected, raw_args, encoded) {
            Ok(interaction) => Some(interaction),
            Err(error) => {
                let _ = writeln!(stderr, "invalid interaction counting request: {error}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
    let scope = match pointbreak::bench_support::longitudinal::LongitudinalCountingScopeV1::new(
        request.run_identity,
    ) {
        Ok(scope) => scope,
        Err(error) => {
            let _ = writeln!(stderr, "{error}");
            return ExitCode::FAILURE;
        }
    };
    if let Some((_, route)) = &interaction {
        scope.record_observed_route_once(*route);
        scope.record_execution_actor_once(
            pointbreak::bench_support::longitudinal::InteractionActorV1::RequestReader,
        );
    }
    let _guard = scope.enter();
    let (result, semantic_result_sha256) = {
        let mut counting_stdout = LongitudinalCountingWriter::new(stdout);
        let result = dispatch(cli, &mut counting_stdout, stderr, raw_args);
        let semantic_result_sha256 = counting_stdout.semantic_result_sha256();
        (result, semantic_result_sha256)
    };
    if interaction.is_some() {
        scope.record_semantic_result_sha256_once(semantic_result_sha256);
        scope.record_outcome_once(result.is_ok(), i32::from(result.is_err()));
    }
    let receipt = match interaction {
        Some((expected, _)) => scope
            .interaction_receipt(expected)
            .map_err(|error| error.to_string())
            .and_then(|receipt| write_counting_receipt(&request.receipt_path, &receipt)),
        None => {
            let mut context = request.context;
            context.success = result.is_ok();
            scope
                .receipt(context)
                .map_err(|error| error.to_string())
                .and_then(|receipt| write_counting_receipt(&request.receipt_path, &receipt))
        }
    };
    if let Err(error) = receipt {
        if let Err(product_error) = &result {
            let _ = writeln!(stderr, "{product_error}");
        }
        let _ = writeln!(
            stderr,
            "could not write longitudinal counting receipt: {error}"
        );
        return ExitCode::FAILURE;
    }
    result_exit_code(result, stderr)
}

#[cfg(feature = "longitudinal-counting")]
fn validate_interaction_request(
    expected: pointbreak::bench_support::longitudinal::InteractionPerformanceExpectedContextV1,
    raw_args: &[OsString],
    encoded: &str,
) -> Result<
    (
        pointbreak::bench_support::longitudinal::InteractionPerformanceExpectedContextV1,
        pointbreak::bench_support::longitudinal::InteractionRouteV1,
    ),
    String,
> {
    expected.validate().map_err(|error| error.to_string())?;
    let arguments = interaction_product_arguments(raw_args, encoded)?;
    if arguments != expected.arguments {
        return Err("interaction arguments do not match the actual CLI argv".to_owned());
    }
    let route = interaction_route_for_arguments(&arguments, &expected)?;
    if route != expected.route {
        return Err("interaction route does not match the actual CLI argv".to_owned());
    }
    Ok((expected, route))
}

#[cfg(feature = "longitudinal-counting")]
fn interaction_product_arguments(
    raw_args: &[OsString],
    encoded: &str,
) -> Result<Vec<String>, String> {
    let mut arguments = Vec::new();
    let mut index = 1;
    let mut hidden_request_count = 0;
    while index < raw_args.len() {
        let argument = raw_args[index]
            .to_str()
            .ok_or_else(|| "interaction CLI argv must be UTF-8".to_owned())?;
        if argument == "--longitudinal-counting" {
            let value = raw_args
                .get(index + 1)
                .and_then(|value| value.to_str())
                .ok_or_else(|| "interaction counting option requires a UTF-8 value".to_owned())?;
            if value != encoded {
                return Err(
                    "interaction counting option does not match the decoded request".to_owned(),
                );
            }
            hidden_request_count += 1;
            index += 2;
            continue;
        }
        if let Some(value) = argument.strip_prefix("--longitudinal-counting=") {
            if value != encoded {
                return Err(
                    "interaction counting option does not match the decoded request".to_owned(),
                );
            }
            hidden_request_count += 1;
            index += 1;
            continue;
        }
        arguments.push(argument.to_owned());
        index += 1;
    }
    if hidden_request_count != 1 {
        return Err("interaction counting option must occur exactly once".to_owned());
    }
    Ok(arguments)
}

#[cfg(feature = "longitudinal-counting")]
fn interaction_route_for_arguments(
    arguments: &[String],
    expected: &pointbreak::bench_support::longitudinal::InteractionPerformanceExpectedContextV1,
) -> Result<pointbreak::bench_support::longitudinal::InteractionRouteV1, String> {
    use pointbreak::bench_support::longitudinal::InteractionRouteV1 as Route;

    let cli = Cli::try_parse_from(
        std::iter::once("pointbreak").chain(arguments.iter().map(String::as_str)),
    )
    .map_err(|_| "interaction CLI argv is not one of the seven frozen routes".to_owned())?;
    let (route, repo, revision, track) = if let Some(qualified) = qualified_invocation_read_v1(&cli)
    {
        if qualified.explicit_format != Some(output::OutputFormat::Json) {
            return Err("interaction CLI argv is not one of the seven frozen routes".to_owned());
        }
        let route = match qualified.route {
            InvocationReadRouteV1::AssessmentCurrentResult => Route::AssessmentCurrentResult,
            InvocationReadRouteV1::AssessmentCurrentSummary => Route::AssessmentCurrentSummary,
            InvocationReadRouteV1::InputRequestOpenAllTracks => Route::InputRequestOpenAllTracks,
            InvocationReadRouteV1::ObservationReviewerList => Route::ObservationReviewerList,
            InvocationReadRouteV1::ValidationReviewerList => Route::ValidationReviewerList,
            InvocationReadRouteV1::AttentionCurrentOrFallback => Route::AttentionCurrentOrFallback,
        };
        (
            route,
            Some(qualified.repo),
            qualified.revision,
            qualified.track,
        )
    } else if let Command::Version(args) = &cli.command
        && args.explicit_format_v1() == Some(output::OutputFormat::Json)
    {
        (Route::VersionJson, None, None, None)
    } else {
        return Err("interaction CLI argv is not one of the seven frozen routes".to_owned());
    };
    if let Some(repo) = repo
        && !repo.is_absolute()
    {
        return Err("interaction route repository path must be absolute".to_owned());
    }
    if route != expected.route {
        return Ok(route);
    }
    if revision != expected.revision.as_deref() {
        return Err("interaction route Revision does not match expected context".to_owned());
    }
    if track != expected.track.as_deref() {
        return Err("interaction route track does not match expected context".to_owned());
    }
    Ok(route)
}

#[cfg(feature = "longitudinal-counting")]
fn write_counting_receipt<T: serde::Serialize>(
    path: &std::path::Path,
    receipt: &T,
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
    semantic_sha256: Sha256,
}

#[cfg(feature = "longitudinal-counting")]
impl<'a> LongitudinalCountingWriter<'a> {
    fn new(inner: &'a mut dyn Write) -> Self {
        Self {
            inner,
            semantic_sha256: Sha256::new(),
        }
    }

    fn semantic_result_sha256(&self) -> String {
        format!("{:x}", self.semantic_sha256.clone().finalize())
    }
}

#[cfg(feature = "longitudinal-counting")]
impl Write for LongitudinalCountingWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(bytes)?;
        self.semantic_sha256.update(&bytes[..written]);
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
    let mut public_read_context = preflight_public_store_capability(&cli, raw_args)?;

    let result = match cli.command {
        Command::Assessment(args) => match public_read_context.take() {
            Some(context) => assessment::run_with_public_read_context(*args, context, stdout),
            None => assessment::run(*args, stdout, stderr),
        },
        Command::Association(args) => association::run(*args, stdout, stderr),
        Command::Attention(args) => match public_read_context.take() {
            Some(context) => attention::run_with_public_read_context(args, context, stdout),
            None => attention::run(args, stdout),
        },
        Command::Capture(args) => capture::run(args, &cli.tracing, stdout, stderr),
        Command::Change(args) => change::run(args, stdout, stderr),
        Command::Diff(args) => diff::run(args, stdout),
        Command::Endorse(args) => endorse::run(args, stdout, stderr),
        Command::Fact(args) => fact::run(args, stdout, stderr),
        Command::History(args) => history::run(args, stdout),
        Command::Identity(args) => identity::run(args, stdout, stderr),
        Command::InputRequest(args) => match public_read_context.take() {
            Some(context) => input_request::run_with_public_read_context(*args, context, stdout),
            None => input_request::run(*args, stdout, stderr),
        },
        Command::Inspect(args) => inspect::run(args, stdout),
        Command::Key(args) => key::run(args, stdout),
        Command::Observation(args) => match public_read_context.take() {
            Some(context) => observation::run_with_public_read_context(*args, context, stdout),
            None => observation::run(*args, stdout, stderr),
        },
        Command::Revision(args) => revision::run(args, stdout),
        Command::Store(args) => store::run(args, stdout, stderr),
        Command::Validation(args) => match public_read_context.take() {
            Some(context) => validation::run_with_public_read_context(args, context, stdout),
            None => validation::run(args, stdout, stderr),
        },
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

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn diagnostic_h1_preserves_the_l0_refusal_and_attributes_only_the_real_preflight() {
        use pointbreak::bench_support::longitudinal::{
            InteractionActorV1, LongitudinalCountingScopeV1, LongitudinalDerivedAccessPhaseV1,
        };

        let repo = tempfile::tempdir().unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(repo.path())
                .status()
                .unwrap()
                .success()
        );
        let revision = format!("rev:sha256:{}", "1".repeat(64));
        let raw_args = vec![
            OsString::from("pointbreak"),
            OsString::from("observation"),
            OsString::from("list"),
            OsString::from("--repo"),
            repo.path().as_os_str().to_owned(),
            OsString::from("--exact-revision"),
            OsString::from(revision),
            OsString::from("--format"),
            OsString::from("json"),
        ];
        let ordinary = Cli::try_parse_from(raw_args.clone()).unwrap();
        let ordinary_error = preflight_public_store_capability(&ordinary, &raw_args)
            .err()
            .expect("L0 refusal")
            .to_string();
        let diagnostic = Cli::try_parse_from(raw_args.clone()).unwrap();
        let counting = LongitudinalCountingScopeV1::new("7".repeat(64)).unwrap();
        counting.record_execution_actor_once(InteractionActorV1::RequestReader);
        let _guard = counting.enter();

        let diagnostic_error = preflight_public_store_capability(&diagnostic, &raw_args)
            .err()
            .expect("same L0 refusal")
            .to_string();

        assert_eq!(diagnostic_error, ordinary_error);
        assert_eq!(
            counting
                .snapshot()
                .derived_access_phases
                .iter()
                .map(|sample| sample.phase)
                .collect::<Vec<_>>(),
            vec![LongitudinalDerivedAccessPhaseV1::CliCapabilityPreflightH1]
        );
    }

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn qualified_attention_l0_refuses_without_entering_legacy_h1() {
        use pointbreak::bench_support::longitudinal::{
            InteractionActorV1, LongitudinalCountingScopeV1,
        };

        let repo = tempfile::tempdir().unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(repo.path())
                .status()
                .unwrap()
                .success()
        );
        let raw_args = vec![
            OsString::from("pointbreak"),
            OsString::from("attention"),
            OsString::from("list"),
            OsString::from("--repo"),
            repo.path().as_os_str().to_owned(),
            OsString::from("--revision"),
            OsString::from(format!("rev:sha256:{}", "1".repeat(64))),
            OsString::from("--format"),
            OsString::from("json"),
        ];
        let cli = Cli::try_parse_from(raw_args.clone()).unwrap();
        let counting = LongitudinalCountingScopeV1::new("8".repeat(64)).unwrap();
        counting.record_execution_actor_once(InteractionActorV1::RequestReader);
        let _guard = counting.enter();

        let error = preflight_public_store_capability(&cli, &raw_args)
            .err()
            .expect("L0 refusal")
            .to_string();

        assert!(error.contains("migration_required"), "{error}");
        let snapshot = counting.snapshot();
        assert!(snapshot.derived_access_phases.is_empty());
        assert_eq!(snapshot.counters.directory_entries_walked, 0);
        assert_eq!(snapshot.counters.event_decodes, 0);
    }
}

#[cfg(all(test, feature = "longitudinal-counting"))]
mod longitudinal_counting_tests {
    use std::io::{Error, ErrorKind};

    use sha2::{Digest, Sha256};

    use super::*;

    fn legacy_context() -> serde_json::Value {
        serde_json::json!({
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
        })
    }

    fn interaction_request(receipt_path: &std::path::Path) -> serde_json::Value {
        serde_json::json!({
            "runIdentity": "1".repeat(64),
            "context": legacy_context(),
            "interactionContext": {
                "execution": {
                    "sourceCommit": "a".repeat(40),
                    "sourceTree": "b".repeat(40),
                    "cargoLockSha256": "c".repeat(64),
                    "binaryPath": "/tmp/pointbreak-interaction-test",
                    "binarySha256": "d".repeat(64),
                    "buildProfile": "debug",
                    "rustcVersion": "rustc test",
                    "features": ["gix", "longitudinal-counting"]
                },
                "route": "version_json",
                "arguments": ["version", "--format", "json"],
                "setupExpectation": "not_applicable"
            },
            "receiptPath": receipt_path
        })
    }

    fn sha256(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    struct PartialThenErrorWriter {
        accepted: Vec<u8>,
        writes: usize,
    }

    impl Write for PartialThenErrorWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.writes += 1;
            if self.writes == 1 {
                let accepted = bytes.len().min(3);
                self.accepted.extend_from_slice(&bytes[..accepted]);
                Ok(accepted)
            } else {
                Err(Error::new(
                    ErrorKind::BrokenPipe,
                    "fixture writer rejected bytes",
                ))
            }
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn final_cli_writer_counts_only_successfully_emitted_stdout_bytes() {
        let scope = pointbreak::bench_support::longitudinal::LongitudinalCountingScopeV1::new(
            "9".repeat(64),
        )
        .expect("valid scope");
        let _guard = scope.enter();
        let mut output = Vec::new();
        let mut writer = LongitudinalCountingWriter::new(&mut output);

        writer.write_all(b"pointbreak").expect("write output");
        writer.flush().expect("flush output");

        assert_eq!(output, b"pointbreak");
        assert_eq!(
            scope.snapshot().counters.response_bytes,
            b"pointbreak".len() as u64
        );
    }

    #[test]
    fn counted_writer_hashes_only_each_accepted_prefix_and_preserves_errors() {
        let scope = pointbreak::bench_support::longitudinal::LongitudinalCountingScopeV1::new(
            "8".repeat(64),
        )
        .expect("valid scope");
        let _guard = scope.enter();
        let mut output = PartialThenErrorWriter {
            accepted: Vec::new(),
            writes: 0,
        };
        let mut writer = LongitudinalCountingWriter::new(&mut output);

        assert_eq!(writer.write(b"abcdef").expect("partial write"), 3);
        let error = writer.write(b"def").expect_err("writer error");
        assert_eq!(error.kind(), ErrorKind::BrokenPipe);
        assert_eq!(error.to_string(), "fixture writer rejected bytes");
        assert_eq!(writer.semantic_result_sha256(), sha256(b"abc"));
        assert_eq!(scope.snapshot().counters.response_bytes, 3);
        assert_eq!(output.accepted, b"abc");
    }

    #[test]
    fn interaction_transport_binds_actual_route_actor_outcome_and_accepted_stdout() {
        let directory = tempfile::tempdir().expect("temporary receipt directory");
        let receipt_path = directory.path().join("interaction-receipt.json");
        let request = interaction_request(&receipt_path);
        let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&request).expect("request JSON"));
        let raw_args = vec![
            OsString::from("pointbreak"),
            OsString::from("--longitudinal-counting"),
            OsString::from(&encoded),
            OsString::from("version"),
            OsString::from("--format"),
            OsString::from("json"),
        ];
        let mut cli = Cli::try_parse_from(raw_args.clone()).expect("valid CLI");
        assert_eq!(
            cli.longitudinal_counting.take().as_deref(),
            Some(encoded.as_str())
        );
        let product_stdout = b"{\"ok\":true}\n";
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit = run_counted_cli_with_dispatch(
            cli,
            &mut stdout,
            &mut stderr,
            &encoded,
            &raw_args,
            |_, stdout, _, _| {
                pointbreak::bench_support::longitudinal::LongitudinalCountingScopeV1::current()
                    .expect("active interaction scope")
                    .record_observed_route_state_once(
                        pointbreak::bench_support::longitudinal::InteractionObservedRouteStateV1::NotApplicable,
                    );
                stdout.write_all(product_stdout)?;
                Ok(())
            },
        );

        assert_eq!(
            exit,
            ExitCode::SUCCESS,
            "{}",
            String::from_utf8_lossy(&stderr)
        );
        assert_eq!(stdout, product_stdout);
        let receipt: pointbreak::bench_support::longitudinal::InteractionPerformanceReceiptV1 =
            serde_json::from_slice(&std::fs::read(&receipt_path).expect("receipt bytes"))
                .expect("receipt JSON");
        receipt.validate().expect("valid interaction receipt");
        assert_eq!(
            receipt.observed.route,
            pointbreak::bench_support::longitudinal::InteractionRouteV1::VersionJson
        );
        assert_eq!(
            receipt.observed.execution_actor,
            pointbreak::bench_support::longitudinal::InteractionActorV1::RequestReader
        );
        assert!(receipt.observed.success);
        assert_eq!(receipt.observed.exit_code, 0);
        assert_eq!(
            receipt.observed.semantic_result_sha256,
            sha256(product_stdout)
        );
    }

    #[test]
    fn interaction_transport_rejects_expected_arguments_that_do_not_match_actual_argv() {
        let directory = tempfile::tempdir().expect("temporary receipt directory");
        let receipt_path = directory.path().join("interaction-receipt.json");
        let mut request = interaction_request(&receipt_path);
        request["interactionContext"]["arguments"] = serde_json::json!(["version"]);
        let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&request).expect("request JSON"));
        let raw_args = vec![
            OsString::from("pointbreak"),
            OsString::from("--longitudinal-counting"),
            OsString::from(&encoded),
            OsString::from("version"),
            OsString::from("--format"),
            OsString::from("json"),
        ];
        let mut cli = Cli::try_parse_from(raw_args.clone()).expect("valid CLI");
        cli.longitudinal_counting.take();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit = run_counted_cli_with_dispatch(
            cli,
            &mut stdout,
            &mut stderr,
            &encoded,
            &raw_args,
            |_, _, _, _| panic!("mismatched route must fail before product dispatch"),
        );

        assert_eq!(exit, ExitCode::FAILURE);
        assert!(stdout.is_empty());
        assert!(!receipt_path.exists());
        assert!(String::from_utf8_lossy(&stderr).contains("arguments do not match"));
    }

    #[test]
    fn interaction_transport_rejects_an_expected_route_that_does_not_match_actual_argv() {
        let directory = tempfile::tempdir().expect("temporary receipt directory");
        let receipt_path = directory.path().join("interaction-receipt.json");
        let mut request = interaction_request(&receipt_path);
        request["interactionContext"]["route"] = serde_json::json!("assessment_current_result");
        request["interactionContext"]["setupExpectation"] =
            serde_json::json!("authoritative_replay");
        request["interactionContext"]["fixtureIdentitySha256"] = serde_json::json!("e".repeat(64));
        request["interactionContext"]["revision"] =
            serde_json::json!(format!("rev:sha256:{}", "f".repeat(64)));
        request["interactionContext"]["track"] = serde_json::json!("agent:reviewer");
        request["interactionContext"]["domainActor"] = serde_json::json!("actor:agent:test");
        let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&request).expect("request JSON"));
        let raw_args = vec![
            OsString::from("pointbreak"),
            OsString::from("--longitudinal-counting"),
            OsString::from(&encoded),
            OsString::from("version"),
            OsString::from("--format"),
            OsString::from("json"),
        ];
        let mut cli = Cli::try_parse_from(raw_args.clone()).expect("valid CLI");
        cli.longitudinal_counting.take();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit = run_counted_cli_with_dispatch(
            cli,
            &mut stdout,
            &mut stderr,
            &encoded,
            &raw_args,
            |_, _, _, _| panic!("mismatched route must fail before product dispatch"),
        );

        assert_eq!(exit, ExitCode::FAILURE);
        assert!(stdout.is_empty());
        assert!(!receipt_path.exists());
        assert!(String::from_utf8_lossy(&stderr).contains("route does not match"));
    }

    #[test]
    fn interaction_transport_rejects_caller_supplied_observed_facts() {
        let directory = tempfile::tempdir().expect("temporary receipt directory");
        let receipt_path = directory.path().join("interaction-receipt.json");
        let mut request = interaction_request(&receipt_path);
        request["interactionContext"]["observed"] = serde_json::json!({
            "success": true,
            "semanticResultSha256": "0".repeat(64)
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

        assert_eq!(exit, ExitCode::FAILURE);
        assert!(stdout.is_empty());
        assert!(!receipt_path.exists());
        assert!(String::from_utf8_lossy(&stderr).contains("unknown field"));
    }

    #[test]
    fn interaction_transport_refuses_a_preexisting_receipt_before_dispatch() {
        let directory = tempfile::tempdir().expect("temporary receipt directory");
        let receipt_path = directory.path().join("interaction-receipt.json");
        std::fs::write(&receipt_path, b"preserve me\n").expect("preexisting receipt");
        let request = interaction_request(&receipt_path);
        let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&request).expect("request JSON"));
        let raw_args = vec![
            OsString::from("pointbreak"),
            OsString::from("--longitudinal-counting"),
            OsString::from(&encoded),
            OsString::from("version"),
            OsString::from("--format"),
            OsString::from("json"),
        ];
        let mut cli = Cli::try_parse_from(raw_args.clone()).expect("valid CLI");
        cli.longitudinal_counting.take();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit = run_counted_cli_with_dispatch(
            cli,
            &mut stdout,
            &mut stderr,
            &encoded,
            &raw_args,
            |_, _, _, _| panic!("preexisting path must fail before product dispatch"),
        );

        assert_eq!(exit, ExitCode::FAILURE);
        assert!(stdout.is_empty());
        assert_eq!(
            std::fs::read(&receipt_path).expect("preserved receipt"),
            b"preserve me\n"
        );
        assert!(String::from_utf8_lossy(&stderr).contains("already exists"));
    }

    #[test]
    fn interaction_receipt_validation_failure_never_publishes_a_partial_receipt() {
        let directory = tempfile::tempdir().expect("temporary receipt directory");
        let receipt_path = directory.path().join("interaction-receipt.json");
        let request = interaction_request(&receipt_path);
        let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&request).expect("request JSON"));
        let raw_args = vec![
            OsString::from("pointbreak"),
            OsString::from("--longitudinal-counting"),
            OsString::from(&encoded),
            OsString::from("version"),
            OsString::from("--format"),
            OsString::from("json"),
        ];
        let mut cli = Cli::try_parse_from(raw_args.clone()).expect("valid CLI");
        cli.longitudinal_counting.take();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit = run_counted_cli_with_dispatch(
            cli,
            &mut stdout,
            &mut stderr,
            &encoded,
            &raw_args,
            |_, stdout, _, _| {
                stdout.write_all(b"product output\n")?;
                Ok(())
            },
        );

        assert_eq!(exit, ExitCode::FAILURE);
        assert_eq!(stdout, b"product output\n");
        assert!(!receipt_path.exists());
        assert!(String::from_utf8_lossy(&stderr).contains("observed route state"));
    }

    #[test]
    fn interaction_receipt_failure_preserves_the_product_error_first() {
        let directory = tempfile::tempdir().expect("temporary receipt directory");
        let receipt_path = directory.path().join("interaction-receipt.json");
        let request = interaction_request(&receipt_path);
        let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&request).expect("request JSON"));
        let raw_args = vec![
            OsString::from("pointbreak"),
            OsString::from("--longitudinal-counting"),
            OsString::from(&encoded),
            OsString::from("version"),
            OsString::from("--format"),
            OsString::from("json"),
        ];
        let mut cli = Cli::try_parse_from(raw_args.clone()).expect("valid CLI");
        cli.longitudinal_counting.take();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit = run_counted_cli_with_dispatch(
            cli,
            &mut stdout,
            &mut stderr,
            &encoded,
            &raw_args,
            |_, _, _, _| Err("product failed exactly".into()),
        );

        assert_eq!(exit, ExitCode::FAILURE);
        assert!(stdout.is_empty());
        assert!(!receipt_path.exists());
        let stderr = String::from_utf8(stderr).expect("UTF-8 stderr");
        assert!(stderr.starts_with("product failed exactly\n"), "{stderr}");
        assert!(
            stderr.contains("could not write longitudinal counting receipt:"),
            "{stderr}"
        );
    }

    #[test]
    fn interaction_transport_records_the_real_product_error_outcome() {
        let directory = tempfile::tempdir().expect("temporary receipt directory");
        let receipt_path = directory.path().join("interaction-receipt.json");
        let request = interaction_request(&receipt_path);
        let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&request).expect("request JSON"));
        let raw_args = vec![
            OsString::from("pointbreak"),
            OsString::from("--longitudinal-counting"),
            OsString::from(&encoded),
            OsString::from("version"),
            OsString::from("--format"),
            OsString::from("json"),
        ];
        let mut cli = Cli::try_parse_from(raw_args.clone()).expect("valid CLI");
        cli.longitudinal_counting.take();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit = run_counted_cli_with_dispatch(
            cli,
            &mut stdout,
            &mut stderr,
            &encoded,
            &raw_args,
            |_, _, _, _| {
                pointbreak::bench_support::longitudinal::LongitudinalCountingScopeV1::current()
                    .expect("active interaction scope")
                    .record_observed_route_state_once(
                        pointbreak::bench_support::longitudinal::InteractionObservedRouteStateV1::NotApplicable,
                    );
                Err("product failed exactly".into())
            },
        );

        assert_eq!(exit, ExitCode::FAILURE);
        assert!(stdout.is_empty());
        assert!(String::from_utf8_lossy(&stderr).contains("product failed exactly"));
        let receipt: pointbreak::bench_support::longitudinal::InteractionPerformanceReceiptV1 =
            serde_json::from_slice(&std::fs::read(&receipt_path).expect("receipt bytes"))
                .expect("receipt JSON");
        assert!(!receipt.observed.success);
        assert_eq!(receipt.observed.exit_code, 1);
        assert_eq!(receipt.observed.semantic_result_sha256, sha256(b""));
    }

    #[test]
    fn interaction_transport_rejects_a_relative_receipt_path() {
        let mut request = interaction_request(std::path::Path::new("relative-receipt.json"));
        request["receiptPath"] = serde_json::json!("relative-receipt.json");
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

        assert_eq!(exit, ExitCode::FAILURE);
        assert!(stdout.is_empty());
        assert!(String::from_utf8_lossy(&stderr).contains("must be absolute"));
    }

    #[test]
    fn hidden_cli_transport_writes_a_disjoint_receipt_after_the_final_stdout_boundary() {
        let directory = tempfile::tempdir().expect("temporary receipt directory");
        let receipt_path = directory.path().join("receipt.json");
        let request = serde_json::json!({
            "runIdentity": "1".repeat(64),
            "context": legacy_context(),
            "receiptPath": receipt_path
        });
        let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&request).expect("request JSON"));
        let raw_args = vec![
            OsString::from("pointbreak"),
            OsString::from("--longitudinal-counting"),
            OsString::from(&encoded),
            OsString::from("version"),
            OsString::from("--format"),
            OsString::from("json"),
        ];
        let mut cli = Cli::try_parse_from(raw_args.clone()).expect("valid CLI");
        cli.longitudinal_counting.take();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit = run_counted_cli_with_dispatch(
            cli,
            &mut stdout,
            &mut stderr,
            &encoded,
            &raw_args,
            |cli, stdout, _, _| match cli.command {
                Command::Version(args) => version::run(args, stdout),
                _ => unreachable!("version fixture parsed another command"),
            },
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

    #[test]
    fn ordinary_version_invocations_are_byte_and_outcome_stable_without_a_hidden_request() {
        fn run_ordinary_version() -> (Result<(), String>, Vec<u8>) {
            let cli = Cli::try_parse_from(["pointbreak", "version", "--format", "json"])
                .expect("ordinary version CLI");
            assert!(cli.longitudinal_counting.is_none());
            let mut stdout = Vec::new();
            let result = match cli.command {
                Command::Version(args) => version::run(args, &mut stdout),
                _ => unreachable!("version fixture parsed another command"),
            }
            .map_err(|error| error.to_string());
            (result, stdout)
        }

        let (first_result, first_stdout) = run_ordinary_version();
        let (second_result, second_stdout) = run_ordinary_version();

        assert_eq!(first_result, second_result);
        assert_eq!(first_stdout, second_stdout);
        assert!(first_result.is_ok());
    }
}
