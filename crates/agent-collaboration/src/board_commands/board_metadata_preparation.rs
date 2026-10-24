//! Prepare typed metadata operations before transmission.
use super::*;
pub(super) fn prepare_board_metadata(
    command: BoardMetadataCommand,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    match command {
        BoardMetadataCommand::Create(arguments) => {
            let (actor, acting_for) = parse_mutation_identity(&arguments.identity)?;
            let request = BoardCreateRequest {
                kind: serde_json::from_str(&arguments.kind)
                    .map_err(|_| "--kind must be typed BoardKind JSON".to_owned())?,
                request_id: arguments.identity.request.request_id()?,
                protocol_version: ProtocolVersion::CURRENT,
                board_id: parse_or_generate_uuid_v7(arguments.board_id, "--board-id")?,
                workspace_id: parse_uuid_v7(arguments.workspace_id, "--workspace-id")?,
                name: parse_bounded_text(arguments.name, "--name")?,
                description: parse_bounded_text(arguments.description, "--description")?,
                actor,
                acting_for,
            };
            Ok((
                PreparedBoardCommand::BoardCreate(request),
                command_context(arguments.common),
            ))
        }
        BoardMetadataCommand::Update(arguments) => {
            let (actor, acting_for) = parse_mutation_identity(&arguments.identity)?;
            let request = BoardUpdateRequest {
                request_id: arguments.identity.request.request_id()?,
                protocol_version: ProtocolVersion::CURRENT,
                board_id: parse_uuid_v7(arguments.board_id, "--board-id")?,
                name: parse_bounded_text(arguments.name, "--name")?,
                description: parse_bounded_text(arguments.description, "--description")?,
                actor,
                acting_for,
            };
            Ok((
                PreparedBoardCommand::BoardUpdate(request),
                command_context(arguments.common),
            ))
        }
        BoardMetadataCommand::Show(arguments) => Ok((
            PreparedBoardCommand::BoardShow(BoardShowRequest {
                board_id: parse_uuid_v7(arguments.board_id, "--board-id")?,
            }),
            command_context(arguments.common),
        )),
        BoardMetadataCommand::List(arguments) => Ok((
            PreparedBoardCommand::BoardList(BoardListRequest {
                workspace_id: arguments
                    .workspace_id
                    .map(|value| parse_uuid_v7(value, "--workspace-id"))
                    .transpose()?,
                include_archived: arguments.include_archived,
                page: prepare_page_request(arguments.page)?,
            }),
            command_context(arguments.common),
        )),
        BoardMetadataCommand::Archive(arguments) => {
            let (actor, acting_for) = parse_mutation_identity(&arguments.identity)?;
            Ok((
                PreparedBoardCommand::BoardArchive(BoardArchiveRequest {
                    request_id: arguments.identity.request.request_id()?,
                    protocol_version: ProtocolVersion::CURRENT,
                    board_id: parse_uuid_v7(arguments.board_id, "--board-id")?,
                    actor,
                    acting_for,
                }),
                command_context(arguments.common),
            ))
        }
        BoardMetadataCommand::Close(arguments) => {
            let (actor, acting_for) = parse_mutation_identity(&arguments.identity)?;
            Ok((
                PreparedBoardCommand::BoardClose(BoardCloseRequest {
                    request_id: arguments.identity.request.request_id()?,
                    protocol_version: ProtocolVersion::CURRENT,
                    board_id: parse_uuid_v7(arguments.board_id, "--board-id")?,
                    actor,
                    acting_for,
                }),
                command_context(arguments.common),
            ))
        }
    }
}

pub(super) fn prepare_workspace(
    command: WorkspaceCommand,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    match command {
        WorkspaceCommand::Create(arguments) => {
            let (actor, acting_for) = parse_mutation_identity(&arguments.identity)?;
            Ok((
                PreparedBoardCommand::WorkspaceCreate(WorkspaceCreateRequest {
                    request_id: arguments.identity.request.request_id()?,
                    protocol_version: ProtocolVersion::CURRENT,
                    workspace_id: parse_or_generate_uuid_v7(
                        arguments.workspace_id,
                        "--workspace-id",
                    )?,
                    name: parse_bounded_text(arguments.name, "--name")?,
                    description: parse_bounded_text(arguments.description, "--description")?,
                    actor,
                    acting_for,
                }),
                command_context(arguments.common),
            ))
        }
        WorkspaceCommand::Update(arguments) => {
            let (actor, acting_for) = parse_mutation_identity(&arguments.identity)?;
            Ok((
                PreparedBoardCommand::WorkspaceUpdate(WorkspaceUpdateRequest {
                    request_id: arguments.identity.request.request_id()?,
                    protocol_version: ProtocolVersion::CURRENT,
                    workspace_id: parse_uuid_v7(arguments.workspace_id, "--workspace-id")?,
                    name: parse_bounded_text(arguments.name, "--name")?,
                    description: parse_bounded_text(arguments.description, "--description")?,
                    actor,
                    acting_for,
                }),
                command_context(arguments.common),
            ))
        }
        WorkspaceCommand::Archive(arguments) => {
            let (actor, acting_for) = parse_mutation_identity(&arguments.identity)?;
            Ok((
                PreparedBoardCommand::WorkspaceArchive(WorkspaceArchiveRequest {
                    request_id: arguments.identity.request.request_id()?,
                    protocol_version: ProtocolVersion::CURRENT,
                    workspace_id: parse_uuid_v7(arguments.workspace_id, "--workspace-id")?,
                    actor,
                    acting_for,
                }),
                command_context(arguments.common),
            ))
        }
        WorkspaceCommand::Show(arguments) => Ok((
            PreparedBoardCommand::WorkspaceShow(WorkspaceShowRequest {
                workspace_id: parse_uuid_v7(arguments.workspace_id, "--workspace-id")?,
            }),
            command_context(arguments.common),
        )),
        WorkspaceCommand::List(arguments) => Ok((
            PreparedBoardCommand::WorkspaceList {
                repository: optional_repository(arguments.repository)?,
                page: prepare_page_request(arguments.page)?,
            },
            command_context(arguments.common),
        )),
    }
}

pub(super) fn prepare_repository_command(
    command: RepositoryCommand,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    match command {
        RepositoryCommand::Attach(arguments) => {
            let (actor, acting_for) = parse_mutation_identity(&arguments.identity)?;
            let workspace_id = parse_uuid_v7(arguments.workspace_id, "--workspace-id")?;
            let repository = required_repository(arguments.repository)?;
            Ok((
                PreparedBoardCommand::RepositoryAttach {
                    request: arguments.identity.request,
                    workspace_id,
                    repository,
                    actor,
                    acting_for,
                },
                command_context(arguments.common),
            ))
        }
        RepositoryCommand::Detach(arguments) => {
            let (actor, acting_for) = parse_mutation_identity(&arguments.identity)?;
            let workspace_id = parse_uuid_v7(arguments.workspace_id, "--workspace-id")?;
            let repository = required_repository(arguments.repository)?;
            Ok((
                PreparedBoardCommand::RepositoryDetach {
                    request: arguments.identity.request,
                    workspace_id,
                    repository,
                    actor,
                    acting_for,
                },
                command_context(arguments.common),
            ))
        }
        RepositoryCommand::List(arguments) => Ok((
            PreparedBoardCommand::RepositoryList(RepositoryListRequest {
                workspace_id: parse_uuid_v7(arguments.workspace_id, "--workspace-id")?,
                page: prepare_page_request(arguments.page)?,
            }),
            command_context(arguments.common),
        )),
    }
}

pub(super) fn prepare_project(
    command: ProjectCommand,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    match command {
        ProjectCommand::NotificationCancel(args) => {
            super::super::board_entity_preparation::file_write(
                args,
                super::super::board_entity_preparation::ProjectWriteKind::ProjectNotificationCancel,
            )
        }
        ProjectCommand::Show(arguments) => Ok((
            PreparedBoardCommand::ProjectShow(ProjectShowRequest {
                project_id: parse_uuid_v7(arguments.project_id, "--project-id")?,
            }),
            command_context(arguments.common),
        )),
        ProjectCommand::Describe(args) => super::super::board_entity_preparation::file_write(
            args,
            super::super::board_entity_preparation::ProjectWriteKind::ProjectDescribe,
        ),
        ProjectCommand::PinDefinition(args) => super::super::board_entity_preparation::file_write(
            args,
            super::super::board_entity_preparation::ProjectWriteKind::ProjectPinDefinition,
        ),
        ProjectCommand::PublishPlan(args) => super::super::board_entity_preparation::file_write(
            args,
            super::super::board_entity_preparation::ProjectWriteKind::ProjectPublishPlan,
        ),
        ProjectCommand::Close(args) => super::super::board_entity_preparation::file_write(
            args,
            super::super::board_entity_preparation::ProjectWriteKind::ProjectClose,
        ),
        ProjectCommand::Claim(arguments) => Ok((
            PreparedBoardCommand::ProjectClaim(board_sync_protocol::ProjectClaimRequest {
                request_id: arguments.request.request_id()?,
                protocol_version: ProtocolVersion::CURRENT,
                project_id: parse_uuid_v7(arguments.project, "PROJECT")?,
            }),
            command_context(arguments.common),
        )),
        ProjectCommand::Create(arguments) => {
            let (actor, acting_for) = parse_mutation_identity(&arguments.identity)?;
            Ok((
                PreparedBoardCommand::ProjectCreate(ProjectCreateRequest {
                    request_id: arguments.identity.request.request_id()?,
                    protocol_version: ProtocolVersion::CURRENT,
                    project_id: parse_or_generate_uuid_v7(arguments.project_id, "--project-id")?,
                    board_id: parse_uuid_v7(arguments.board_id, "--board-id")?,
                    title: parse_bounded_text(arguments.title, "--title")?,
                    description: arguments
                        .description
                        .map(|value| parse_bounded_text(value, "--description"))
                        .transpose()?,
                    actor,
                    acting_for,
                }),
                command_context(arguments.common),
            ))
        }
        ProjectCommand::Update(arguments) => {
            let (actor, acting_for) = parse_mutation_identity(&arguments.identity)?;
            Ok((
                PreparedBoardCommand::ProjectUpdate(ProjectUpdateRequest {
                    request_id: arguments.identity.request.request_id()?,
                    protocol_version: ProtocolVersion::CURRENT,
                    project_id: parse_uuid_v7(arguments.project_id, "--project-id")?,
                    title: parse_bounded_text(arguments.title, "--title")?,
                    actor,
                    acting_for,
                }),
                command_context(arguments.common),
            ))
        }
        ProjectCommand::List(arguments) => Ok((
            PreparedBoardCommand::ProjectList(ProjectListRequest {
                board_id: parse_uuid_v7(arguments.board_id, "--board-id")?,
                page: prepare_page_request(arguments.page)?,
            }),
            command_context(arguments.common),
        )),
    }
}
