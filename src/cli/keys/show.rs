use std::io::Write;

use clap::Args;

use crate::cli::json;

#[derive(Debug, Args)]
pub(super) struct ShowArgs {}

#[derive(serde::Serialize)]
struct ShowBody {}

pub(super) fn run(
    _args: ShowArgs,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let document = json::DiagnosticDocument::new("shore.keys-show", ShowBody {}, vec![]);
    json::write_json(stdout, &document, false)
}
