use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use pointbreak::model::{InputRequestId, ObservationId, RevisionId, RevisionRefV1};
use pointbreak::session::event::{FactPortRelationV1, FactRefV1};
use pointbreak::session::{FactPortOptions, port_review_fact};

use crate::cli::common::{SignableOptions, SigningSkip};
use crate::cli::id_resolver::IdResolver;
use crate::cli::output;

#[derive(Debug, Args)]
pub(super) struct FactArgs {
    #[command(subcommand)]
    command: FactCommand,
}

#[derive(Debug, Subcommand)]
enum FactCommand {
    /// Record explicit context continuity from one exact Revision to another.
    Port(FactPortArgs),
}

#[derive(Debug, Args)]
struct FactPortArgs {
    #[arg(long, default_value = ".")]
    repo: PathBuf,

    /// Exact `REVISION_ID@OBJECT_ARTIFACT_SHA256` origin reference.
    #[arg(long)]
    origin_revision: String,

    /// Origin observation or input-request id.
    #[arg(long)]
    origin_fact: String,

    /// Exact safe target cursor emitted by `pointbreak change select` or capture.
    #[arg(long)]
    review_cursor: String,

    #[arg(long, value_enum)]
    relation: FactPortRelationArg,

    /// Required by reanchored-as and carried-open-as; forbidden otherwise.
    #[arg(long)]
    target_fact: Option<String>,

    #[arg(long)]
    rationale_content_hash: Option<String>,

    /// Review lane that records this continuity claim.
    #[arg(long)]
    track: String,

    #[arg(long)]
    sign_key: Option<String>,

    #[command(flatten)]
    format_args: output::FormatArgs,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "kebab-case")]
enum FactPortRelationArg {
    ContextOnly,
    ReanchoredAs,
    CarriedOpenAs,
    ResolvedBy,
}

impl From<FactPortRelationArg> for FactPortRelationV1 {
    fn from(value: FactPortRelationArg) -> Self {
        match value {
            FactPortRelationArg::ContextOnly => Self::ContextOnly,
            FactPortRelationArg::ReanchoredAs => Self::ReanchoredAs,
            FactPortRelationArg::CarriedOpenAs => Self::CarriedOpenAs,
            FactPortRelationArg::ResolvedBy => Self::ResolvedBy,
        }
    }
}

pub(super) fn run(
    args: FactArgs,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    match args.command {
        FactCommand::Port(args) => port(args, stdout, stderr),
    }
}

fn port(
    args: FactPortArgs,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let ids = IdResolver::new(&args.repo);
    let origin_revision = parse_revision_ref(&ids, &args.origin_revision)?;
    let origin_fact = parse_fact_ref(&ids, &args.origin_fact)?;
    let mut options = FactPortOptions::new(
        &args.repo,
        origin_revision,
        origin_fact,
        args.review_cursor,
        args.relation.into(),
        args.track,
    );
    if let Some(target_fact) = args.target_fact {
        options = options.with_target_fact(parse_fact_ref(&ids, &target_fact)?);
    }
    if let Some(hash) = args.rationale_content_hash {
        options = options.with_rationale_content_hash(hash);
    }
    let (options, skip) = apply_signer(options, &args.repo, args.sign_key.as_deref(), stderr);
    let result = port_review_fact(options)?;
    crate::cli::common::surface_best_effort_skip(&skip, stderr);
    let format = output::resolve_format(args.format_args.explicit(), output::OutputFormat::Json)?;
    let text = if result.created {
        format!("recorded {} fact port", result.port_id.as_str())
    } else {
        format!("fact port {} already recorded", result.port_id.as_str())
    };
    output::write_document(stdout, format, &result, || text)
}

fn parse_revision_ref(
    ids: &IdResolver,
    value: &str,
) -> Result<RevisionRefV1, Box<dyn std::error::Error>> {
    let (revision, artifact_hash) = value
        .split_once('@')
        .ok_or("--origin-revision must be REVISION_ID@OBJECT_ARTIFACT_SHA256")?;
    Ok(RevisionRefV1::new(
        RevisionId::new(ids.rev(revision)?),
        artifact_hash.to_owned(),
    )?)
}

fn parse_fact_ref(ids: &IdResolver, value: &str) -> Result<FactRefV1, Box<dyn std::error::Error>> {
    let fact = ids.fact(value)?;
    if fact.starts_with("obs:") {
        Ok(FactRefV1::Observation {
            observation_id: ObservationId::new(fact),
        })
    } else if fact.starts_with("input-request:") {
        Ok(FactRefV1::InputRequest {
            input_request_id: InputRequestId::new(fact),
        })
    } else {
        Err("fact must be an observation or input-request id".into())
    }
}

fn apply_signer<O: SignableOptions>(
    mut options: O,
    repo: &std::path::Path,
    sign_key: Option<&str>,
    stderr: &mut dyn Write,
) -> (O, SigningSkip) {
    let mut skip = None;
    if let Some(resolved) = crate::cli::common::resolve_and_surface_signer(repo, sign_key, stderr) {
        let (signed, signer_skip) = crate::cli::common::apply_resolved_signer(options, resolved);
        options = signed;
        skip = signer_skip;
    }
    (options, skip)
}
