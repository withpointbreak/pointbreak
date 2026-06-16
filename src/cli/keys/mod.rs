use std::io::Write;

use clap::{Args, Subcommand};

mod init;
mod list;
mod show;

use init::InitArgs;
use list::ListArgs;
use show::ShowArgs;

#[derive(Debug, Args)]
pub(super) struct KeysArgs {
    #[command(subcommand)]
    command: KeysCommand,
}

#[derive(Debug, Subcommand)]
enum KeysCommand {
    /// Generate a new signing key in the user-level keystore.
    Init(InitArgs),
    /// List local signing keys and their enrollment status.
    List(ListArgs),
    /// Print a key's did:key and/or raw public key.
    Show(ShowArgs),
    // A later subcommand for enrollment slots in here without reshaping the
    // enum; the dispatcher's match gains one arm.
}

pub(super) fn run(
    args: KeysArgs,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    match args.command {
        KeysCommand::Init(args) => {
            tracing::debug!(command = "keys.init", "command_start");
            init::run(args, stdout)
        }
        KeysCommand::List(args) => {
            tracing::debug!(command = "keys.list", "command_start");
            list::run(args, stdout)
        }
        KeysCommand::Show(args) => {
            tracing::debug!(command = "keys.show", "command_start");
            show::run(args, stdout)
        }
    }
}
