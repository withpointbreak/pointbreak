use std::path::PathBuf;

use clap::Args;

#[derive(Clone, Debug, Args)]
pub(super) struct ReviewInputArgs {
    #[arg(long, default_value = ".")]
    pub(super) repo: PathBuf,

    #[arg(long)]
    pub(super) review_notes: Option<PathBuf>,
}
