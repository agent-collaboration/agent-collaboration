//! Clap arguments for subscription scope and policy commands.
use super::board_arguments::CommonArguments;
use super::board_request_arguments::RequestMetadataArguments;
use clap::{Args, ValueEnum};

#[derive(Args)]
pub(super) struct ThreadSubscriptionScopeArguments {
    /// Root Thread UUIDv7. Mutually exclusive with --project-id.
    #[arg(long, conflicts_with = "project_id")]
    pub thread_id: Option<String>,
    /// Project UUIDv7. Mutually exclusive with --thread-id.
    #[arg(long, conflicts_with = "thread_id")]
    pub project_id: Option<String>,
}

#[derive(Args)]
pub(super) struct ThreadSubscribeArguments {
    #[command(flatten)]
    pub request: RequestMetadataArguments,
    #[command(flatten)]
    pub scope: ThreadSubscriptionScopeArguments,
    /// Typed Reader Identity JSON, or self for the current session.
    #[arg(long)]
    pub actor: String,
    #[arg(long, value_enum)]
    pub mode: Option<SubscriptionModeKind>,
    #[arg(long, value_enum)]
    pub when_idle: Option<WhenIdleKind>,
    /// Quiet period: 0 seconds through 30 minutes.
    #[arg(long)]
    pub quiet: Option<String>,
    /// Maximum batch delay: at least the quiet period, up to 60 minutes.
    #[arg(long)]
    pub cap: Option<String>,
    /// Subscription lifetime: 10 minutes through 7 days.
    #[arg(long = "for", value_name = "DURATION")]
    pub for_duration: Option<String>,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(super) struct ThreadUnsubscribeArguments {
    #[command(flatten)]
    pub request: RequestMetadataArguments,
    #[command(flatten)]
    pub scope: ThreadSubscriptionScopeArguments,
    /// Typed Reader Identity JSON, or self for the current session.
    #[arg(long)]
    pub actor: String,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(super) struct ThreadSubscriptionsArguments {
    /// Typed Reader Identity JSON, or self for the current session.
    #[arg(long)]
    pub actor: String,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Clone, Copy, ValueEnum)]
pub(super) enum SubscriptionModeKind {
    Deliver,
    Poll,
    Off,
}

#[derive(Clone, Copy, ValueEnum)]
pub(super) enum WhenIdleKind {
    Hold,
    Wake,
    Drop,
}
