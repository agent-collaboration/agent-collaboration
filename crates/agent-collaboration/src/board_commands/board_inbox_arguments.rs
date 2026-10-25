//! Inbox argument shapes mirror independent project write composition.
use super::board_arguments::{CommonArguments, PageArguments};
use clap::{Args, ValueEnum};
#[derive(Args)]
pub(super) struct InboxFetchArguments {
    /// Select top-level messages; independently watched threads are always included.
    #[arg(long, value_enum)]
    pub scope: InboxScopeKind,
    #[arg(long)]
    pub workspace_id: Option<String>,
    #[arg(long)]
    pub board_id: Option<String>,
    #[arg(long)]
    pub project_id: Option<String>,
    #[arg(long, value_enum, default_value = "unread")]
    pub read_mode: InboxModeKind,
    /// Typed reader Identity JSON. Fetching never acknowledges activity.
    #[arg(long)]
    pub reader: String,
    /// Per-project initialization entry JSON with its own RequestId; repeat for aggregate scope.
    #[arg(long = "initialization")]
    pub initializations: Vec<String>,
    #[command(flatten)]
    pub page: PageArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(super) struct InboxAcknowledgeArguments {
    /// AcknowledgementEntry JSON, including requestId, scope and FeedCursor; repeat for a batch.
    #[arg(long = "entry", required = true)]
    pub entries: Vec<String>,
    /// Shared declared Identity JSON.
    #[arg(long)]
    pub actor: String,
    #[arg(long)]
    pub acting_for: Option<String>,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(super) struct InboxWorkspacesArguments {
    /// Typed reader Identity JSON.
    #[arg(long)]
    pub reader: String,
    #[arg(long)]
    pub unread_only: bool,
    #[command(flatten)]
    pub page: PageArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Clone, Copy, ValueEnum)]
pub(super) enum InboxScopeKind {
    Workspace,
    Board,
    Project,
}
#[derive(Clone, Copy, ValueEnum)]
pub(super) enum InboxModeKind {
    Unread,
    Latest,
}
