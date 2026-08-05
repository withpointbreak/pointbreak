use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use pointbreak::documents::{
    AssociationComparisonDocumentV1, AssociationComparisonRefV1, AssociationComparisonStateV1,
    AssociationProofAvailabilityV1, ChangeDocumentFacadeV1, ChangeQueryUnavailableDocumentV1,
    ContentAvailabilityV1, FactFamilyStateV1, FactPresentationV1, ReaderProfileDocumentV1,
    RevisionInterdiffAvailabilityV1, RevisionInterdiffDocumentV1, RevisionInterdiffRefV1,
    RevisionResourceDocumentV1, RevisionResourceProjectionV1, RevisionResourceRefV1,
    revision_show_document_v3,
};
use pointbreak::model::{ChangeId, RevisionId, RevisionRefV1};
use pointbreak::session::{
    AssessmentRecordStatus, ChangeReaderReadyV1, ChangeReaderStateV1, ObservationStatus,
    ReviewCursorV1, ReviewSourceBindingV1, RevisionShowOptions, SnapshotContentState,
    change_reader_state_for_repo, select_review_cursor, show_revision_for_change_reader_ready,
    validate_review_cursor_for_write,
};

use crate::cli::output;

#[derive(Debug, Args)]
pub(super) struct ChangeArgs {
    #[command(subcommand)]
    command: ChangeCommand,
}

#[derive(Debug, Subcommand)]
enum ChangeCommand {
    /// Report the store capability before any Change payload is read
    Profile(ReadArgs),
    /// List stable Changes
    List(ReadArgs),
    /// List Changes that still require judgment
    Attention(ReadArgs),
    /// Show one stable Change and every exact current candidate
    Show(ChangeReadArgs),
    /// Select one exact current Revision and emit a self-hashed review cursor
    Select(SelectArgs),
    /// Show one exact Revision in a named Change context
    Revision(ExactReadArgs),
    /// Read the immutable captured resource for one exact Revision
    Resource(ExactReadArgs),
    /// Describe a separately identified comparison between two exact Revisions
    Interdiff(InterdiffArgs),
}

#[derive(Debug, Args)]
struct ReadArgs {
    /// Repository root or a path inside the repository.
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    #[command(flatten)]
    format_args: output::FormatArgs,
}

#[derive(Debug, Args)]
struct ChangeReadArgs {
    change: String,
    /// Repository root or a path inside the repository.
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    #[command(flatten)]
    format_args: output::FormatArgs,
}

#[derive(Debug, Args)]
struct SelectArgs {
    change: String,
    /// Select this exact Revision. Required when the Change has multiple current Revisions.
    #[arg(long)]
    revision: Option<String>,
    /// Permit a captured-resource cursor over a non-current member.
    #[arg(long)]
    allow_historical: bool,
    /// Revalidate a previously emitted cursor against the current Change graph before selecting.
    #[arg(long)]
    cursor: Option<String>,
    /// Repository root or a path inside the repository.
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    #[command(flatten)]
    format_args: output::FormatArgs,
}

#[derive(Debug, Args)]
struct ExactReadArgs {
    change: String,
    revision: String,
    /// Exact object-artifact hash from the selected RevisionRefV1.
    #[arg(long)]
    artifact_hash: String,
    /// Hydrate body-like fact text inside the exact Revision document.
    #[arg(long)]
    include_body: bool,
    /// Repository root or a path inside the repository.
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    #[command(flatten)]
    format_args: output::FormatArgs,
}

#[derive(Debug, Args)]
struct InterdiffArgs {
    change: String,
    from: String,
    to: String,
    #[arg(long)]
    from_artifact_hash: String,
    #[arg(long)]
    to_artifact_hash: String,
    /// Repository root or a path inside the repository.
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    #[command(flatten)]
    format_args: output::FormatArgs,
}

pub(super) fn run(
    args: ChangeArgs,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    match args.command {
        ChangeCommand::Profile(args) => {
            let state = change_reader_state_for_repo(&args.repo)?;
            write(
                &args.format_args,
                stdout,
                &ReaderProfileDocumentV1::from(&state.capability),
            )
        }
        ChangeCommand::List(args) => {
            with_facade(&args.repo, &args.format_args, stdout, |facade, _| {
                Ok(serde_json::to_value(facade.list_document())?)
            })
        }
        ChangeCommand::Attention(args) => {
            with_facade(&args.repo, &args.format_args, stdout, |facade, _| {
                Ok(serde_json::to_value(facade.attention_document(false))?)
            })
        }
        ChangeCommand::Show(args) => {
            with_facade(&args.repo, &args.format_args, stdout, |facade, _| {
                Ok(serde_json::to_value(
                    facade.detail_document(&ChangeId::new(args.change))?,
                )?)
            })
        }
        ChangeCommand::Select(args) => run_select(args, stdout),
        ChangeCommand::Revision(args) => run_exact(args, stdout, true),
        ChangeCommand::Resource(args) => run_exact(args, stdout, false),
        ChangeCommand::Interdiff(args) => run_interdiff(args, stdout),
    }
}

fn with_facade(
    repo: &std::path::Path,
    format_args: &output::FormatArgs,
    stdout: &mut dyn Write,
    build: impl FnOnce(
        &ChangeDocumentFacadeV1,
        &ChangeReaderReadyV1,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let state = change_reader_state_for_repo(repo)?;
    if let Some(unavailable) = ChangeQueryUnavailableDocumentV1::for_inspection(&state.capability) {
        return write(format_args, stdout, &unavailable);
    }
    let ready = ready(&state)?;
    let facade =
        ChangeDocumentFacadeV1::new(ready.projection.clone(), ready.document_projection.clone())?;
    let document = build(&facade, ready)?;
    write(format_args, stdout, &document)
}

fn ready(state: &ChangeReaderStateV1) -> Result<&ChangeReaderReadyV1, Box<dyn std::error::Error>> {
    state
        .ready()
        .ok_or_else(|| "Change reader state has no complete semantic projection".into())
}

fn run_select(args: SelectArgs, stdout: &mut dyn Write) -> Result<(), Box<dyn std::error::Error>> {
    with_facade(&args.repo, &args.format_args, stdout, |_facade, ready| {
        let change_id = ChangeId::new(args.change);
        let change = ready
            .projection
            .changes
            .get(&change_id)
            .ok_or_else(|| format!("Change {} is unavailable", change_id.as_str()))?;
        let previous = args
            .cursor
            .as_deref()
            .map(ReviewCursorV1::decode_token)
            .transpose()?;
        if let Some(token) = args.cursor.as_deref() {
            validate_review_cursor_for_write(
                token,
                change,
                &ready.document_projection,
                &ReviewSourceBindingV1::Captured,
            )
            .map_err(|error| serde_json::to_string(&error).unwrap_or_else(|_| error.to_string()))?;
        }
        let revision_id = args
            .revision
            .map(RevisionId::new)
            .or_else(|| previous.map(|cursor| cursor.revision.revision_id));
        let selected = select_review_cursor(
            change,
            &ready.document_projection,
            revision_id.as_ref(),
            args.allow_historical,
            ReviewSourceBindingV1::Captured,
        )
        .map_err(|error| serde_json::to_string(&error).unwrap_or_else(|_| error.to_string()))?;
        Ok(serde_json::to_value(selected)?)
    })
}

fn run_exact(
    args: ExactReadArgs,
    stdout: &mut dyn Write,
    contextual: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    with_facade(&args.repo, &args.format_args, stdout, |facade, ready| {
        let change_id = ChangeId::new(args.change);
        let exact = exact_ref(
            ready,
            &change_id,
            &RevisionId::new(args.revision),
            &args.artifact_hash,
        )?;
        let exact_read = build_exact_read(&args.repo, ready, &exact, args.include_body)?;
        if contextual {
            Ok(serde_json::to_value(facade.contextual_revision_document(
                &change_id,
                &exact,
                exact_read.resource,
                exact_read.facts,
                exact_read.associations,
            )?)?)
        } else {
            Ok(serde_json::to_value(exact_read.resource)?)
        }
    })
}

fn run_interdiff(
    args: InterdiffArgs,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    with_facade(&args.repo, &args.format_args, stdout, |_facade, ready| {
        let change_id = ChangeId::new(args.change);
        Ok(serde_json::to_value(build_interdiff(
            ready,
            &change_id,
            &RevisionId::new(args.from),
            &args.from_artifact_hash,
            &RevisionId::new(args.to),
            &args.to_artifact_hash,
        )?)?)
    })
}

pub(crate) fn build_interdiff(
    ready: &ChangeReaderReadyV1,
    change_id: &ChangeId,
    from_revision_id: &RevisionId,
    from_artifact_hash: &str,
    to_revision_id: &RevisionId,
    to_artifact_hash: &str,
) -> Result<RevisionInterdiffDocumentV1, Box<dyn std::error::Error>> {
    let from = exact_ref(ready, change_id, from_revision_id, from_artifact_hash)?;
    let to = exact_ref(ready, change_id, to_revision_id, to_artifact_hash)?;
    Ok(RevisionInterdiffDocumentV1::new(
        RevisionInterdiffRefV1 {
            from,
            to,
            algorithm_version: "unavailable-v1".to_owned(),
            scope: Vec::new(),
        },
        RevisionInterdiffAvailabilityV1::Unavailable,
        None,
        vec!["revision_interdiff_not_available".to_owned()],
    )?)
}

pub(crate) fn exact_ref(
    ready: &ChangeReaderReadyV1,
    change_id: &ChangeId,
    revision_id: &RevisionId,
    artifact_hash: &str,
) -> Result<RevisionRefV1, Box<dyn std::error::Error>> {
    let change = ready
        .projection
        .changes
        .get(change_id)
        .ok_or_else(|| format!("Change {} is unavailable", change_id.as_str()))?;
    if !change.members.contains(revision_id) {
        return Err("exact Revision is not an active member of the Change".into());
    }
    let candidate = RevisionRefV1::new(revision_id.clone(), artifact_hash.to_owned())?;
    if !ready
        .document_projection
        .revision_refs
        .get(revision_id)
        .is_some_and(|references| references.contains(&candidate))
    {
        return Err("exact Revision/hash selector does not match authoritative state".into());
    }
    Ok(candidate)
}

pub(crate) struct ExactRead {
    pub(crate) resource: RevisionResourceDocumentV1,
    pub(crate) facts: Vec<FactPresentationV1>,
    pub(crate) associations: Vec<AssociationComparisonDocumentV1>,
}

pub(crate) fn build_exact_read(
    repo: &std::path::Path,
    ready: &ChangeReaderReadyV1,
    exact: &RevisionRefV1,
    include_body: bool,
) -> Result<ExactRead, Box<dyn std::error::Error>> {
    let result = show_revision_for_change_reader_ready(
        RevisionShowOptions::new(repo)
            .with_revision_id(exact.revision_id.clone())
            .with_exact(true)
            .with_include_body(include_body)
            .with_read_for_display(true),
        ready,
    )?;
    if result.revision.object_artifact_content_hash != exact.object_artifact_content_hash {
        return Err("exact Revision projection returned a different artifact hash".into());
    }
    let facts = fact_presentations(&result, exact);
    let associations = association_documents(&result, exact)?;
    let resource_ref = RevisionResourceRefV1 {
        revision: exact.clone(),
        object_id: result.revision.object_id.clone(),
    };
    let projection = RevisionResourceProjectionV1 {
        track_id: result.filters.track_id.clone(),
        include_body,
    };
    let memberships = ready
        .document_projection
        .membership_claims
        .iter()
        .filter(|claim| claim.active && claim.revision_id == exact.revision_id)
        .cloned()
        .collect();
    let state = result.snapshot_content_state;
    let unavailable = unavailable_content_availability(&result.diagnostics);
    let exact_document = revision_show_document_v3(result, exact.clone(), memberships)?;
    let resource = match state {
        SnapshotContentState::Present => RevisionResourceDocumentV1::available(
            resource_ref,
            projection,
            &exact.object_artifact_content_hash,
            serde_json::to_value(exact_document)?,
        )?,
        SnapshotContentState::SuppressedPresent | SnapshotContentState::PhysicallyRemoved => {
            RevisionResourceDocumentV1::unavailable(
                resource_ref,
                projection,
                ContentAvailabilityV1::Removed,
            )?
        }
        SnapshotContentState::Unavailable => {
            RevisionResourceDocumentV1::unavailable(resource_ref, projection, unavailable)?
        }
    };
    Ok(ExactRead {
        resource,
        facts,
        associations,
    })
}

fn unavailable_content_availability(
    diagnostics: &[pointbreak::session::ProjectionDiagnostic],
) -> ContentAvailabilityV1 {
    if diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "snapshot_content_unavailable"
            && diagnostic.message.to_ascii_lowercase().contains("mismatch")
    }) {
        ContentAvailabilityV1::Mismatch
    } else {
        ContentAvailabilityV1::Missing
    }
}

fn fact_presentations(
    result: &pointbreak::session::RevisionShowResult,
    exact: &RevisionRefV1,
) -> Vec<FactPresentationV1> {
    let mut facts = Vec::new();
    for view in &result.observations {
        facts.push(fact(
            view.id.as_str(),
            "observation",
            exact,
            &view.writer.actor_id,
            Some(view.track_id.clone()),
            if view.status == ObservationStatus::Active {
                FactFamilyStateV1::Current
            } else {
                FactFamilyStateV1::Stale
            },
        ));
    }
    for view in &result.input_requests {
        facts.push(fact(
            view.id.as_str(),
            "input_request",
            exact,
            &view.writer.actor_id,
            Some(view.track_id.clone()),
            FactFamilyStateV1::Current,
        ));
    }
    for view in &result.assessments {
        facts.push(fact(
            view.id.as_str(),
            "assessment",
            exact,
            &view.writer.actor_id,
            Some(view.track_id.clone()),
            if view.status == AssessmentRecordStatus::Current {
                FactFamilyStateV1::Current
            } else {
                FactFamilyStateV1::Stale
            },
        ));
    }
    for view in &result.validation_checks {
        facts.push(fact(
            view.id.as_str(),
            "validation",
            exact,
            &view.writer.actor_id,
            Some(view.track_id.clone()),
            if view.superseded_by_revisions.is_empty() {
                FactFamilyStateV1::Current
            } else {
                FactFamilyStateV1::Stale
            },
        ));
    }
    facts.sort_by(|left, right| left.fact_id.cmp(&right.fact_id));
    facts
}

fn fact(
    fact_id: &str,
    family: &str,
    exact: &RevisionRefV1,
    actor_id: &pointbreak::model::ActorId,
    track_id: Option<pointbreak::model::TrackId>,
    family_state: FactFamilyStateV1,
) -> FactPresentationV1 {
    FactPresentationV1 {
        fact_id: fact_id.to_owned(),
        family: family.to_owned(),
        origin_revision: exact.clone(),
        context_change_id: None,
        presented_in_revision: None,
        port_relation: None,
        actor_id: actor_id.clone(),
        track_id,
        family_state,
        revision_currency: pointbreak::documents::ChangeRevisionCurrencyV1::Current,
        availability: ContentAvailabilityV1::Available,
    }
}

fn association_documents(
    result: &pointbreak::session::RevisionShowResult,
    exact: &RevisionRefV1,
) -> Result<Vec<AssociationComparisonDocumentV1>, Box<dyn std::error::Error>> {
    result
        .commit_range
        .current_commits
        .iter()
        .filter_map(|association| {
            association
                .commit_association_id
                .clone()
                .map(|association_id| {
                    AssociationComparisonDocumentV1::new(
                        AssociationComparisonRefV1 {
                            revision: exact.clone(),
                            association_id,
                            commit_oid: association.commit_oid.clone(),
                            comparison_base: "captured_revision".to_owned(),
                            view_kind: "landing".to_owned(),
                            proof_ref: None,
                        },
                        AssociationComparisonStateV1::Unknown,
                        AssociationProofAvailabilityV1::NotRequested,
                        vec!["comparison_proof_not_requested".to_owned()],
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn write<T: serde::Serialize>(
    format_args: &output::FormatArgs,
    stdout: &mut dyn Write,
    document: &T,
) -> Result<(), Box<dyn std::error::Error>> {
    let format = output::resolve_format(format_args.explicit(), output::OutputFormat::Json)?;
    output::write_document(stdout, format, document, || {
        serde_json::to_string_pretty(document).unwrap_or_else(|_| "unavailable".to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_content_preserves_mismatch_as_a_distinct_typed_state() {
        let mismatch = pointbreak::session::ProjectionDiagnostic {
            code: "snapshot_content_unavailable".to_owned(),
            message: "snapshot content is unavailable: object artifact content hash mismatch"
                .to_owned(),
        };
        assert_eq!(
            unavailable_content_availability(&[mismatch]),
            ContentAvailabilityV1::Mismatch
        );
        assert_eq!(
            unavailable_content_availability(&[]),
            ContentAvailabilityV1::Missing
        );
    }
}
