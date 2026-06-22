use std::io::Write;
use std::path::PathBuf;

use clap::Args;
use shoreline::documents::unit_show_document;
use shoreline::model::RevisionId;
use shoreline::session::{
    EventVerificationPolicy, RevisionShowOptions, enrich_liveness, show_revision,
};

use crate::cli::json;

#[derive(Debug, Args)]
pub(super) struct ShowArgs {
    /// Repository root or a path inside the repository.
    #[arg(long, default_value = ".")]
    repo: PathBuf,

    /// The revision to show (a head seed): a current head resolves exactly; a
    /// superseded revision resolves its thread's current head, erroring when that
    /// thread has competing heads. Omitted shows the current capture.
    #[arg(long)]
    revision: Option<String>,

    /// Filter narrative facts to one review track.
    #[arg(long)]
    track: Option<String>,

    /// Hydrate body-like text from inline payloads or body artifacts.
    #[arg(long)]
    include_body: bool,

    /// Pretty-print the JSON response.
    #[arg(long, conflicts_with = "compact")]
    pretty: bool,

    /// Emit compact JSON explicitly.
    #[arg(long)]
    compact: bool,
}

pub(super) fn run(
    args: ShowArgs,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let span = tracing::info_span!("shore.review.show");
    let _entered = span.enter();
    tracing::debug!(command = "review.show", "command_start");

    let pretty = args.pretty;
    let result = show_revision(show_options(&args))?;

    // Liveness (merged/live/orphaned per OID + headline) is layered here, outside
    // the git-free document workflow: best-effort, omitted when reachability is
    // unknown.
    let liveness = enrich_liveness(&result.commit_range, &args.repo, None).ok();
    let document = unit_show_document(result);
    let mut value = serde_json::to_value(&document)?;
    if let Some(liveness) = liveness
        && let Some(commit_range) = value
            .get_mut("commitRange")
            .and_then(|cr| cr.as_object_mut())
    {
        commit_range.insert("liveness".to_owned(), serde_json::to_value(liveness)?);
    }
    json::write_json(stdout, &value, pretty)
}

fn show_options(args: &ShowArgs) -> RevisionShowOptions {
    let mut options = RevisionShowOptions::new(&args.repo).with_include_body(args.include_body);
    if let Some(revision) = &args.revision {
        options = options.with_revision_id(RevisionId::new(revision.clone()));
    }
    if let Some(track) = &args.track {
        options = options.with_track(track.clone());
    }
    if let Some(map) = super::common::discover_delegation_map(&args.repo) {
        options = options.with_delegation_map(map);
    }
    // Advisory policy + reader trust: enable the per-event verificationStatus +
    // endorsement readback, reader-relative; render-only, never a gate.
    options = options
        .with_trust_set(super::common::discover_trust_set(&args.repo))
        .with_verification_policy(EventVerificationPolicy::advisory())
        .with_actor_attributes(super::common::discover_actor_attributes(&args.repo));
    options
}
