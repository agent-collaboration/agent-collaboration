use super::board_arguments::CommonArguments;
use clap::Args;

#[derive(Args)]
#[command(group(clap::ArgGroup::new("receipt_scope").required(true).multiple(false).args(["project_id", "workspace_id"])))]
pub(super) struct RequestEffectsArguments {
    /// Request identity returned by the write.
    pub request: String,
    /// Project scope holding the receipt.
    #[arg(long)]
    pub project_id: Option<String>,
    /// Workspace scope for a Fleet receipt.
    #[arg(long)]
    pub workspace_id: Option<String>,
    /// Opaque continuation returned in the preceding page's next field.
    #[arg(long)]
    pub after: Option<String>,
    #[command(flatten)]
    pub common: CommonArguments,
}
