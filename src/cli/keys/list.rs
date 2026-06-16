use std::io::Write;

use clap::Args;

use crate::cli::json;

#[derive(Debug, Args)]
pub(super) struct ListArgs {}

#[derive(serde::Serialize)]
struct ListBody {
    keys: Vec<serde_json::Value>,
}

pub(super) fn run(
    _args: ListArgs,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let document =
        json::DiagnosticDocument::new("shore.keys-list", ListBody { keys: Vec::new() }, vec![]);
    json::write_json(stdout, &document, false)
}
