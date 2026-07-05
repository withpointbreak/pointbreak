use std::io::Write;

use clap::{Args, Subcommand};

mod list;
mod show;

#[derive(Debug, Args)]
pub(super) struct RevisionArgs {
    #[command(subcommand)]
    command: RevisionCommand,
}

#[derive(Debug, Subcommand)]
enum RevisionCommand {
    List(list::RevisionListArgs),
    Show(show::ShowArgs),
}

pub(super) fn run(
    args: RevisionArgs,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    match args.command {
        RevisionCommand::List(args) => list::run(args, stdout),
        RevisionCommand::Show(args) => show::run(args, stdout),
    }
}
