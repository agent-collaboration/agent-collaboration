#[path = "board_inbox_preparation.rs"]
mod board_inbox_preparation;
use board_inbox_preparation::prepare_inbox;
#[path = "board_thread_preparation.rs"]
mod board_thread_preparation;
use board_thread_preparation::{prepare_message, prepare_thread};
#[path = "board_metadata_preparation.rs"]
mod board_metadata_preparation;
use super::board_arguments::{
    BoardCommand, BoardMetadataCommand, CommonArguments, InboxCommand, ParticipantRoleKind,
    PlacementKind, PostCommand, ProjectCommand, RepositoryCommand, ThreadCommand,
    ThreadCreateArguments, ThreadJoinArguments, ThreadLeaveArguments, ThreadMutationArguments,
    ThreadParticipantCommand, ThreadWatchArguments, WorkspaceCommand,
};
use super::board_inbox_arguments::InboxModeKind;
use super::board_thread_subscription_preparation::PendingSubscriptionActor;
pub(super) use super::board_thread_wait_preparation::PendingSubscriptionWait;
use super::board_value_parsing::{
    command_context, inbox_scope, message_scope, message_selection, optional_repository,
    parse_acting_for, parse_addressees, parse_bounded_text, parse_identity,
    parse_mutation_identity, parse_or_generate_uuid_v7, parse_thread_ref, parse_thread_subject,
    parse_uuid_v7, prepare_page_request, read_actor, read_message_file, required_repository,
};
use board_metadata_preparation::{
    prepare_board_metadata, prepare_project, prepare_repository_command, prepare_workspace,
};
use collaboration_client::BoardRepositoryLocation;
use collaboration_client::CollaborationClient;
use collaboration_client::board::{
    AcknowledgementEntry, ActingForIdentity, BoardArchiveRequest, BoardCloseRequest,
    BoardCreateRequest, BoardListRequest, BoardShowRequest, BoardUpdateRequest,
    DiscoverySearchRequest, EndpointId, HumanId, Identity, InboxAcknowledgeRequest,
    InboxAcknowledgeRequestProps, InboxFetchRequest, InboxFetchRequestProps, InboxReadMode,
    InboxScope, InboxWorkspacesRequest, InvalidPostReferences, LeaveFollowUp, PageRequest,
    ParticipantRole, PostListRequest, PostReferences, PostSearchRequest, PostShowRequest,
    PostTarget, ProjectCreateRequest, ProjectListRequest, ProjectShowRequest, ProjectUpdateRequest,
    ProjectWatchRequest, ProtocolVersion, ReadActor, ReadInitializationEntry, ReferenceTarget,
    RepositoryListRequest, RequestEffectsRequest, RequestEffectsScope, RequestId, ServiceId,
    SessionEndpointRef, SessionId, SessionRef, TextValidationError, ThreadCreateRequest,
    ThreadJoinRequest, ThreadLeaveRequest, ThreadListRequest, ThreadParticipantListRequest,
    ThreadPostRequest, ThreadRef, ThreadRefileRequest, ThreadResolutionReason,
    ThreadResolveRequest, ThreadShowRequest, ThreadUnresolveRequest, ThreadUnwatchRequest,
    ThreadWatchRequest, WorkspaceArchiveRequest, WorkspaceCreateRequest, WorkspaceId,
    WorkspaceShowRequest, WorkspaceUpdateRequest,
};
use collaboration_client::protocol::{
    ThreadSubscribeRequest, ThreadSubscriptionsRequest, ThreadUnsubscribeRequest,
};
use std::path::PathBuf;

pub(super) struct CommandContext {
    pub service_directory: Option<PathBuf>,
    pub json: bool,
}

pub(super) enum PreparedBoardCommand {
    RequestEffects(RequestEffectsRequest),
    View(Box<super::board_view_commands::PreparedViewCommand>),
    Entity(Box<super::board_entity_preparation::PreparedEntityCommand>),
    DiscoverySearch(DiscoverySearchRequest),
    PostSearch(PostSearchRequest),
    WorkspaceCreate(WorkspaceCreateRequest),
    WorkspaceUpdate(WorkspaceUpdateRequest),
    WorkspaceArchive(WorkspaceArchiveRequest),
    WorkspaceShow(WorkspaceShowRequest),
    WorkspaceList {
        repository: Option<BoardRepositoryLocation>,
        page: PageRequest,
    },
    RepositoryAttach {
        request: super::board_request_arguments::RequestMetadataArguments,
        workspace_id: WorkspaceId,
        repository: BoardRepositoryLocation,
        actor: Identity,
        acting_for: Option<ActingForIdentity>,
    },
    RepositoryDetach {
        request: super::board_request_arguments::RequestMetadataArguments,
        workspace_id: WorkspaceId,
        repository: BoardRepositoryLocation,
        actor: Identity,
        acting_for: Option<ActingForIdentity>,
    },
    RepositoryList(RepositoryListRequest),
    BoardCreate(BoardCreateRequest),
    BoardUpdate(BoardUpdateRequest),
    BoardShow(BoardShowRequest),
    BoardList(BoardListRequest),
    BoardArchive(BoardArchiveRequest),
    BoardClose(BoardCloseRequest),
    ProjectClaim(board_sync_protocol::ProjectClaimRequest),
    ProjectCreate(ProjectCreateRequest),
    ProjectUpdate(ProjectUpdateRequest),
    ProjectList(ProjectListRequest),
    ProjectShow(ProjectShowRequest),
    ThreadPost(PendingThreadPost),
    PostShow(PostShowRequest),
    PostList(PostListRequest),
    ThreadShow(ThreadShowRequest),
    ThreadResolve(ThreadResolveRequest),
    ThreadRefile(ThreadRefileRequest),
    ThreadUnresolve(ThreadUnresolveRequest),
    ThreadWatch(ThreadWatchRequest),
    ThreadUnwatch(ThreadUnwatchRequest),
    ProjectWatch(ProjectWatchRequest),
    ProjectUnwatch(ProjectWatchRequest),
    ThreadList(ThreadListRequest),
    RepositoryThreadList {
        repository: BoardRepositoryLocation,
        reader: Option<Identity>,
        page: PageRequest,
        default_repository_path: Option<String>,
    },
    ThreadCreate(Box<PendingThreadCreate>),
    ThreadJoin(Box<PendingThreadJoin>),
    ThreadSubscribe(PendingSubscriptionActor<ThreadSubscribeRequest>),
    ThreadUnsubscribe(PendingSubscriptionActor<ThreadUnsubscribeRequest>),
    ThreadSubscriptions(PendingSubscriptionActor<ThreadSubscriptionsRequest>),
    ThreadSubscriptionWait(PendingSubscriptionWait),
    ThreadLeave(PendingThreadLeave),
    ThreadParticipantList(ThreadParticipantListRequest),
    InboxFetch(InboxFetchRequest),
    InboxAcknowledge(InboxAcknowledgeRequest),
    InboxWorkspaces(InboxWorkspacesRequest),
}

#[derive(Clone)]
pub(super) enum ActorInput {
    Explicit(Identity),
    Self_,
}

pub(super) struct PendingThreadCreate {
    pub request: ThreadCreateRequest,
    pub actor: ActorInput,
}

pub(super) struct PendingThreadPost {
    pub request: ThreadPostRequest,
    pub text_file: Option<PathBuf>,
}

pub(super) struct PendingThreadJoin {
    pub request: ThreadJoinRequest,
    pub actor: ActorInput,
}

pub(super) struct PendingThreadLeave {
    pub request: ThreadLeaveRequest,
    pub actor: ActorInput,
}

pub(super) fn finalize_actor(
    actor: &ActorInput,
    client: &CollaborationClient,
) -> Result<Identity, String> {
    match actor {
        ActorInput::Explicit(identity) => Ok(identity.clone()),
        ActorInput::Self_ => self_identity(client),
    }
}

fn self_identity(client: &CollaborationClient) -> Result<Identity, String> {
    let harness = collaboration_client::current_session_identity::read_harness_session_identity()
        .map_err(|error| format!("--actor self: {error}"))?;
    let service_id: ServiceId = String::from(client.identity().service_id.clone())
        .try_into()
        .map_err(|_| "--actor self could not use the verified service identity".to_owned())?;
    let session_id = SessionId::try_from(harness.session_id).map_err(|_| {
        format!(
            "--actor self requires {} to contain a non-empty session ID without NUL",
            harness.harness.environment_variable
        )
    })?;
    let endpoint_id = EndpointId::try_from(harness.harness.endpoint_id.to_owned())
        .map_err(|_| "--actor self could not construct the local session endpoint".to_owned())?;
    Ok(Identity::Session {
        session: SessionRef {
            endpoint: SessionEndpointRef {
                service_id,
                endpoint_id,
            },
            session_id,
        },
    })
}

pub(super) fn finalize_identity(
    identity: &mut Identity,
    client: &CollaborationClient,
) -> Result<(), String> {
    if identity
        .as_human()
        .is_some_and(|id| id.as_str() == "__agent_collaboration_self__")
    {
        *identity = self_identity(client)?;
    }
    Ok(())
}

pub(super) fn finalize_command(
    command: &mut PreparedBoardCommand,
    client: &CollaborationClient,
) -> Result<(), String> {
    match command {
        PreparedBoardCommand::Entity(command) => command.finalize(client)?,
        PreparedBoardCommand::View(command) => command.finalize(client)?,
        PreparedBoardCommand::RepositoryAttach { actor, .. }
        | PreparedBoardCommand::RepositoryDetach { actor, .. } => finalize_identity(actor, client)?,
        PreparedBoardCommand::WorkspaceCreate(request) => {
            finalize_identity(&mut request.actor, client)?
        }
        PreparedBoardCommand::WorkspaceUpdate(request) => {
            finalize_identity(&mut request.actor, client)?
        }
        PreparedBoardCommand::WorkspaceArchive(request) => {
            finalize_identity(&mut request.actor, client)?
        }
        PreparedBoardCommand::BoardCreate(request) => {
            finalize_identity(&mut request.actor, client)?
        }
        PreparedBoardCommand::BoardUpdate(request) => {
            finalize_identity(&mut request.actor, client)?
        }
        PreparedBoardCommand::BoardArchive(request) => {
            finalize_identity(&mut request.actor, client)?
        }
        PreparedBoardCommand::BoardClose(request) => finalize_identity(&mut request.actor, client)?,
        PreparedBoardCommand::ProjectCreate(request) => {
            finalize_identity(&mut request.actor, client)?
        }
        PreparedBoardCommand::ProjectUpdate(request) => {
            finalize_identity(&mut request.actor, client)?
        }
        PreparedBoardCommand::ThreadPost(pending) => {
            finalize_identity(&mut pending.request.actor, client)?
        }
        PreparedBoardCommand::ThreadShow(request) => {
            if let ReadActor::Identified { reader } = &mut request.reader {
                finalize_identity(reader, client)?;
            }
        }
        PreparedBoardCommand::ThreadRefile(request) => {
            finalize_identity(&mut request.actor, client)?
        }
        PreparedBoardCommand::ThreadResolve(request) => {
            finalize_identity(&mut request.actor, client)?
        }
        PreparedBoardCommand::ThreadUnresolve(request) => {
            finalize_identity(&mut request.actor, client)?
        }
        PreparedBoardCommand::ThreadWatch(request) => {
            finalize_identity(&mut request.actor, client)?
        }
        PreparedBoardCommand::ThreadUnwatch(request) => {
            finalize_identity(&mut request.actor, client)?
        }
        PreparedBoardCommand::ProjectWatch(request)
        | PreparedBoardCommand::ProjectUnwatch(request) => {
            finalize_identity(&mut request.actor, client)?
        }
        PreparedBoardCommand::ThreadList(request) => {
            finalize_identity(&mut request.reader, client)?
        }
        PreparedBoardCommand::RepositoryThreadList {
            reader: Some(reader),
            ..
        } => finalize_identity(reader, client)?,
        PreparedBoardCommand::InboxFetch(request) => {
            finalize_identity(&mut request.reader, client)?
        }
        PreparedBoardCommand::InboxAcknowledge(request) => {
            finalize_identity(&mut request.actor, client)?
        }
        PreparedBoardCommand::InboxWorkspaces(request) => {
            finalize_identity(&mut request.reader, client)?
        }
        PreparedBoardCommand::ThreadCreate(pending) => {
            let actor = finalize_actor(&pending.actor, client)?;
            if matches!(actor, Identity::Session { .. }) && pending.request.role.is_none() {
                return Err("Thread Create requires --role for a session actor".into());
            }
            pending.request.actor = actor;
        }
        PreparedBoardCommand::ThreadJoin(pending) => {
            pending.request.actor = finalize_actor(&pending.actor, client)?;
            // --replace self names this session, not a human called "self".
            if let Some(replace) = &mut pending.request.replace {
                finalize_identity(replace, client)?;
            }
        }
        PreparedBoardCommand::ThreadSubscribe(pending) => {
            pending.request.actor = finalize_actor(&pending.actor, client)?;
        }
        PreparedBoardCommand::ThreadUnsubscribe(pending) => {
            pending.request.actor = finalize_actor(&pending.actor, client)?;
        }
        PreparedBoardCommand::ThreadSubscriptions(pending) => {
            pending.request.actor = finalize_actor(&pending.actor, client)?;
        }
        PreparedBoardCommand::ThreadSubscriptionWait(pending) => {
            pending.request.actor = finalize_actor(&pending.actor, client)?;
        }
        PreparedBoardCommand::ThreadLeave(pending) => {
            pending.request.actor = finalize_actor(&pending.actor, client)?;
            if let Some(LeaveFollowUp::HandTo { to: handover }) = &mut pending.request.after_leave {
                finalize_identity(handover, client)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn prepare(
    command: BoardCommand,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    match command {
        BoardCommand::RequestEffects(arguments) => {
            let scope = match (arguments.project_id, arguments.workspace_id) {
                (Some(project), None) => RequestEffectsScope::Project {
                    project_id: parse_uuid_v7(project, "--project-id")?,
                },
                (None, Some(workspace)) => RequestEffectsScope::Fleet {
                    workspace_id: parse_uuid_v7(workspace, "--workspace-id")?,
                },
                _ => return Err("select exactly one --project-id or --workspace-id".to_owned()),
            };
            Ok((
                PreparedBoardCommand::RequestEffects(RequestEffectsRequest {
                    scope,
                    request: parse_uuid_v7(arguments.request, "request")?,
                    after: arguments.after,
                }),
                command_context(arguments.common),
            ))
        }
        BoardCommand::View { command } => super::board_view_commands::prepare_view(command),
        BoardCommand::Workstream { command } => {
            super::board_entity_preparation::prepare_workstream(command)
        }
        BoardCommand::Task { command } => super::board_entity_preparation::prepare_task(command),
        BoardCommand::Subtask { command } => {
            super::board_entity_preparation::prepare_subtask(command)
        }
        BoardCommand::WorkTrail { command } => {
            super::board_entity_preparation::prepare_worktrail(command)
        }
        BoardCommand::Role { command } => super::board_entity_preparation::prepare_role(command),
        BoardCommand::Fact { command } => super::board_entity_preparation::prepare_fact(command),
        BoardCommand::Feed { command } => super::board_entity_preparation::prepare_feed(command),
        BoardCommand::Adr { command } => super::board_entity_preparation::prepare_adr(command),
        BoardCommand::Search(arguments) => {
            super::board_search_commands::prepare_discovery_search(arguments)
        }
        BoardCommand::Workspace { command } => prepare_workspace(command),
        BoardCommand::Repository { command } => prepare_repository_command(command),
        BoardCommand::Board { command } => prepare_board_metadata(command),
        BoardCommand::Project { command } => prepare_project(command),
        BoardCommand::Post { command } => prepare_message(command),
        BoardCommand::Thread { command } => prepare_thread(command),
        BoardCommand::Inbox { command } => prepare_inbox(command),
    }
}

pub(super) fn parse_actor_input(value: &str) -> Result<ActorInput, String> {
    if value == "self" {
        Ok(ActorInput::Self_)
    } else {
        parse_identity(value, "--actor").map(ActorInput::Explicit)
    }
}

pub(super) fn placeholder_identity() -> Result<Identity, String> {
    Ok(Identity::Human {
        human_id: HumanId::try_from("pending-actor-finalization".to_owned())
            .map_err(|_| "could not prepare actor identity".to_owned())?,
    })
}
