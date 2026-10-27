use super::board_arguments::{CommonArguments, PageArguments, PostScopeKind};
use super::board_preparation::{CommandContext, PreparedBoardCommand};
use super::board_value_parsing::{
    command_context, parse_bounded_text, parse_uuid_v7, prepare_page_request,
};
use clap::{Args, ValueEnum};
use collaboration_client::board::{
    DiscoveryKind, DiscoveryScope, DiscoverySearchRequest, PostListScope, PostSearchRequest,
    SearchPostKind,
};

#[derive(Clone, Copy, ValueEnum)]
pub(super) enum DiscoveryScopeKind {
    AllWorkspaces,
    Workspace,
    Board,
}
#[derive(Clone, Copy, ValueEnum)]
pub(super) enum DiscoveryResultKind {
    All,
    Workspace,
    Board,
    Project,
}
#[derive(Clone, Copy, ValueEnum)]
pub(super) enum MessageResultKind {
    Both,
    TopLevel,
    Thread,
}

#[derive(Args)]
pub(super) struct DiscoverySearchArguments {
    /// Literal substring; 1 to 256 UTF-8 bytes after trimming. ASCII case-insensitive.
    #[arg(long)]
    pub query: String,
    #[arg(long, value_enum, default_value = "all-workspaces")]
    pub scope: DiscoveryScopeKind,
    #[arg(long)]
    pub workspace_id: Option<String>,
    #[arg(long)]
    pub board_id: Option<String>,
    #[arg(long, value_enum, default_value = "all")]
    pub kind: DiscoveryResultKind,
    #[arg(long)]
    pub include_archived: bool,
    #[command(flatten)]
    pub page: PageArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}

#[derive(Args)]
pub(super) struct PostSearchArguments {
    /// Literal substring; 1 to 256 UTF-8 bytes after trimming. ASCII case-insensitive.
    #[arg(long)]
    pub query: String,
    #[arg(long, value_enum, default_value = "all-workspaces")]
    pub scope: PostScopeKind,
    #[arg(long)]
    pub workspace_id: Option<String>,
    #[arg(long)]
    pub board_id: Option<String>,
    #[arg(long)]
    pub project_id: Option<String>,
    #[arg(long)]
    pub thread_id: Option<String>,
    #[arg(long, value_enum, default_value = "both")]
    pub kind: MessageResultKind,
    #[arg(long)]
    pub include_archived: bool,
    #[command(flatten)]
    pub page: PageArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}

pub(super) fn prepare_discovery_search(
    arguments: DiscoverySearchArguments,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    let scope = match (arguments.scope, arguments.workspace_id, arguments.board_id) {
        (DiscoveryScopeKind::AllWorkspaces, None, None) => DiscoveryScope::AllWorkspaces,
        (DiscoveryScopeKind::Workspace, Some(value), None) => DiscoveryScope::Workspace {
            workspace_id: parse_uuid_v7(value, "--workspace-id")?,
        },
        (DiscoveryScopeKind::Board, None, Some(value)) => DiscoveryScope::Board {
            board_id: parse_uuid_v7(value, "--board-id")?,
        },
        _ => {
            return Err(
                "--scope requires exactly its matching ID (or none for all-workspaces)".into(),
            );
        }
    };
    let kind = match arguments.kind {
        DiscoveryResultKind::All => DiscoveryKind::All,
        DiscoveryResultKind::Workspace => DiscoveryKind::Workspace,
        DiscoveryResultKind::Board => DiscoveryKind::Board,
        DiscoveryResultKind::Project => DiscoveryKind::ProjectDirectoryEntry,
    };
    if matches!(scope, DiscoveryScope::Board { .. }) && kind == DiscoveryKind::Workspace {
        return Err("--kind workspace cannot be used within a board scope".into());
    }
    Ok((
        PreparedBoardCommand::DiscoverySearch(DiscoverySearchRequest {
            query: parse_bounded_text(arguments.query, "--query")?,
            scope,
            kind,
            include_archived: arguments.include_archived,
            page: prepare_page_request(arguments.page)?,
        }),
        command_context(arguments.common),
    ))
}

pub(super) fn prepare_message_search(
    arguments: PostSearchArguments,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    let scope = match (
        arguments.scope,
        arguments.workspace_id,
        arguments.board_id,
        arguments.project_id,
        arguments.thread_id,
    ) {
        (PostScopeKind::AllWorkspaces, None, None, None, None) => PostListScope::AllWorkspaces,
        (PostScopeKind::Workspace, Some(value), None, None, None) => PostListScope::Workspace {
            workspace_id: parse_uuid_v7(value, "--workspace-id")?,
        },
        (PostScopeKind::Board, None, Some(value), None, None) => PostListScope::Board {
            board_id: parse_uuid_v7(value, "--board-id")?,
        },
        (PostScopeKind::Project, None, None, Some(value), None) => PostListScope::Project {
            project_id: parse_uuid_v7(value, "--project-id")?,
        },
        (PostScopeKind::Thread, None, None, None, Some(value)) => PostListScope::Thread {
            thread: super::board_value_parsing::parse_thread_ref(value)?,
        },
        _ => {
            return Err(
                "--scope requires exactly its matching ID (or none for all-workspaces)".into(),
            );
        }
    };
    let kind = match arguments.kind {
        MessageResultKind::Both => SearchPostKind::Both,
        MessageResultKind::TopLevel => SearchPostKind::TopLevel,
        MessageResultKind::Thread => SearchPostKind::Thread,
    };
    if matches!(scope, PostListScope::Thread { .. }) && kind == SearchPostKind::TopLevel {
        return Err("--kind top-level cannot be used within a thread scope".into());
    }
    Ok((
        PreparedBoardCommand::PostSearch(PostSearchRequest {
            query: parse_bounded_text(arguments.query, "--query")?,
            scope,
            kind,
            include_archived: arguments.include_archived,
            page: prepare_page_request(arguments.page)?,
        }),
        command_context(arguments.common),
    ))
}
