//! Parse domain command families without opening a connection or local store.
use super::board_entity_arguments::*;
use super::board_preparation::{CommandContext, PreparedBoardCommand, finalize_identity};
use super::board_value_parsing::*;
use collaboration_client::{CollaborationClient, board::*};
use serde::de::DeserializeOwned;

#[derive(Clone, Copy)]
pub(super) enum ProjectWriteKind {
    ProjectNotificationCancel,
    WorkstreamCreate,
    WorkstreamRename,
    WorkstreamClose,
    TaskCreate,
    TaskUpdate,
    TaskSetStatus,
    TaskMoveWorkstream,
    TaskLink,
    TaskUnlink,
    TaskDuplicate,
    TaskReplace,
    SubtaskAdd,
    SubtaskCheck,
    SubtaskUncheck,
    SubtaskMove,
    SubtaskRemove,
    WorkTrailAdd,
    WorkTrailRedact,
    RoleGrant,
    RoleRelease,
    RoleReplaceLead,
    RoleAcknowledgeCurrent,
    OwnerGrantRecord,
    MoveRequest,
    FactRecord,
    ProjectPinDefinition,
    ProjectPublishPlan,
    ProjectClose,
    ProjectDescribe,
}
pub(super) enum PreparedEntityCommand {
    MoveCheck(ProjectMoveCheckRequest),
    ProjectWrite(ProjectWriteKind, Box<ProjectRequest>),
    Show(ProjectReadKind, ProjectRecordShowRequest),
    List(ProjectReadKind, ProjectRecordListRequest),
    SubtaskList(SubtaskListRequest),
    Trail(WorkTrailShowRequest),
    Feed(FeedReadRequest),
    AdrCreate(Box<AdrCreateRequest>),
    AdrTransition(Box<AdrTransitionRequest>),
    AdrShow(FleetRecordShowRequest),
    AdrList(FleetRecordListRequest),
    ProcessRegister(Box<ProcessRegisterRequest>),
}
impl PreparedEntityCommand {
    pub(super) fn finalize(&mut self, client: &CollaborationClient) -> Result<(), String> {
        match self {
            Self::MoveCheck(request) => finalize_identity(&mut request.actor, client),
            Self::ProjectWrite(_, request) => finalize_identity(&mut request.actor, client),
            Self::AdrCreate(request) => finalize_identity(&mut request.actor, client),
            Self::AdrTransition(request) => finalize_identity(&mut request.actor, client),
            Self::ProcessRegister(request) => finalize_identity(&mut request.actor, client),
            Self::Feed(request) => match &mut request.reader {
                ReadActor::Identified { reader } => finalize_identity(reader, client),
                _ => Ok(()),
            },
            Self::Trail(request) => match &mut request.reader {
                ReadActor::Identified { reader } => finalize_identity(reader, client),
                _ => Ok(()),
            },
            _ => Ok(()),
        }
    }
}
pub(super) fn prepared(
    command: PreparedEntityCommand,
    common: super::board_arguments::CommonArguments,
) -> (PreparedBoardCommand, CommandContext) {
    (
        PreparedBoardCommand::Entity(Box::new(command)),
        command_context(common),
    )
}
pub(super) fn write(
    arguments: ProjectWriteArguments,
    kind: ProjectWriteKind,
    operation: BoardOperation,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    let (actor, acting_for) = parse_mutation_identity(&arguments.identity)?;
    let request = ProjectRequest {
        request_id: arguments.identity.request.request_id()?,
        protocol_version: ProtocolVersion::CURRENT,
        project_id: parse_uuid_v7(arguments.project_id, "--project-id")?,
        actor,
        acting_for,
        operation,
    };
    Ok(prepared(
        PreparedEntityCommand::ProjectWrite(kind, Box::new(request)),
        arguments.common,
    ))
}
pub(super) fn show(
    arguments: EntityShowArguments,
    kind: ProjectReadKind,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    Ok(prepared(
        PreparedEntityCommand::Show(
            kind,
            ProjectRecordShowRequest {
                project_id: parse_uuid_v7(arguments.project_id, "--project-id")?,
                record: RecordRef {
                    kind: kind.into(),
                    id: parse_uuid_v7(arguments.id, "--id")?,
                },
            },
        ),
        arguments.common,
    ))
}
pub(super) fn list(
    arguments: EntityListArguments,
    kind: ProjectReadKind,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    Ok(prepared(
        PreparedEntityCommand::List(
            kind,
            ProjectRecordListRequest {
                project_id: parse_uuid_v7(arguments.project_id, "--project-id")?,
                page: prepare_page_request(arguments.page)?,
            },
        ),
        arguments.common,
    ))
}
pub(super) fn json_value<TValue: DeserializeOwned>(
    value: &str,
    flag: &str,
) -> Result<TValue, String> {
    serde_json::from_str(value).map_err(|error| format!("{flag} must contain typed JSON: {error}"))
}
fn enum_value<TValue: DeserializeOwned>(value: String, flag: &str) -> Result<TValue, String> {
    serde_json::from_value(serde_json::Value::String(value))
        .map_err(|error| format!("{flag}: {error}"))
}
pub(super) fn optional_id<TIdentity: TryFrom<String>>(
    value: Option<String>,
    generated: TIdentity,
    flag: &str,
) -> Result<TIdentity, String> {
    value
        .map(|value| parse_uuid_v7(value, flag))
        .transpose()
        .map(|value| value.unwrap_or(generated))
}
fn revision(value: u64) -> Result<SubjectRevision, String> {
    value
        .try_into()
        .map_err(|_| "invalid --expected-revision".to_owned())
}
fn link_targets(
    values: Vec<String>,
    project: &str,
    flag: &str,
) -> Result<Vec<TaskLinkTarget>, String> {
    values
        .into_iter()
        .map(|value| link_target(value, project, flag))
        .collect()
}
fn link_target(value: String, project: &str, flag: &str) -> Result<TaskLinkTarget, String> {
    if value.trim_start().starts_with('{') {
        return json_value(&value, flag);
    }
    Ok(TaskLinkTarget {
        project: parse_uuid_v7(project.to_owned(), "--project-id")?,
        task: parse_uuid_v7(value, flag)?,
    })
}

pub(super) fn prepare_workstream(
    command: WorkstreamCommand,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    match command {
        WorkstreamCommand::Create {
            write: args,
            workstream_id,
            name,
            goal,
        } => write(
            args,
            ProjectWriteKind::WorkstreamCreate,
            BoardOperation::WorkstreamCreate {
                workstream_id: optional_id(
                    workstream_id,
                    WorkstreamId::generate(),
                    "--workstream-id",
                )?,
                name: parse_bounded_text(name, "--name")?,
                goal,
            },
        ),
        WorkstreamCommand::Rename {
            write: args,
            workstream_id,
            name,
            expected_revision,
        } => write(
            args,
            ProjectWriteKind::WorkstreamRename,
            BoardOperation::WorkstreamRename {
                workstream_id: parse_uuid_v7(workstream_id, "--workstream-id")?,
                name: parse_bounded_text(name, "--name")?,
                expected_revision: revision(expected_revision)?,
            },
        ),
        WorkstreamCommand::Close {
            write: args,
            workstream_id,
            reason,
            note,
            expected_revision,
        } => write(
            args,
            ProjectWriteKind::WorkstreamClose,
            BoardOperation::WorkstreamClose {
                workstream_id: parse_uuid_v7(workstream_id, "--workstream-id")?,
                reason: enum_value(reason, "--reason")?,
                note,
                expected_revision: revision(expected_revision)?,
            },
        ),
        WorkstreamCommand::Show(args) => show(args, ProjectReadKind::Workstream),
        WorkstreamCommand::List(args) => list(args, ProjectReadKind::Workstream),
    }
}
pub(super) fn prepare_task(
    command: TaskCommand,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    match command {
        TaskCommand::Create {
            write: args,
            task_id,
            workstream_id,
            title,
            body,
            blocked_by,
            related_to,
        } => {
            let blocked_by = link_targets(blocked_by, &args.project_id, "--blocked-by")?;
            let related_to = link_targets(related_to, &args.project_id, "--related-to")?;
            write(
                args,
                ProjectWriteKind::TaskCreate,
                BoardOperation::TaskCreate {
                    task_id: optional_id(task_id, TaskId::generate(), "--task-id")?,
                    workstream_id: parse_uuid_v7(workstream_id, "--workstream-id")?,
                    title: parse_bounded_text(title, "--title")?,
                    body,
                    blocked_by,
                    related_to,
                },
            )
        }
        TaskCommand::Update {
            write: args,
            task_id,
            title,
            body,
            expected_revision,
        } => write(
            args,
            ProjectWriteKind::TaskUpdate,
            BoardOperation::TaskUpdate {
                task_id: parse_uuid_v7(task_id, "--task-id")?,
                title: parse_bounded_text(title, "--title")?,
                body,
                expected_revision: revision(expected_revision)?,
            },
        ),
        TaskCommand::SetStatus {
            write: args,
            task_id,
            status,
            reason,
            expected_revision,
        } => write(
            args,
            ProjectWriteKind::TaskSetStatus,
            BoardOperation::TaskSetStatus {
                task_id: parse_uuid_v7(task_id, "--task-id")?,
                status: match (status.as_str(), reason) {
                    ("todo", None) => TaskStatusChange::Todo,
                    ("doing", None) => TaskStatusChange::Doing,
                    ("done", None) => TaskStatusChange::Done,
                    ("drop", Some(reason)) => TaskStatusChange::Drop {
                        reason: parse_bounded_text(reason, "--reason")?,
                    },
                    ("drop", None) => return Err("--status drop requires --reason".into()),
                    (_, Some(_)) => {
                        return Err("--reason is supported only with --status drop".into());
                    }
                    _ => return Err("--status must be todo, doing, done or drop".into()),
                },
                expected_revision: revision(expected_revision)?,
            },
        ),
        TaskCommand::MoveWorkstream {
            write: args,
            task_id,
            workstream_id,
            expected_revision,
        } => write(
            args,
            ProjectWriteKind::TaskMoveWorkstream,
            BoardOperation::TaskMoveWorkstream {
                task_id: parse_uuid_v7(task_id, "--task-id")?,
                workstream_id: parse_uuid_v7(workstream_id, "--workstream-id")?,
                expected_revision: revision(expected_revision)?,
            },
        ),
        TaskCommand::Link {
            write: args,
            from,
            to,
            kind,
        } => {
            let target = link_target(to, &args.project_id, "--to")?;
            write(
                args,
                ProjectWriteKind::TaskLink,
                BoardOperation::TaskLink {
                    link: TaskLinkInput {
                        from: parse_uuid_v7(from, "--from")?,
                        to: target,
                        kind: enum_value(kind, "--kind")?,
                    },
                },
            )
        }
        TaskCommand::Unlink {
            write: args,
            from,
            to,
            kind,
        } => write(
            args,
            ProjectWriteKind::TaskUnlink,
            BoardOperation::TaskUnlink {
                link: TaskLink {
                    from: parse_uuid_v7(from, "--from")?,
                    to: parse_uuid_v7(to, "--to")?,
                    kind: enum_value(kind, "--kind")?,
                },
            },
        ),
        TaskCommand::Duplicate {
            write: args,
            task_id,
            of,
            note,
            expected_revision,
        } => write(
            args,
            ProjectWriteKind::TaskDuplicate,
            BoardOperation::TaskAction {
                task_id: parse_uuid_v7(task_id, "--task-id")?,
                action: TaskAction::Duplicate {
                    of: parse_uuid_v7(of, "--of")?,
                    note,
                },
                expected_revision: revision(expected_revision)?,
            },
        ),
        TaskCommand::Replace {
            write: args,
            task_id,
            by,
            note,
            expected_revision,
        } => write(
            args,
            ProjectWriteKind::TaskReplace,
            BoardOperation::TaskAction {
                task_id: parse_uuid_v7(task_id, "--task-id")?,
                action: TaskAction::Replace {
                    by: parse_uuid_v7(by, "--by")?,
                    note,
                },
                expected_revision: revision(expected_revision)?,
            },
        ),
        TaskCommand::Show(args) => show(args, ProjectReadKind::Task),
        TaskCommand::List(args) => list(args, ProjectReadKind::Task),
    }
}
pub(super) fn prepare_subtask(
    command: SubtaskCommand,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    match command {
        SubtaskCommand::Add {
            write: args,
            subtask_id,
            task_id,
            text,
            place,
        } => write(
            args,
            ProjectWriteKind::SubtaskAdd,
            BoardOperation::SubtaskAdd {
                subtask_id: optional_id(subtask_id, SubtaskId::generate(), "--subtask-id")?,
                task_id: parse_uuid_v7(task_id, "--task-id")?,
                text: parse_bounded_text(text, "--text")?,
                place: parse_bounded_text(place, "--place")?,
            },
        ),
        SubtaskCommand::Check {
            write: args,
            subtask_id,
        } => write(
            args,
            ProjectWriteKind::SubtaskCheck,
            BoardOperation::SubtaskCheck {
                subtask_id: parse_uuid_v7(subtask_id, "--subtask-id")?,
                checked: true,
            },
        ),
        SubtaskCommand::Uncheck {
            write: args,
            subtask_id,
        } => write(
            args,
            ProjectWriteKind::SubtaskUncheck,
            BoardOperation::SubtaskCheck {
                subtask_id: parse_uuid_v7(subtask_id, "--subtask-id")?,
                checked: false,
            },
        ),
        SubtaskCommand::Move {
            write: args,
            subtask_id,
            place,
        } => write(
            args,
            ProjectWriteKind::SubtaskMove,
            BoardOperation::SubtaskMove {
                subtask_id: parse_uuid_v7(subtask_id, "--subtask-id")?,
                place: parse_bounded_text(place, "--place")?,
            },
        ),
        SubtaskCommand::Remove {
            write: args,
            subtask_id,
        } => write(
            args,
            ProjectWriteKind::SubtaskRemove,
            BoardOperation::SubtaskRemove {
                subtask_id: parse_uuid_v7(subtask_id, "--subtask-id")?,
            },
        ),
        SubtaskCommand::Show(args) => show(args, ProjectReadKind::Subtask),
        SubtaskCommand::List {
            args,
            include_removed,
        } => Ok(prepared(
            PreparedEntityCommand::SubtaskList(SubtaskListRequest {
                project_id: parse_uuid_v7(args.project_id, "--project-id")?,
                page: prepare_page_request(args.page)?,
                include_removed,
            }),
            args.common,
        )),
    }
}

pub(super) use super::board_entity_metadata_preparation::{
    file_write, prepare_adr, prepare_fact, prepare_feed, prepare_move, prepare_process,
    prepare_role, prepare_worktrail,
};
