//! Separate location discovery and immutable-message text search.
use crate::{
    Board, BoardId, Page, PageRequest, Post, PostListScope, ProjectDirectoryEntry, SearchQuery,
    Workspace, WorkspaceId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum DiscoveryScope {
    AllWorkspaces,
    #[serde(rename_all = "camelCase")]
    Workspace {
        workspace_id: WorkspaceId,
    },
    #[serde(rename_all = "camelCase")]
    Board {
        board_id: BoardId,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum DiscoveryKind {
    #[default]
    All,
    Workspace,
    Board,
    ProjectDirectoryEntry,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SearchPostKind {
    #[default]
    Both,
    TopLevel,
    Thread,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiscoverySearchRequest {
    pub query: SearchQuery,
    pub scope: DiscoveryScope,
    #[serde(default)]
    pub kind: DiscoveryKind,
    #[serde(default)]
    pub include_archived: bool,
    pub page: PageRequest,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum DiscoveryHit {
    Workspace {
        workspace: Box<Workspace>,
    },
    Board {
        workspace: Box<Workspace>,
        board: Box<Board>,
    },
    Project {
        workspace: Box<Workspace>,
        board: Box<Board>,
        project: Box<ProjectDirectoryEntry>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiscoverySearchResult {
    pub page: Page<DiscoveryHit>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostSearchRequest {
    pub query: SearchQuery,
    pub scope: PostListScope,
    #[serde(default)]
    pub kind: SearchPostKind,
    #[serde(default)]
    pub include_archived: bool,
    pub page: PageRequest,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocatedPost {
    pub workspace_id: WorkspaceId,
    pub post: Post,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostSearchResult {
    pub page: Page<LocatedPost>,
}
