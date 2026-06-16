use std::io::Write;

use clap::{Args, Subcommand};

mod enroll;
mod init;
mod list;
mod show;

use enroll::EnrollArgs;
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
    /// Stage an allow-list entry binding a local key's did:key to an actor.
    /// Possession-style: this stages the working-tree `.shore/allowed-signers.json`
    /// edit only; commit it to authorize the binding.
    Enroll(EnrollArgs),
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
        KeysCommand::Enroll(args) => {
            tracing::debug!(command = "keys.enroll", "command_start");
            enroll::run(args, stdout)
        }
    }
}
