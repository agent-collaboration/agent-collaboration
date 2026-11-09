//! One Fleet-owned request row carries routing, holder delivery and requester correlation.
use crate::{
    fleet_record_codec::{check_private_id, corrupt, required},
    generated_project_requests as queries,
};
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_protocol::{ConnectionId, FrameId, PeerId};
use fleet_session::SessionKey;
use message_board::ProjectId;
use runtime_primitives::SyncTime;
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProjectRequest {
    pub forward_frame: FrameId,
    pub request_frame: FrameId,
    pub connection: ConnectionId,
    pub session: SessionKey,
    pub project: ProjectId,
    pub phase: ProjectRequestPhase,
    pub deadline: SyncTime,
    pub created_at: SyncTime,
    pub updated_at: SyncTime,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum ProjectRequestPhase {
    Routing,
    AwaitingHolder { holder: PeerId },
    AtHolder { holder: PeerId },
}

fn decode_request(row: queries::StoredProjectRequest) -> Result<ProjectRequest, StoreError> {
    for (value, field) in [
        (&row.id, "id"),
        (&row.request_frame_id, "request_frame_id"),
        (&row.requester_machine_id, "requester_machine_id"),
        (&row.requester_session_id, "requester_session_id"),
        (&row.requester_connection_id, "requester_connection_id"),
        (&row.project_id, "project_id"),
    ] {
        check_private_id(value.as_deref().ok_or_else(|| invalid(field))?, field)?;
    }
    let phase = match (row.phase.as_deref(), row.holder_machine_id) {
        (Some("routing"), None) => ProjectRequestPhase::Routing,
        (Some(kind @ ("awaiting_holder" | "at_holder")), Some(holder)) => {
            check_private_id(&holder, "machine_project_requests.holder_machine_id")?;
            let holder = holder
                .try_into()
                .map_err(|_| invalid("holder_machine_id"))?;
            if kind == "awaiting_holder" {
                ProjectRequestPhase::AwaitingHolder { holder }
            } else {
                ProjectRequestPhase::AtHolder { holder }
            }
        }
        _ => return Err(invalid("phase")),
    };
    Ok(ProjectRequest {
        forward_frame: required(row.id, "id")?
            .try_into()
            .map_err(|_| invalid("id"))?,
        request_frame: required(row.request_frame_id, "request_frame_id")?
            .try_into()
            .map_err(|_| invalid("request_frame_id"))?,
        connection: required(row.requester_connection_id, "requester_connection_id")?
            .try_into()
            .map_err(|_| invalid("requester_connection_id"))?,
        session: SessionKey {
            peer: required(row.requester_machine_id, "requester_machine_id")?
                .try_into()
                .map_err(|_| invalid("requester_machine_id"))?,
            session_id: required(row.requester_session_id, "requester_session_id")?
                .try_into()
                .map_err(|_| invalid("requester_session_id"))?,
        },
        project: required(row.project_id, "project_id")?
            .try_into()
            .map_err(|_| invalid("project_id"))?,
        phase,
        deadline: timestamp(row.deadline_at, "deadline_at")?,
        created_at: timestamp(row.created_at, "created_at")?,
        updated_at: timestamp(row.updated_at, "updated_at")?,
    })
}
fn timestamp(value: Option<String>, field: &str) -> Result<SyncTime, StoreError> {
    let text = required(value, field)?;
    let millis = text.parse::<i64>().map_err(|_| invalid(field))?;
    if millis.to_string() != text {
        return Err(invalid(field));
    }
    Ok(SyncTime::from_unix_millis(millis))
}
fn invalid(field: &str) -> StoreError {
    corrupt(
        &format!("machine_project_requests.{field}"),
        "invalid request identity or state",
    )
}

pub(crate) async fn requests<Scope: TransactionScope>(
    scope: &mut Scope,
) -> Result<Vec<ProjectRequest>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    queries::read_project_requests(scope)
        .await
        .map_err(StoreError::backend)?
        .into_iter()
        .map(decode_request)
        .collect()
}
pub(crate) async fn by_frame<Scope: TransactionScope>(
    scope: &mut Scope,
    id: FrameId,
) -> Result<Option<ProjectRequest>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    queries::read_project_request(scope, id.to_string())
        .await
        .map_err(StoreError::backend)?
        .map(|raw| {
            let row = decode_request(raw)?;
            if row.forward_frame != id {
                return Err(invalid("id"));
            }
            Ok(row)
        })
        .transpose()
}
pub(crate) async fn save<Scope: TransactionScope>(
    scope: &mut Scope,
    row: &ProjectRequest,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    let (phase, holder) = match row.phase {
        ProjectRequestPhase::Routing => ("routing", None),
        ProjectRequestPhase::AwaitingHolder { holder } => {
            ("awaiting_holder", Some(holder.to_string()))
        }
        ProjectRequestPhase::AtHolder { holder } => ("at_holder", Some(holder.to_string())),
    };
    queries::store_project_request(
        scope,
        row.forward_frame.to_string(),
        row.request_frame.to_string(),
        row.session.peer.to_string(),
        row.session.session_id.to_string(),
        row.connection.to_string(),
        row.project.as_str().into(),
        holder,
        phase.into(),
        row.deadline.unix_millis().to_string(),
        row.created_at.unix_millis().to_string(),
        row.updated_at.unix_millis().to_string(),
    )
    .await
    .map_err(StoreError::backend)
}
pub(crate) async fn remove<Scope: TransactionScope>(
    scope: &mut Scope,
    id: FrameId,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    queries::delete_project_request(scope, id.to_string())
        .await
        .map_err(StoreError::backend)
}

#[cfg(test)]
#[path = "project_request_store_test.rs"]
mod tests;

/// The earliest forwarded-post deadline, or the retry time while forward
/// expiry is failing on the Fleet object's forward_expiry row.
pub(crate) async fn next_forward_expiry<Scope: TransactionScope>(
    scope: &mut Scope,
) -> Result<Option<SyncTime>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    if let Ok(subject) = crate::fleet_feed_store::state_subject(scope).await {
        match crate::work_attempts::schedule(
            scope,
            subject,
            crate::work_attempts::AttemptKind::ForwardExpiry,
        )
        .await?
        {
            crate::work_attempts::SubjectSchedule::Unscheduled => {}
            crate::work_attempts::SubjectSchedule::RetryAt(at) => return Ok(Some(at)),
            crate::work_attempts::SubjectSchedule::NotDue => return Ok(None),
        }
    }
    next_deadline(scope).await
}

/// The earliest pending forwarded-post deadline, if any post is pending.
pub(crate) async fn next_deadline<Scope: TransactionScope>(
    scope: &mut Scope,
) -> Result<Option<SyncTime>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    Ok(requests(scope)
        .await?
        .into_iter()
        .map(|request| request.deadline)
        .min())
}
