use std::io::Write;

use clap::{Args, Subcommand};

use crate::cli_tracing::TracingArgs;

pub(super) mod assessment;
pub(super) mod association;
pub(super) mod capture;
pub(super) mod common;
pub(super) mod endorse;
pub(super) mod history;
pub(super) mod input_request;
pub(super) mod observation;
pub(super) mod revisions;
pub(super) mod show;
pub(super) mod validation;

#[derive(Debug, Args)]
pub(super) struct ReviewArgs {
    #[command(subcommand)]
    command: ReviewCommand,
}

#[derive(Debug, Subcommand)]
enum ReviewCommand {
    Assessment(assessment::AssessmentArgs),
    Association(association::AssociationArgs),
    Capture(capture::CaptureArgs),
    Endorse(endorse::EndorseArgs),
    History(history::HistoryArgs),
    InputRequest(input_request::InputRequestArgs),
    Observation(observation::ObservationArgs),
    Revisions(revisions::RevisionsArgs),
    Show(show::ShowArgs),
    Validation(validation::ValidationArgs),
}

pub(super) fn run(
    args: ReviewArgs,
    tracing: &TracingArgs,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    match args.command {
        ReviewCommand::Assessment(args) => assessment::run(args, stdout, stderr),
        ReviewCommand::Association(args) => association::run(args, stdout, stderr),
        ReviewCommand::Capture(args) => capture::run(args, tracing, stdout, stderr),
        ReviewCommand::Endorse(args) => endorse::run(args, stdout, stderr),
        ReviewCommand::History(args) => history::run(args, stdout),
        ReviewCommand::InputRequest(args) => input_request::run(args, stdout, stderr),
        ReviewCommand::Observation(args) => observation::run(args, stdout, stderr),
        ReviewCommand::Revisions(args) => revisions::run(args, stdout),
        ReviewCommand::Show(args) => show::run(args, stdout),
        ReviewCommand::Validation(args) => validation::run(args, stdout, stderr),
    }
}
