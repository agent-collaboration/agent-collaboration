//! Typed fleet board calls; uncertain writes are never retransmitted.
use super::{BoardClientError, CollaborationClient};
use message_board::public_replies::WriteReply;
use message_board::{
    AdrCreateRequest, AdrTransitionRequest, BoardArchiveRequest, BoardCloseRequest,
    BoardCreateRequest, BoardListRequest, BoardListResult, BoardShowRequest, BoardShowResult,
    BoardUpdateRequest, FleetRecordListRequest, FleetRecordShowRequest, ProjectCreateRequest,
    ProjectListRequest, ProjectListResult, ProjectShowRequest, ProjectShowResult,
    ProjectUpdateRequest, Record, RecordListResult, RepositoryAttachRequest,
    RepositoryDetachRequest, RepositoryListRequest, RepositoryListResult, ResourceIdentity,
    WorkspaceArchiveRequest, WorkspaceCreateRequest, WorkspaceListRequest, WorkspaceListResult,
    WorkspaceShowRequest, WorkspaceShowResult, WorkspaceUpdateRequest,
};
impl CollaborationClient {
    pub async fn project_show(
        &self,
        request: ProjectShowRequest,
    ) -> Result<ProjectShowResult, BoardClientError> {
        self.board_call("project_show", request).await
    }
    pub async fn adr_create(
        &self,
        request: AdrCreateRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Workspace {
            workspace_id: request.workspace_id.clone(),
        };
        self.board_mutation_call("adr_create", request, resource)
            .await
    }

    pub async fn adr_transition(
        &self,
        request: AdrTransitionRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Workspace {
            workspace_id: request.workspace_id.clone(),
        };
        self.board_mutation_call("adr_transition", request, resource)
            .await
    }

    pub async fn adr_show(
        &self,
        request: FleetRecordShowRequest,
    ) -> Result<Record, BoardClientError> {
        self.board_call("adr_show", request).await
    }
    pub async fn adr_list(
        &self,
        request: FleetRecordListRequest,
    ) -> Result<RecordListResult, BoardClientError> {
        self.board_call("adr_list", request).await
    }

    pub async fn workspace_create(
        &self,
        request: WorkspaceCreateRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Workspace {
            workspace_id: request.workspace_id.clone(),
        };
        self.board_mutation_call("workspace_create", request, resource)
            .await
    }
    pub async fn workspace_archive(
        &self,
        request: WorkspaceArchiveRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Workspace {
            workspace_id: request.workspace_id.clone(),
        };
        self.board_mutation_call("workspace_archive", request, resource)
            .await
    }
    pub async fn workspace_update(
        &self,
        request: WorkspaceUpdateRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Workspace {
            workspace_id: request.workspace_id.clone(),
        };
        self.board_mutation_call("workspace_update", request, resource)
            .await
    }
    pub async fn workspace_show(
        &self,
        request: WorkspaceShowRequest,
    ) -> Result<WorkspaceShowResult, BoardClientError> {
        self.board_call("workspace_show", request).await
    }
    pub async fn workspace_list(
        &self,
        request: WorkspaceListRequest,
    ) -> Result<WorkspaceListResult, BoardClientError> {
        self.board_call("workspace_list", request).await
    }
    pub async fn repository_attach(
        &self,
        request: RepositoryAttachRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Workspace {
            workspace_id: request.workspace_id.clone(),
        };
        self.board_mutation_call("repository_attach", request, resource)
            .await
    }
    pub async fn repository_detach(
        &self,
        request: RepositoryDetachRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Workspace {
            workspace_id: request.workspace_id.clone(),
        };
        self.board_mutation_call("repository_detach", request, resource)
            .await
    }
    pub async fn repository_list(
        &self,
        request: RepositoryListRequest,
    ) -> Result<RepositoryListResult, BoardClientError> {
        self.board_call("repository_list", request).await
    }
    pub async fn board_create(
        &self,
        request: BoardCreateRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Board {
            board_id: request.board_id.clone(),
        };
        self.board_mutation_call("board_create", request, resource)
            .await
    }
    pub async fn board_update(
        &self,
        request: BoardUpdateRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Board {
            board_id: request.board_id.clone(),
        };
        self.board_mutation_call("board_update", request, resource)
            .await
    }
    pub async fn board_show(
        &self,
        request: BoardShowRequest,
    ) -> Result<BoardShowResult, BoardClientError> {
        self.board_call("board_show", request).await
    }
    pub async fn board_list(
        &self,
        request: BoardListRequest,
    ) -> Result<BoardListResult, BoardClientError> {
        self.board_call("board_list", request).await
    }
    pub async fn board_close(
        &self,
        request: BoardCloseRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Board {
            board_id: request.board_id.clone(),
        };
        self.board_mutation_call("board_close", request, resource)
            .await
    }
    pub async fn board_archive(
        &self,
        request: BoardArchiveRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Board {
            board_id: request.board_id.clone(),
        };
        self.board_mutation_call("board_archive", request, resource)
            .await
    }
    pub async fn project_claim(
        &self,
        request: board_sync_protocol::ProjectClaimRequest,
    ) -> Result<board_sync_protocol::ProjectClaimReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("project_claim", request, resource)
            .await
    }
    pub async fn project_create(
        &self,
        request: ProjectCreateRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("project_create", request, resource)
            .await
    }
    pub async fn project_update(
        &self,
        request: ProjectUpdateRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("project_update", request, resource)
            .await
    }
    pub async fn project_list(
        &self,
        request: ProjectListRequest,
    ) -> Result<ProjectListResult, BoardClientError> {
        self.board_call("project_list", request).await
    }
}
