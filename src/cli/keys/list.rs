use std::io::Write;
use std::path::PathBuf;

use clap::Args;
use shoreline::keys::list_keys;

use crate::cli::json;
use crate::cli::review::common::discover_trust_set;

#[derive(Debug, Args)]
pub(super) struct ListArgs {
    /// Repository whose committed allowed-signers file determines enrollment.
    #[arg(long, default_value = ".")]
    repo: PathBuf,

    #[arg(long)]
    pretty: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct KeyEntry {
    name: String,
    did_key: String,
    default: bool,
    enrolled: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ListBody {
    keys: Vec<KeyEntry>,
}

pub(super) fn run(
    args: ListArgs,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    // Single trust-set discovery path, shared with the verifying read commands.
    let trust_set = discover_trust_set(&args.repo);
    let keys = list_keys()?
        .into_iter()
        .map(|info| {
            let did_key = info.signer_id().clone();
            KeyEntry {
                default: info.name() == "default",
                enrolled: trust_set.contains_signer(&did_key),
                did_key: did_key.as_str().to_owned(),
                name: info.name().to_owned(),
            }
        })
        .collect();
    let document = json::DiagnosticDocument::new("shore.keys-list", ListBody { keys }, vec![]);
    json::write_json(stdout, &document, args.pretty)
}
