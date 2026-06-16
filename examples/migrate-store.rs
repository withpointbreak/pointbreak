//! One-off migration driver: relocate a legacy flat `.shore/` store to
//! `.shore/data/` and upgrade event writer fields in place (writer.tool ->
//! writer.producer, drop writer.role). Owner-run; NOT part of the `shore`
//! binary — the migration is a one-time, owner-only operation, so it ships as a
//! tested library function plus this thin driver rather than a permanent CLI
//! subcommand.
use std::process::ExitCode;

use shoreline::session::{MigrateStoreOptions, migrate_store};

fn main() -> ExitCode {
    let repo = std::env::args().nth(1).unwrap_or_else(|| ".".to_owned());
    match migrate_store(MigrateStoreOptions::new(&repo)) {
        Ok(result) => {
            println!(
                "migrated {repo}: relocated={} events_rewritten={} events_unchanged={}",
                result.relocated, result.events_rewritten, result.events_unchanged,
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("migrate-store failed for {repo}: {error}");
            ExitCode::FAILURE
        }
    }
}
