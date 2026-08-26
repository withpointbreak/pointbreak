use std::io::Write;

use clap::{Args, Subcommand};
use pointbreak::session::PublicReadCommandContextV1;

mod list;
mod show;

#[derive(Debug, Args)]
pub(super) struct RevisionArgs {
    #[command(subcommand)]
    command: RevisionCommand,
}

impl RevisionArgs {
    pub(super) fn qualified_invocation_read_v1(
        &self,
    ) -> Option<super::QualifiedInvocationReadV1<'_>> {
        match &self.command {
            RevisionCommand::Show(args) => args.qualified_invocation_read_v1(),
            RevisionCommand::List(_) => None,
        }
    }
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

pub(super) fn run_with_public_read_context(
    args: RevisionArgs,
    context: PublicReadCommandContextV1,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    match args.command {
        RevisionCommand::Show(args) => show::run_with_public_read_context(args, context, stdout),
        RevisionCommand::List(_) => {
            Err("the public read context admits only the qualified revision show shape".into())
        }
    }
}
