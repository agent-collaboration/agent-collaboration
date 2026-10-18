//! Typed SDK dispatch keeps domain command selection closed.
use super::board_entity_preparation::{PreparedEntityCommand, ProjectWriteKind};
use collaboration_client::{BoardClientError, ClientError, CollaborationClient, board::*};
use serde::Serialize;
use serde_json::Value;
fn serialized(value: impl Serialize) -> Result<Value, BoardClientError> {
    serde_json::to_value(value)
        .map_err(|_| ClientError::Protocol("board result could not be serialized").into())
}
pub(super) async fn dispatch(
    client: &CollaborationClient,
    command: PreparedEntityCommand,
) -> Result<Value, BoardClientError> {
    match command {
        PreparedEntityCommand::MoveCheck(request) => serialized(client.move_check(request).await?),
        PreparedEntityCommand::ProjectWrite(kind, request) => serialized(match kind {
            ProjectWriteKind::WorkstreamCreate => client.workstream_create(*request).await?,
            ProjectWriteKind::WorkstreamRename => client.workstream_rename(*request).await?,
            ProjectWriteKind::WorkstreamClose => client.workstream_close(*request).await?,
            ProjectWriteKind::TaskCreate => client.task_create(*request).await?,
            ProjectWriteKind::TaskUpdate => client.task_update(*request).await?,
            ProjectWriteKind::TaskSetStatus => client.task_set_status(*request).await?,
            ProjectWriteKind::TaskMoveWorkstream => client.task_move_workstream(*request).await?,
            ProjectWriteKind::TaskLink => client.task_link(*request).await?,
            ProjectWriteKind::TaskUnlink => client.task_unlink(*request).await?,
            ProjectWriteKind::TaskDuplicate => client.task_duplicate(*request).await?,
            ProjectWriteKind::TaskReplace => client.task_replace(*request).await?,
            ProjectWriteKind::SubtaskAdd => client.subtask_add(*request).await?,
            ProjectWriteKind::SubtaskCheck => client.subtask_check(*request).await?,
            ProjectWriteKind::SubtaskUncheck => client.subtask_uncheck(*request).await?,
            ProjectWriteKind::SubtaskMove => client.subtask_move(*request).await?,
            ProjectWriteKind::SubtaskRemove => client.subtask_remove(*request).await?,
            ProjectWriteKind::WorkTrailAdd => client.work_trail_add(*request).await?,
            ProjectWriteKind::WorkTrailRedact => client.work_trail_redact(*request).await?,
            ProjectWriteKind::RoleGrant => client.role_grant(*request).await?,
            ProjectWriteKind::RoleRelease => client.role_release(*request).await?,
            ProjectWriteKind::RoleReplaceLead => client.role_replace_lead(*request).await?,
            ProjectWriteKind::RoleAcknowledgeCurrent => {
                client.role_acknowledge_current(*request).await?
            }
            ProjectWriteKind::OwnerGrantRecord => client.owner_grant_record(*request).await?,
            ProjectWriteKind::MoveRequest => client.move_request(*request).await?,
            ProjectWriteKind::FactRecord => client.fact_record(*request).await?,
            ProjectWriteKind::ProjectPinDefinition => {
                client.project_pin_definition(*request).await?
            }
            ProjectWriteKind::ProjectPublishPlan => client.project_publish_plan(*request).await?,
            ProjectWriteKind::ProjectClose => client.project_close(*request).await?,
        }),
        PreparedEntityCommand::Show(kind, request) => serialized(match kind {
            ProjectReadKind::Workstream => client.workstream_show(request).await?,
            ProjectReadKind::Task => client.task_show(request).await?,
            ProjectReadKind::Subtask => client.subtask_show(request).await?,
            ProjectReadKind::RoleAssignment => client.role_show(request).await?,
            ProjectReadKind::Move => client.move_show(request).await?,
            ProjectReadKind::Fact => client.fact_show(request).await?,
            ProjectReadKind::NotifyIntent => client.intent_show(request).await?,
            ProjectReadKind::WorkTrailRow => {
                return Err(ClientError::InvalidRequest("use work-trail show").into());
            }
        }),
        PreparedEntityCommand::List(kind, request) => serialized(match kind {
            ProjectReadKind::Workstream => client.workstream_list(request).await?,
            ProjectReadKind::Task => client.task_list(request).await?,
            ProjectReadKind::Subtask => {
                return Err(ClientError::InvalidRequest("use subtask list").into());
            }
            ProjectReadKind::RoleAssignment => client.role_list(request).await?,
            ProjectReadKind::Move => client.move_list(request).await?,
            ProjectReadKind::Fact => client.fact_list(request).await?,
            ProjectReadKind::NotifyIntent => client.intent_list(request).await?,
            ProjectReadKind::WorkTrailRow => {
                return Err(ClientError::InvalidRequest("use work-trail show").into());
            }
        }),
        PreparedEntityCommand::SubtaskList(request) => {
            serialized(client.subtask_list(request).await?)
        }
        PreparedEntityCommand::Trail(request) => serialized(client.work_trail_show(request).await?),
        PreparedEntityCommand::Feed(request) => serialized(client.feed_read(request).await?),
        PreparedEntityCommand::AdrCreate(request) => serialized(client.adr_create(*request).await?),
        PreparedEntityCommand::AdrTransition(request) => {
            serialized(client.adr_transition(*request).await?)
        }
        PreparedEntityCommand::AdrShow(request) => serialized(client.adr_show(request).await?),
        PreparedEntityCommand::AdrList(request) => serialized(client.adr_list(request).await?),
        PreparedEntityCommand::ProcessRegister(request) => {
            serialized(client.process_register(*request).await?)
        }
    }
}
