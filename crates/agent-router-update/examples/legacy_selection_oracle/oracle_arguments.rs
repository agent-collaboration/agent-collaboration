use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "legacy_selection_oracle",
    about = "Independently select inbox posts from a sealed legacy SQLite copy"
)]
pub(crate) struct OracleArguments {
    #[arg(long, required = true)]
    pub(crate) legacy_copy: PathBuf,
    #[arg(long, required = true)]
    pub(crate) copy_root: PathBuf,
    #[arg(long, required = true)]
    pub(crate) output: PathBuf,
}
