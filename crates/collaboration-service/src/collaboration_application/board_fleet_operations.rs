//! Workspace, board, repository and directory writes use the fleet port.
use super::{BoardOperations, admission_error};
use fleet_admission::{FleetIds, FleetOperation, FleetRequest};
use message_board::public_replies::WriteReply;
use message_board::*;

impl BoardOperations<'_> {
    pub(super) async fn fleet_write(
        &self,
        request: FleetRequest,
    ) -> Result<WriteReply, BoardError> {
        let reply = self
            .admission()?
            .fleet
            .submit(
                &request,
                &FleetIds {
                    generated_record: RecordId::generate(),
                },
                &self.admission()?.record_origin(),
            )
            .await
            .map_err(|error| admission_error(error.into()))?;
        self.public_write_reply(
            RequestEffectsScope::Fleet {
                workspace_id: request.workspace,
            },
            &request.request_id,
            message_board::WriteReply {
                result: reply.result,
                reader: None,
            },
        )
        .await
    }
    pub(super) fn fleet_request(
        &self,
        header: FleetHeader,
        operation: FleetOperation,
    ) -> Result<FleetRequest, BoardError> {
        Ok(FleetRequest {
            request_id: header.request_id,
            workspace: header.workspace,
            actor: RecordedActor::declared(header.actor),
            acting_for: header.acting_for,
            protocol_version: header.protocol_version,
            operation,
        })
    }
    pub(super) async fn fleet_record(&self, reference: RecordRef) -> Result<Record, BoardError> {
        self.admission()?
            .fleet
            .record(&reference)
            .await
            .map_err(|error| admission_error(error.into()))?
            .ok_or_else(|| super::missing_record(&reference))
    }
    pub async fn workspace_create(
        &self,
        request: WorkspaceCreateRequest,
    ) -> Result<WriteReply, BoardError> {
        let header = FleetHeader {
            request_id: request.request_id,
            workspace: request.workspace_id,
            actor: request.actor,
            acting_for: request.acting_for,
            protocol_version: request.protocol_version,
        };
        let request = self.fleet_request(
            header,
            FleetOperation::WorkspaceCreate {
                name: request.name,
                description: request.description,
            },
        )?;
        self.fleet_write(request).await
    }
    pub async fn workspace_archive(
        &self,
        request: WorkspaceArchiveRequest,
    ) -> Result<WriteReply, BoardError> {
        let header = FleetHeader {
            request_id: request.request_id,
            workspace: request.workspace_id,
            actor: request.actor,
            acting_for: request.acting_for,
            protocol_version: request.protocol_version,
        };
        self.fleet_write(self.fleet_request(header, FleetOperation::WorkspaceArchive {})?)
            .await
    }
    pub async fn workspace_update(
        &self,
        request: WorkspaceUpdateRequest,
    ) -> Result<WriteReply, BoardError> {
        let header = FleetHeader {
            request_id: request.request_id,
            workspace: request.workspace_id,
            actor: request.actor,
            acting_for: request.acting_for,
            protocol_version: request.protocol_version,
        };
        self.fleet_write(self.fleet_request(
            header,
            FleetOperation::WorkspaceUpdate {
                name: request.name,
                description: request.description,
            },
        )?)
        .await
    }
    pub async fn repository_attach(
        &self,
        request: RepositoryAttachRequest,
    ) -> Result<WriteReply, BoardError> {
        self.require_local_repository(&request.repository)?;
        let header = FleetHeader {
            request_id: request.request_id,
            workspace: request.workspace_id,
            actor: request.actor,
            acting_for: request.acting_for,
            protocol_version: request.protocol_version,
        };
        self.fleet_write(self.fleet_request(
            header,
            FleetOperation::RepositoryAttach {
                repository: request.repository,
            },
        )?)
        .await
    }
    pub async fn repository_detach(
        &self,
        request: RepositoryDetachRequest,
    ) -> Result<WriteReply, BoardError> {
        self.require_local_repository(&request.repository)?;
        let header = FleetHeader {
            request_id: request.request_id,
            workspace: request.workspace_id,
            actor: request.actor,
            acting_for: request.acting_for,
            protocol_version: request.protocol_version,
        };
        self.fleet_write(self.fleet_request(
            header,
            FleetOperation::RepositoryDetach {
                repository: request.repository,
            },
        )?)
        .await
    }
    pub(super) fn require_local_repository(
        &self,
        repository: &RepositoryRef,
    ) -> Result<(), BoardError> {
        if let RepositoryRef::Local { service_id, .. } = repository
            && service_id.as_str() != String::from(self.identity.service_id.clone())
        {
            return Err(BoardError::invalid_field(
                "repository.serviceId",
                "must match the selected board service",
            ));
        }
        Ok(())
    }
    pub async fn board_create(
        &self,
        request: BoardCreateRequest,
    ) -> Result<WriteReply, BoardError> {
        let header = FleetHeader {
            request_id: request.request_id,
            workspace: request.workspace_id,
            actor: request.actor,
            acting_for: request.acting_for,
            protocol_version: request.protocol_version,
        };
        self.fleet_write(self.fleet_request(
            header,
            FleetOperation::BoardCreate {
                board: request.board_id,
                name: request.name,
                description: request.description,
                board_kind: request.kind,
            },
        )?)
        .await
    }
    pub async fn board_update(
        &self,
        request: BoardUpdateRequest,
    ) -> Result<WriteReply, BoardError> {
        let Record::Board(board) = self.fleet_record(request.board_id.clone().into()).await? else {
            return Err(BoardError::invalid_field("boardId", "is not a board"));
        };
        let header = FleetHeader {
            request_id: request.request_id,
            workspace: board.workspace_id,
            actor: request.actor,
            acting_for: request.acting_for,
            protocol_version: request.protocol_version,
        };
        self.fleet_write(self.fleet_request(
            header,
            FleetOperation::BoardUpdate {
                board: request.board_id,
                name: request.name,
                description: request.description,
            },
        )?)
        .await
    }
    pub async fn board_close(&self, request: BoardCloseRequest) -> Result<WriteReply, BoardError> {
        let Record::Board(board) = self.fleet_record(request.board_id.clone().into()).await? else {
            return Err(BoardError::invalid_field("boardId", "is not a board"));
        };
        let header = FleetHeader {
            request_id: request.request_id,
            workspace: board.workspace_id,
            actor: request.actor,
            acting_for: request.acting_for,
            protocol_version: request.protocol_version,
        };
        self.fleet_write(self.fleet_request(
            header,
            FleetOperation::BoardClose {
                board: request.board_id,
            },
        )?)
        .await
    }
    pub async fn board_archive(
        &self,
        request: BoardArchiveRequest,
    ) -> Result<WriteReply, BoardError> {
        let Record::Board(board) = self.fleet_record(request.board_id.clone().into()).await? else {
            return Err(BoardError::invalid_field("boardId", "is not a board"));
        };
        let header = FleetHeader {
            request_id: request.request_id,
            workspace: board.workspace_id,
            actor: request.actor,
            acting_for: request.acting_for,
            protocol_version: request.protocol_version,
        };
        self.fleet_write(self.fleet_request(
            header,
            FleetOperation::BoardArchive {
                board: request.board_id,
            },
        )?)
        .await
    }
    pub async fn project_create(
        &self,
        request: ProjectCreateRequest,
    ) -> Result<WriteReply, BoardError> {
        let reply = self
            .admission()?
            .create_project(&request)
            .await
            .map_err(admission_error)?;
        self.public_write_reply(
            RequestEffectsScope::Project {
                project_id: request.project_id,
            },
            &request.request_id,
            reply,
        )
        .await
    }
    pub async fn project_update(
        &self,
        request: ProjectUpdateRequest,
    ) -> Result<WriteReply, BoardError> {
        let Record::DirectoryEntry(project) =
            self.fleet_record(request.project_id.clone().into()).await?
        else {
            return Err(BoardError::invalid_field(
                "projectId",
                "is not a directory entry",
            ));
        };
        let header = FleetHeader {
            request_id: request.request_id,
            workspace: project.envelope.scope.workspace,
            actor: request.actor,
            acting_for: request.acting_for,
            protocol_version: request.protocol_version,
        };
        self.fleet_write(self.fleet_request(
            header,
            FleetOperation::ProjectDirectoryUpdate {
                project: request.project_id,
                board: project.board,
                title: request.title,
            },
        )?)
        .await
    }
}
pub(super) struct FleetHeader {
    pub(super) request_id: RequestId,
    pub(super) workspace: WorkspaceId,
    pub(super) actor: Identity,
    pub(super) acting_for: Option<ActingForIdentity>,
    pub(super) protocol_version: ProtocolVersion,
}
