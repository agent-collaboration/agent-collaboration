use super::board_entity_arguments::{
    AdrCommand, FactCommand, FeedCommand, OperationFileArguments, RoleCommand, SubtaskCommand,
    TaskCommand, WorkTrailCommand, WorkstreamCommand,
};
use super::board_inbox_arguments::{
    InboxAcknowledgeArguments, InboxFetchArguments, InboxWorkspacesArguments,
};
use super::board_receipt_arguments::RequestEffectsArguments;
use super::board_request_arguments::RequestMetadataArguments;
use super::board_search_commands::{DiscoverySearchArguments, PostSearchArguments};
use super::board_subscription_arguments::{
    ThreadSubscribeArguments, ThreadSubscriptionsArguments, ThreadUnsubscribeArguments,
};
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "agent-collaboration",
    bin_name = "agent-collaboration",
    about = "Coordinate durable workspace message boards through the selected Router service"
)]
pub(super) struct BoardArguments {
    #[command(subcommand)]
    pub command: BoardCommand,
}

#[derive(Subcommand)]
pub(super) enum BoardCommand {
    /// Read a request receipt without replaying the write.
    RequestEffects(RequestEffectsArguments),
    /// Inspect recorded work, pending duties and session context.
    View {
        #[command(subcommand)]
        command: super::board_view_commands::ViewCommand,
    },
    /// Search workspace, board, and project names/descriptions with strict location filters.
    Search(DiscoverySearchArguments),
    /// Create, inspect, update, or list workspaces.
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommand,
    },
    /// Attach, detach, or list workspace repository associations.
    Repository {
        #[command(subcommand)]
        command: RepositoryCommand,
    },
    /// Create, inspect or archive boards.
    Board {
        #[command(subcommand)]
        command: BoardMetadataCommand,
    },
    /// Create, update, or list projects.
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    /// Post, show, or list immutable messages.
    Post {
        #[command(subcommand)]
        command: PostCommand,
    },
    /// Inspect, change, watch, or list root-message threads.
    Thread {
        #[command(subcommand)]
        command: ThreadCommand,
    },
    /// Fetch or acknowledge personal unread activity.
    Inbox {
        #[command(subcommand)]
        command: InboxCommand,
    },
    Workstream {
        #[command(subcommand)]
        command: WorkstreamCommand,
    },
    Task {
        #[command(subcommand)]
        command: TaskCommand,
    },
    Subtask {
        #[command(subcommand)]
        command: SubtaskCommand,
    },
    WorkTrail {
        #[command(subcommand)]
        command: WorkTrailCommand,
    },
    Role {
        #[command(subcommand)]
        command: RoleCommand,
    },
    Fact {
        #[command(subcommand)]
        command: FactCommand,
    },
    Feed {
        #[command(subcommand)]
        command: FeedCommand,
    },
    Adr {
        #[command(subcommand)]
        command: AdrCommand,
    },
}

#[derive(Subcommand)]
pub(super) enum BoardMetadataCommand {
    /// Create a board in a workspace. Obtain the workspace owner's permission before running this command.
    Create(BoardCreateArguments),
    /// Replace a board's name and description.
    Update(BoardUpdateArguments),
    /// Show one board.
    Show(BoardShowArguments),
    /// List boards, excluding archived boards unless requested.
    List(BoardListArguments),
    /// Archive a board and make its content read-only.
    Archive(BoardLifecycleArguments),
    /// Close an initiative board. Area boards stay open.
    Close(BoardLifecycleArguments),
}

#[derive(Subcommand)]
pub(super) enum WorkspaceCommand {
    /// Create a workspace. Obtain the workspace owner's permission before running this command.
    Create(WorkspaceCreateArguments),
    /// Replace a workspace's name and description.
    Update(WorkspaceUpdateArguments),
    /// Archive the workspace and all of its boards.
    Archive(WorkspaceArchiveArguments),
    /// Show one workspace.
    Show(WorkspaceShowArguments),
    /// List workspaces, optionally associated with a repository.
    List(WorkspaceListArguments),
}

#[derive(Subcommand)]
pub(super) enum RepositoryCommand {
    /// Attach the repository discovered from a path or explicit origin.
    Attach(RepositoryMutationArguments),
    /// Detach the repository discovered from a path or explicit origin.
    Detach(RepositoryMutationArguments),
    /// List repositories associated with a workspace.
    List(RepositoryListArguments),
}

#[derive(Subcommand)]
pub(super) enum ProjectCommand {
    /// Claim an unheld project and verify its seeded native store before writing.
    Claim(ProjectClaimArguments),
    /// Request cancellation of one notification without writing a project record.
    NotificationCancel(OperationFileArguments),
    /// Create a project on an active board.
    Create(ProjectCreateArguments),
    /// Replace a project's title.
    Update(ProjectUpdateArguments),
    /// List projects on a board.
    List(ProjectListArguments),
    /// Read the project directory entry and locally available project state.
    Show(ProjectShowArguments),
    /// Replace the project description under its expected revision.
    Describe(OperationFileArguments),
    PublishPlan(OperationFileArguments),
    Close(OperationFileArguments),
}

#[derive(Subcommand)]
pub(super) enum PostCommand {
    /// Search message text, including messages in unwatched threads.
    Search(PostSearchArguments),
    /// Post an immutable top-level or thread message.
    Post(ThreadPostArguments),
    /// Show one message.
    Show(PostShowArguments),
    /// List message history with an explicit scope and selection mode.
    List(PostListArguments),
}

#[derive(Subcommand)]
pub(super) enum ThreadCommand {
    /// Create a root Thread and optionally join it with an explicit Role.
    Create(ThreadCreateArguments),
    /// Join a Thread with an explicit Role; watching is the default.
    Join(ThreadJoinArguments),
    /// Subscribe to activity in one Thread or Project with an optional delivery policy.
    Subscribe(ThreadSubscribeArguments),
    /// Cancel the subscription for one Thread or Project.
    Unsubscribe(ThreadUnsubscribeArguments),
    /// List active and draining subscriptions for one Reader.
    Subscriptions(ThreadSubscriptionsArguments),
    /// Leave a Thread, handing over or resolving when required by the current Role.
    Leave(ThreadLeaveArguments),
    /// List open and closed Participants for one Thread.
    Participant {
        #[command(subcommand)]
        command: ThreadParticipantCommand,
    },
    /// Show a root-message thread and optional reader watch status.
    Show(ThreadShowArguments),
    /// Mark a thread resolved.
    Resolve(ThreadResolveArguments),
    /// Re-file the thread under an explicit subject as a project role holder.
    Refile(ThreadRefileArguments),
    /// Mark a thread unresolved so thread messages can be posted again.
    Unresolve(ThreadMutationArguments),
    /// Watch future activity in a thread. Earlier history remains an explicit range.
    Watch(ThreadWatchArguments),
    /// Stop watching a thread. Existing message history remains readable.
    Unwatch(ThreadWatchArguments),
    /// List workspace threads and reader watch status.
    #[command(
        long_about = "List workspace threads and reader watch status. With neither --workspace-id nor --repository-path, use the current Git repository when available. Example outside a repository: agent-collaboration thread list --repository-path '<path-in-repository>' (or --workspace-id <workspace-id>). --watched-only requires --workspace-id."
    )]
    List(ThreadListArguments),
    /// Wait for due poll-mode subscription activity; no scope flag selects all subscriptions.
    Wait(ThreadWaitArguments),
}

#[derive(Subcommand)]
pub(super) enum ThreadParticipantCommand {
    /// List open and closed Participants for one Thread.
    List(ThreadParticipantListArguments),
}

#[derive(Subcommand)]
pub(super) enum InboxCommand {
    /// Fetch unread activity without acknowledging it.
    Fetch(InboxFetchArguments),
    /// Acknowledge one or more scopes with independent request keys and feed cursors.
    Acknowledge(InboxAcknowledgeArguments),
    /// List workspaces with personal inbox tracking or watches.
    Workspaces(InboxWorkspacesArguments),
}

#[derive(Args)]
pub(super) struct CommonArguments {
    /// Router communication service directory. Must be absolute.
    #[arg(long)]
    pub service_directory: Option<PathBuf>,
    /// Emit one compact JSON result or error object.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub(super) struct MutationIdentityArguments {
    #[command(flatten)]
    pub request: RequestMetadataArguments,
    /// Typed actor Identity JSON, for example {"kind":"human","humanId":"alice"}.
    #[arg(long)]
    pub actor: String,
    /// Optional human attribution JSON, for example {"kind":"human","humanId":"owner"}.
    #[arg(long)]
    pub acting_for: Option<String>,
}

#[derive(Args)]
pub(super) struct PageArguments {
    /// Maximum records, from 1 through 100.
    #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u32).range(1..=100))]
    pub limit: u32,
    /// Opaque cursor returned by the preceding page of the same query.
    #[arg(long)]
    pub cursor: Option<String>,
}

#[derive(Args)]
pub(super) struct RepositorySelectorArguments {
    /// Git remote origin to normalize into the repository identity.
    #[arg(long, conflicts_with = "repository_path")]
    pub repository_origin: Option<String>,
    /// Path inside a local Git repository; origin is preferred, otherwise its common directory is used.
    #[arg(long, value_name = "PATH", conflicts_with = "repository_origin")]
    pub repository_path: Option<PathBuf>,
}

#[derive(Args)]
pub(super) struct WorkspaceCreateArguments {
    /// Caller-chosen UUIDv7; generated and reported when omitted.
    #[arg(long)]
    pub workspace_id: Option<String>,
    /// Workspace name: 1 to 256 UTF-8 bytes after trimming.
    #[arg(long)]
    pub name: String,
    /// Workspace description: 0 to 16384 UTF-8 bytes.
    #[arg(long, default_value = "")]
    pub description: String,
    #[command(flatten)]
    pub identity: MutationIdentityArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(super) struct WorkspaceUpdateArguments {
    #[arg(long)]
    pub workspace_id: String,
    /// Workspace name: 1 to 256 UTF-8 bytes after trimming.
    #[arg(long)]
    pub name: String,
    /// Workspace description: 0 to 16384 UTF-8 bytes.
    #[arg(long)]
    pub description: String,
    #[command(flatten)]
    pub identity: MutationIdentityArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(super) struct WorkspaceArchiveArguments {
    #[arg(long)]
    pub workspace_id: String,
    #[command(flatten)]
    pub identity: MutationIdentityArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(super) struct WorkspaceShowArguments {
    #[arg(long)]
    pub workspace_id: String,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(super) struct WorkspaceListArguments {
    #[command(flatten)]
    pub repository: RepositorySelectorArguments,
    #[command(flatten)]
    pub page: PageArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(super) struct RepositoryMutationArguments {
    #[arg(long)]
    pub workspace_id: String,
    #[command(flatten)]
    pub repository: RepositorySelectorArguments,
    #[command(flatten)]
    pub identity: MutationIdentityArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(super) struct RepositoryListArguments {
    #[arg(long)]
    pub workspace_id: String,
    #[command(flatten)]
    pub page: PageArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(super) struct BoardCreateArguments {
    /// Typed BoardKind JSON: area or initiative with goal and doneWhen.
    #[arg(long)]
    pub kind: String,
    /// Caller-chosen UUIDv7; generated and reported when omitted.
    #[arg(long)]
    pub board_id: Option<String>,
    #[arg(long)]
    pub workspace_id: String,
    /// Board name: 1 to 256 UTF-8 bytes after trimming.
    #[arg(long)]
    pub name: String,
    /// Board description: 0 to 16384 UTF-8 bytes.
    #[arg(long, default_value = "")]
    pub description: String,
    #[command(flatten)]
    pub identity: MutationIdentityArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(super) struct BoardUpdateArguments {
    #[arg(long)]
    pub board_id: String,
    /// Board name: 1 to 256 UTF-8 bytes after trimming.
    #[arg(long)]
    pub name: String,
    /// Board description: 0 to 16384 UTF-8 bytes.
    #[arg(long)]
    pub description: String,
    #[command(flatten)]
    pub identity: MutationIdentityArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(super) struct BoardShowArguments {
    #[arg(long)]
    pub board_id: String,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(super) struct BoardListArguments {
    #[arg(long)]
    pub workspace_id: Option<String>,
    #[arg(long)]
    pub include_archived: bool,
    #[command(flatten)]
    pub page: PageArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(super) struct BoardLifecycleArguments {
    #[arg(long)]
    pub board_id: String,
    #[command(flatten)]
    pub identity: MutationIdentityArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(super) struct ProjectCreateArguments {
    /// Caller-chosen UUIDv7; generated and reported when omitted.
    #[arg(long)]
    pub project_id: Option<String>,
    #[arg(long)]
    pub board_id: String,
    /// Project name: 1 to 256 UTF-8 bytes after trimming.
    #[arg(long)]
    pub title: String,
    /// Exact project description, at most 16 KiB; empty when omitted.
    #[arg(long)]
    pub description: Option<String>,
    #[command(flatten)]
    pub identity: MutationIdentityArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(super) struct ProjectUpdateArguments {
    #[arg(long)]
    pub project_id: String,
    /// Project name: 1 to 256 UTF-8 bytes after trimming.
    #[arg(long)]
    pub title: String,
    #[command(flatten)]
    pub identity: MutationIdentityArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(super) struct ProjectListArguments {
    #[arg(long)]
    pub board_id: String,
    #[command(flatten)]
    pub page: PageArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(super) struct ProjectShowArguments {
    #[arg(long)]
    pub project_id: String,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Clone, Copy, ValueEnum)]
pub(super) enum PlacementKind {
    Thread,
}

#[derive(Args)]
pub(super) struct ThreadPostArguments {
    /// Caller-chosen UUIDv7; generated and reported when omitted.
    #[arg(long)]
    pub post_id: Option<String>,
    /// Existing-thread placement; use thread create --about for a new root.
    #[arg(long, value_enum, default_value = "thread")]
    pub placement: PlacementKind,
    /// Root message UUIDv7, required for --placement thread.
    #[arg(long)]
    pub thread_id: Option<String>,
    /// Message text: 1 to 65536 UTF-8 bytes. Conflicts with --text-file.
    #[arg(
        long,
        conflicts_with = "text_file",
        required_unless_present = "text_file"
    )]
    pub text: Option<String>,
    /// Read 1 to 65536 UTF-8 bytes from this file. Conflicts with --text.
    #[arg(
        long,
        value_name = "PATH",
        conflicts_with = "text",
        required_unless_present = "text"
    )]
    pub text_file: Option<PathBuf>,
    /// Reference a message UUIDv7. May be repeated; at most 64 distinct references total.
    #[arg(long)]
    pub reference_post: Vec<String>,
    /// Reference a root thread UUIDv7. May be repeated; at most 64 distinct references total.
    #[arg(long)]
    pub reference_thread: Vec<String>,
    /// Explicit notification target JSON, repeated for each identity or role.
    #[arg(long = "addressee")]
    pub addressees: Vec<String>,
    #[command(flatten)]
    pub identity: MutationIdentityArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(super) struct PostShowArguments {
    #[arg(long)]
    pub post_id: String,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Clone, Copy, ValueEnum)]
pub(super) enum PostScopeKind {
    Project,
    Thread,
    Board,
    Workspace,
    AllWorkspaces,
}

#[derive(Clone, Copy, ValueEnum)]
pub(super) enum PostSelectionKind {
    Latest,
    AfterPosition,
    Range,
}

#[derive(Args)]
pub(super) struct PostListArguments {
    #[arg(long, value_enum)]
    pub scope: PostScopeKind,
    #[arg(long)]
    pub project_id: Option<String>,
    #[arg(long)]
    pub thread_id: Option<String>,
    #[arg(long)]
    pub board_id: Option<String>,
    #[arg(long)]
    pub workspace_id: Option<String>,
    /// History selection; defaults to newest messages first.
    #[arg(long, value_enum, default_value = "latest")]
    pub selection: PostSelectionKind,
    /// Exclusive lower activity bound for --selection after-position.
    #[arg(long)]
    pub after: Option<String>,
    /// Inclusive lower activity bound for --selection range.
    #[arg(long)]
    pub from: Option<String>,
    /// Inclusive upper activity bound for --selection range.
    #[arg(long)]
    pub to: Option<String>,
    #[command(flatten)]
    pub page: PageArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[path = "board_thread_arguments.rs"]
mod thread_arguments;
pub(super) use thread_arguments::*;

#[derive(Args)]
pub(super) struct ProjectClaimArguments {
    pub project: String,
    #[command(flatten)]
    pub request: super::board_request_arguments::RequestMetadataArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}
