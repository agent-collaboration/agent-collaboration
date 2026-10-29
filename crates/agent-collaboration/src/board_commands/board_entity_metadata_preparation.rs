//! Trail reads and structured metadata command preparation.
use super::board_entity_arguments::{
    AdrCommand, FactCommand, FeedCommand, OperationFileArguments, RoleCommand, WorkTrailCommand,
};
use super::board_entity_preparation::{
    PreparedEntityCommand, ProjectWriteKind, json_value, list, optional_id, prepared, show, write,
};
use super::board_preparation::{CommandContext, PreparedBoardCommand};
use super::board_value_parsing::{
    parse_bounded_text, parse_uuid_v7, prepare_page_request, read_actor,
};
use collaboration_client::board::{
    BoardOperation, FeedReadRequest, FleetRecordListRequest, FleetRecordShowRequest,
    ProjectReadKind, ProjectRequest, RecordKind, RecordRef, WorkTrailRowId, WorkTrailSelection,
    WorkTrailShowRequest,
};
use serde::de::DeserializeOwned;

pub(super) fn prepare_worktrail(
    command: WorkTrailCommand,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    match command {
        WorkTrailCommand::Add {
            write: args,
            row_id,
            subject,
            text,
            tags,
            links,
        } => write(
            args,
            ProjectWriteKind::WorkTrailAdd,
            BoardOperation::WorkTrailAdd {
                row_id: optional_id(row_id, WorkTrailRowId::generate(), "--row-id")?,
                subject: subject
                    .map(|value| json_value(&value, "--subject"))
                    .transpose()?,
                text: parse_bounded_text(text, "--text")?,
                tags: tags
                    .into_iter()
                    .map(|value| parse_bounded_text(value, "--tag"))
                    .collect::<Result<_, _>>()?,
                links: links
                    .into_iter()
                    .map(|value| json_value(&value, "--link"))
                    .collect::<Result<_, _>>()?,
            },
        ),
        WorkTrailCommand::Redact {
            write: args,
            row_id,
            undo,
        } => write(
            args,
            ProjectWriteKind::WorkTrailRedact,
            BoardOperation::WorkTrailRedact {
                row_id: parse_uuid_v7(row_id, "row_id")?,
                redacted: !undo,
            },
        ),
        WorkTrailCommand::Show {
            project_id,
            reader,
            decisions,
            after,
            before,
            range_top,
            subject,
            tags,
            include_redacted,
            limit,
            common,
        } => {
            let after = after
                .map(|value| json_value(&value, "--after"))
                .transpose()?;
            let before = before
                .map(|value| json_value(&value, "--before"))
                .transpose()?;
            let range_top = range_top
                .map(|value| json_value(&value, "--range-top"))
                .transpose()?;
            let selection = if decisions {
                WorkTrailSelection::Decisions {
                    after,
                    before,
                    range_top,
                }
            } else {
                if before.is_some() || range_top.is_some() {
                    return Err("--before and --range-top require --decisions".into());
                }
                WorkTrailSelection::Forward {
                    after: after.ok_or("--after is required for the forward trail")?,
                }
            };
            Ok(prepared(
                PreparedEntityCommand::Trail(WorkTrailShowRequest {
                    reader: read_actor(reader.as_deref())?,
                    project_id: parse_uuid_v7(project_id, "--project-id")?,
                    selection,
                    limit: limit
                        .try_into()
                        .map_err(|_| "--limit must be 1 through 100")?,
                    subject: subject
                        .map(|value| json_value(&value, "--subject"))
                        .transpose()?,
                    tags: tags
                        .into_iter()
                        .map(|value| parse_bounded_text(value, "--tag"))
                        .collect::<Result<_, _>>()?,
                    include_redacted,
                }),
                common,
            ))
        }
    }
}
fn read_file<TValue: DeserializeOwned>(
    arguments: &OperationFileArguments,
) -> Result<TValue, String> {
    let bytes = std::fs::read(&arguments.request_file)
        .map_err(|error| format!("--request-file: {error}"))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("--request-file requires typed request JSON: {error}"))
}
pub(super) fn file_write(
    args: OperationFileArguments,
    kind: ProjectWriteKind,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    let request: ProjectRequest = read_file(&args)?;
    let valid = matches!(
        (kind, &request.operation),
        (
            ProjectWriteKind::RoleGrant,
            BoardOperation::RoleGrant { .. }
        ) | (
            ProjectWriteKind::RoleRelease,
            BoardOperation::RoleRelease { .. }
        ) | (
            ProjectWriteKind::RoleReplaceLead,
            BoardOperation::RoleReplaceLead { .. }
        ) | (
            ProjectWriteKind::RoleAcknowledgeCurrent,
            BoardOperation::RoleAcknowledgeCurrent { .. }
        ) | (
            ProjectWriteKind::OwnerGrantRecord,
            BoardOperation::OwnerGrantRecord { .. }
        ) | (
            ProjectWriteKind::FactRecord,
            BoardOperation::FactRecord { .. }
        ) | (
            ProjectWriteKind::ProjectPublishPlan,
            BoardOperation::ProjectPublishPlan { .. }
        ) | (
            ProjectWriteKind::ProjectClose,
            BoardOperation::ProjectClose { .. }
        ) | (
            ProjectWriteKind::ProjectDescribe,
            BoardOperation::ProjectDescribe { .. }
        )
    );
    if !valid {
        return Err("--request-file operation must match the command".into());
    }
    Ok(prepared(
        PreparedEntityCommand::ProjectWrite(kind, Box::new(request)),
        args.common,
    ))
}
pub(super) fn prepare_role(
    command: RoleCommand,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    match command {
        RoleCommand::Grant(args) => file_write(args, ProjectWriteKind::RoleGrant),
        RoleCommand::Release(args) => file_write(args, ProjectWriteKind::RoleRelease),
        RoleCommand::ReplaceLead(args) => file_write(args, ProjectWriteKind::RoleReplaceLead),
        RoleCommand::AcknowledgeCurrent(args) => {
            file_write(args, ProjectWriteKind::RoleAcknowledgeCurrent)
        }
        RoleCommand::OwnerGrantRecord(args) => file_write(args, ProjectWriteKind::OwnerGrantRecord),
        RoleCommand::Show(args) => show(args, ProjectReadKind::RoleAssignment),
        RoleCommand::List(args) => list(args, ProjectReadKind::RoleAssignment),
    }
}
pub(super) fn prepare_fact(
    command: FactCommand,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    match command {
        FactCommand::Record(args) => file_write(args, ProjectWriteKind::FactRecord),
        FactCommand::Show(args) => show(args, ProjectReadKind::Fact),
        FactCommand::List(args) => list(args, ProjectReadKind::Fact),
    }
}
pub(super) fn prepare_feed(
    command: FeedCommand,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    match command {
        FeedCommand::Read {
            project_id,
            reader,
            after,
            limit,
            include_redacted,
            common,
        } => Ok(prepared(
            PreparedEntityCommand::Feed(FeedReadRequest {
                reader: read_actor(reader.as_deref())?,
                project_id: parse_uuid_v7(project_id, "--project-id")?,
                after: json_value(&after, "--after")?,
                limit: limit
                    .try_into()
                    .map_err(|_| "--limit must be 1 through 100")?,
                include_redacted,
            }),
            common,
        )),
    }
}
pub(super) fn prepare_adr(
    command: AdrCommand,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    match command {
        AdrCommand::Create(args) => Ok(prepared(
            PreparedEntityCommand::AdrCreate(Box::new(read_file(&args)?)),
            args.common,
        )),
        AdrCommand::Transition(args) => Ok(prepared(
            PreparedEntityCommand::AdrTransition(Box::new(read_file(&args)?)),
            args.common,
        )),
        AdrCommand::Show { id, common } => Ok(prepared(
            PreparedEntityCommand::AdrShow(FleetRecordShowRequest {
                record: RecordRef {
                    kind: RecordKind::Adr,
                    id: parse_uuid_v7(id, "--id")?,
                },
            }),
            common,
        )),
        AdrCommand::List {
            workspace_id,
            page,
            common,
        } => Ok(prepared(
            PreparedEntityCommand::AdrList(FleetRecordListRequest {
                workspace_id: parse_uuid_v7(workspace_id, "--workspace-id")?,
                page: prepare_page_request(page)?,
            }),
            common,
        )),
    }
}
