use crate::{
    ActingForIdentity, Board, BoardId, Description, Identity, Page, PageRequest,
    ProjectDirectoryEntry, ProjectId, RepositoryRef, ResourceName, Workspace, WorkspaceId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

macro_rules! contract {
    ($name:ident { $($(#[$meta:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name { $($(#[$meta])* pub $field: $ty),* }
    };
}

contract!(WorkspaceCreateRequest {
    request_id: crate::RequestId,
    protocol_version: crate::ProtocolVersion,
    resubmits: Option<crate::RequestId>,
    workspace_id: WorkspaceId,
    name: ResourceName,
    description: Description,
    actor: Identity,
    acting_for: Option<ActingForIdentity>,
});

contract!(WorkspaceUpdateRequest {
    request_id: crate::RequestId,
    protocol_version: crate::ProtocolVersion,
    resubmits: Option<crate::RequestId>,
    workspace_id: WorkspaceId,
    name: ResourceName,
    description: Description,
    actor: Identity,
    acting_for: Option<ActingForIdentity>,
});

contract!(WorkspaceArchiveRequest {
    request_id: crate::RequestId, protocol_version: crate::ProtocolVersion,
    resubmits: Option<crate::RequestId>, workspace_id: WorkspaceId,
    actor: Identity, acting_for: Option<ActingForIdentity>,
});

contract!(WorkspaceShowRequest {
    workspace_id: WorkspaceId
});
contract!(WorkspaceShowResult {
    workspace: Workspace
});
contract!(WorkspaceListRequest { repository: Option<RepositoryRef>, page: PageRequest });
contract!(WorkspaceListResult { page: Page<Workspace> });

contract!(RepositoryAttachRequest {
    request_id: crate::RequestId,
    protocol_version: crate::ProtocolVersion,
    resubmits: Option<crate::RequestId>,
    workspace_id: WorkspaceId,
    repository: RepositoryRef,
    actor: Identity,
    acting_for: Option<ActingForIdentity>,
});

contract!(RepositoryDetachRequest {
    request_id: crate::RequestId,
    protocol_version: crate::ProtocolVersion,
    resubmits: Option<crate::RequestId>,
    workspace_id: WorkspaceId,
    repository: RepositoryRef,
    actor: Identity,
    acting_for: Option<ActingForIdentity>,
});

contract!(RepositoryListRequest {
    workspace_id: WorkspaceId,
    page: PageRequest
});
contract!(RepositoryListResult { page: Page<RepositoryRef> });

contract!(BoardCreateRequest {
    kind: crate::BoardKind,
    request_id: crate::RequestId,
    protocol_version: crate::ProtocolVersion,
    resubmits: Option<crate::RequestId>,
    board_id: BoardId,
    workspace_id: WorkspaceId,
    name: ResourceName,
    description: Description,
    actor: Identity,
    acting_for: Option<ActingForIdentity>,
});

contract!(BoardUpdateRequest {
    request_id: crate::RequestId,
    protocol_version: crate::ProtocolVersion,
    resubmits: Option<crate::RequestId>,
    board_id: BoardId,
    name: ResourceName,
    description: Description,
    actor: Identity,
    acting_for: Option<ActingForIdentity>,
});

contract!(BoardShowRequest { board_id: BoardId });
contract!(BoardShowResult { board: Board });
contract!(BoardListRequest { workspace_id: Option<WorkspaceId>, #[serde(default)] include_archived: bool, page: PageRequest });
contract!(BoardListResult { page: Page<Board> });
contract!(BoardArchiveRequest {
    request_id: crate::RequestId,
    protocol_version: crate::ProtocolVersion,
    resubmits: Option<crate::RequestId>,
    board_id: BoardId,
    actor: Identity,
    acting_for: Option<ActingForIdentity>,
});

contract!(BoardCloseRequest {
    request_id: crate::RequestId, protocol_version: crate::ProtocolVersion,
    resubmits: Option<crate::RequestId>, board_id: BoardId,
    actor: Identity, acting_for: Option<ActingForIdentity>,
});

contract!(ProjectCreateRequest {
    request_id: crate::RequestId,
    protocol_version: crate::ProtocolVersion,
    resubmits: Option<crate::RequestId>,
    project_id: ProjectId,
    board_id: BoardId,
    title: ResourceName,
    actor: Identity,
    acting_for: Option<ActingForIdentity>,
});

contract!(ProjectUpdateRequest {
    request_id: crate::RequestId,
    protocol_version: crate::ProtocolVersion,
    resubmits: Option<crate::RequestId>,
    project_id: ProjectId,
    title: ResourceName,
    actor: Identity,
    acting_for: Option<ActingForIdentity>,
});

contract!(ProjectListRequest {
    board_id: BoardId,
    page: PageRequest
});
contract!(ProjectListResult { page: Page<ProjectDirectoryEntry> });
contract!(ProjectShowRequest {
    project_id: ProjectId
});
contract!(ProjectShowResult {
    project: ProjectDirectoryEntry
});
