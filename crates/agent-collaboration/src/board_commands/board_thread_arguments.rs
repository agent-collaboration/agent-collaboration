//! Arguments specific to thread creation, subjects, participation and lifecycle.
use super::super::board_subscription_arguments::{SubscriptionModeKind, WhenIdleKind};
use super::{CommonArguments, MutationIdentityArguments, PageArguments, RequestMetadataArguments};
use clap::{Args, ValueEnum};
use std::path::PathBuf;

#[derive(Args)]
pub(in super::super) struct ThreadShowArguments {
    /// Include the first line of a redacted work-trail subject.
    #[arg(long)]
    pub include_redacted: bool,
    #[arg(long)]
    pub thread_id: String,
    /// Optional typed reader Identity JSON; when present, includes watch status.
    #[arg(long)]
    pub reader: Option<String>,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(in super::super) struct ThreadMutationArguments {
    #[arg(long)]
    pub thread_id: String,
    #[command(flatten)]
    pub identity: MutationIdentityArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(in super::super) struct ThreadResolveArguments {
    #[command(flatten)]
    pub mutation: ThreadMutationArguments,
    /// Required nonempty explanation for resolving the thread.
    #[arg(long)]
    pub reason: String,
}

#[derive(Args)]
pub(in super::super) struct ThreadRefileArguments {
    #[command(flatten)]
    pub mutation: ThreadMutationArguments,
    /// project:<line>, workstream:<id>, task:<id>, row:<id>, or adr:<id>.
    #[arg(long)]
    pub about: String,
}

#[derive(Args)]
pub(in super::super) struct ThreadWatchArguments {
    #[arg(long, conflicts_with = "project_id")]
    pub thread_id: Option<String>,
    #[arg(long, conflicts_with = "thread_id")]
    pub project_id: Option<String>,
    #[command(flatten)]
    pub identity: MutationIdentityArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(in super::super) struct ThreadCreateArguments {
    /// Explicit notification target JSON, repeated for each identity or role.
    #[arg(long = "addressee")]
    pub addressees: Vec<String>,
    /// project:<line>, workstream:<id>, task:<id>, row:<id>, or adr:<id>.
    #[arg(long)]
    pub about: String,
    #[command(flatten)]
    pub request: RequestMetadataArguments,
    /// Caller-chosen UUIDv7; generated and reported when omitted.
    #[arg(long)]
    pub post_id: Option<String>,
    #[arg(long)]
    pub project_id: String,
    /// Typed actor Identity JSON, or self for the current Codex or Claude Code session.
    #[arg(long)]
    pub actor: String,
    #[arg(long)]
    pub acting_for: Option<String>,
    /// Participant Role. Required when the actor is a session.
    #[arg(long, value_enum)]
    pub role: Option<ParticipantRoleKind>,
    /// Create a Watch for this actor.
    #[arg(long, conflicts_with = "no_watch")]
    pub watch: bool,
    /// Do not create a Watch for this actor.
    #[arg(long, conflicts_with = "watch")]
    pub no_watch: bool,
    /// Read root text from this file: 1 to 65536 UTF-8 bytes.
    #[arg(long, value_name = "PATH")]
    pub text_file: PathBuf,
    #[arg(long)]
    pub reference_post: Vec<String>,
    #[arg(long)]
    pub reference_thread: Vec<String>,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(in super::super) struct ThreadJoinArguments {
    #[command(flatten)]
    pub request: RequestMetadataArguments,
    #[arg(long)]
    pub thread_id: String,
    /// Typed actor Identity JSON, or self for the current Codex or Claude Code session.
    #[arg(long)]
    pub actor: String,
    #[arg(long, value_enum)]
    pub role: ParticipantRoleKind,
    /// Watch the Thread after joining; this is the default.
    #[arg(long, conflicts_with = "no_watch")]
    pub watch: bool,
    /// Join without watching the Thread.
    #[arg(long, conflicts_with = "watch")]
    pub no_watch: bool,
    /// Current Orchestrator identity to replace. Only valid with --role orchestrator.
    #[arg(long)]
    pub replace: Option<String>,
    #[arg(long)]
    pub note: Option<String>,
    /// Delivery mode for the subscription created or renewed by joining.
    #[arg(long, value_enum)]
    pub mode: Option<SubscriptionModeKind>,
    /// Behaviour when the target session is idle.
    #[arg(long, value_enum)]
    pub when_idle: Option<WhenIdleKind>,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Clone, Copy, ValueEnum)]
pub(in super::super) enum ParticipantRoleKind {
    Orchestrator,
    Implementer,
    Advisor,
    Reviewer,
    Participant,
}

#[derive(Args)]
pub(in super::super) struct ThreadLeaveArguments {
    #[command(flatten)]
    pub request: RequestMetadataArguments,
    #[arg(long)]
    pub thread_id: String,
    /// Typed actor Identity JSON, or self for the current Codex or Claude Code session.
    #[arg(long)]
    pub actor: String,
    #[arg(long)]
    pub to: Option<String>,
    #[arg(long, conflicts_with = "to")]
    pub resolve: Option<String>,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(in super::super) struct ThreadParticipantListArguments {
    #[arg(long)]
    pub thread_id: String,
    #[command(flatten)]
    pub page: PageArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(in super::super) struct ThreadListArguments {
    #[arg(long)]
    pub workspace_id: Option<String>,
    /// Explicit repository path; lists matching threads across workspaces.
    #[arg(long)]
    pub repository_path: Option<PathBuf>,
    /// Optional typed reader Identity JSON or `self`; adds watch status.
    #[arg(long)]
    pub reader: Option<String>,
    #[arg(long)]
    pub watched_only: bool,
    #[command(flatten)]
    pub page: PageArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(in super::super) struct ThreadWaitArguments {
    /// Independent renewal entry JSON; repeated entries retain their request keys.
    #[arg(long = "renewal")]
    pub renewals: Vec<String>,
    #[arg(long)]
    pub acting_for: Option<String>,
    /// Filter to one or more Thread subscription roots.
    #[arg(long, conflicts_with = "project_id")]
    pub thread_id: Vec<String>,
    /// Filter to one Project subscription.
    #[arg(long, conflicts_with = "thread_id")]
    pub project_id: Option<String>,
    /// Maximum wait: integer followed by s, m, h, or d, at most 25 minutes.
    #[arg(long)]
    pub max_wait: String,
    /// Typed Reader Identity JSON, or self for the current session.
    #[arg(long)]
    pub actor: String,
    #[command(flatten)]
    pub common: CommonArguments,
}
