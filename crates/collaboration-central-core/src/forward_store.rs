//! Typed forwarding correlations keep the origin and holder sides independently durable.
use crate::{
    fleet_record_codec::{check_private_id, corrupt, required},
    generated_forwards as queries,
};
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_sync_protocol::{ConnectionId, FrameId, PeerId};
use fleet_sync_session::SessionKey;
use message_board::{ProjectId, ServiceId};
use runtime_primitives::SyncTime;
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PendingForward {
    pub forward_frame: FrameId,
    pub request_frame: FrameId,
    pub connection: ConnectionId,
    pub session: SessionKey,
    pub project: ProjectId,
    pub phase: ForwardPhase,
    pub deadline: SyncTime,
    pub created_at: SyncTime,
    pub updated_at: SyncTime,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum ForwardPhase {
    Routing,
    AwaitingHolder { installation: ServiceId },
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DeliveredForward {
    pub forward_frame: FrameId,
    pub request_frame: FrameId,
    pub connection: ConnectionId,
    pub origin_session: SessionKey,
    pub sender: ServiceId,
    pub project: ProjectId,
    pub holder: PeerId,
    pub deadline: SyncTime,
    pub created_at: SyncTime,
    pub updated_at: SyncTime,
}

struct ForwardHeader {
    forward_frame: FrameId,
    request_frame: FrameId,
    connection: ConnectionId,
    origin_session: SessionKey,
    project: ProjectId,
    deadline: SyncTime,
    created_at: SyncTime,
    updated_at: SyncTime,
}
impl ForwardHeader {
    fn decode(row: &queries::StoredForwardRow) -> Result<Self, StoreError> {
        check_private_id(
            row.id.as_deref().ok_or_else(|| invalid("id"))?,
            "machine_forward_requests.id",
        )?;
        for (value, field) in [
            (&row.forward_frame_id, "forward_frame_id"),
            (&row.request_frame_id, "request_frame_id"),
            (&row.origin_machine_id, "origin_machine_id"),
            (&row.origin_session_id, "origin_session_id"),
            (&row.connection_id, "connection_id"),
            (&row.project_id, "project_id"),
        ] {
            check_private_id(value.as_deref().ok_or_else(|| invalid(field))?, field)?;
        }
        Ok(Self {
            forward_frame: required(row.forward_frame_id.clone(), "forward_frame_id")?
                .try_into()
                .map_err(|_| invalid("forward_frame_id"))?,
            request_frame: required(row.request_frame_id.clone(), "request_frame_id")?
                .try_into()
                .map_err(|_| invalid("request_frame_id"))?,
            connection: required(row.connection_id.clone(), "connection_id")?
                .try_into()
                .map_err(|_| invalid("connection_id"))?,
            origin_session: SessionKey {
                peer: required(row.origin_machine_id.clone(), "origin_machine_id")?
                    .try_into()
                    .map_err(|_| invalid("origin_machine_id"))?,
                session_id: required(row.origin_session_id.clone(), "origin_session_id")?
                    .try_into()
                    .map_err(|_| invalid("origin_session_id"))?,
            },
            project: required(row.project_id.clone(), "project_id")?
                .try_into()
                .map_err(|_| invalid("project_id"))?,
            deadline: timestamp(row.deadline_at.clone(), "deadline_at")?,
            created_at: timestamp(row.created_at.clone(), "created_at")?,
            updated_at: timestamp(row.updated_at.clone(), "updated_at")?,
        })
    }
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
        &format!("machine_forward_requests.{field}"),
        "invalid correlation identity or state",
    )
}
fn decode_pending(row: queries::StoredForwardRow) -> Result<PendingForward, StoreError> {
    let header = ForwardHeader::decode(&row)?;
    if row.side.as_deref() != Some("origin") {
        return Err(invalid("side"));
    }
    let phase = match (
        row.phase.as_deref(),
        row.expected_holder_machine_id,
        row.holder_service_id,
    ) {
        (Some("routing"), None, None) => ForwardPhase::Routing,
        (Some("awaiting_holder"), Some(peer), Some(service)) => {
            check_private_id(&peer, "machine_forward_requests.expected_holder_machine_id")?;
            ForwardPhase::AwaitingHolder {
                installation: service
                    .try_into()
                    .map_err(|_| invalid("holder_service_id"))?,
            }
        }
        _ => return Err(invalid("phase")),
    };
    Ok(PendingForward {
        forward_frame: header.forward_frame,
        request_frame: header.request_frame,
        connection: header.connection,
        session: header.origin_session,
        project: header.project,
        phase,
        deadline: header.deadline,
        created_at: header.created_at,
        updated_at: header.updated_at,
    })
}
fn decode_delivered(row: queries::StoredForwardRow) -> Result<DeliveredForward, StoreError> {
    let header = ForwardHeader::decode(&row)?;
    if row.side.as_deref() != Some("holder") || row.phase.as_deref() != Some("delivered") {
        return Err(invalid("phase"));
    }
    let holder = required(row.expected_holder_machine_id, "expected_holder_machine_id")?;
    check_private_id(
        &holder,
        "machine_forward_requests.expected_holder_machine_id",
    )?;
    Ok(DeliveredForward {
        forward_frame: header.forward_frame,
        request_frame: header.request_frame,
        connection: header.connection,
        origin_session: header.origin_session,
        sender: required(row.origin_service_id, "origin_service_id")?
            .try_into()
            .map_err(|_| invalid("origin_service_id"))?,
        project: header.project,
        holder: holder
            .try_into()
            .map_err(|_| invalid("expected_holder_machine_id"))?,
        deadline: header.deadline,
        created_at: header.created_at,
        updated_at: header.updated_at,
    })
}

macro_rules! read_correlations {
    ($all:ident, $one:ident, $side:literal, $row:ty, $decode:ident) => {
        pub(crate) async fn $all<Scope: TransactionScope>(
            scope: &mut Scope,
        ) -> Result<Vec<$row>, StoreError>
        where
            <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
            for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
            for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
            for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
        {
            queries::read_forward_rows(scope, $side.into())
                .await
                .map_err(StoreError::backend)?
                .into_iter()
                .map($decode)
                .collect()
        }
        pub(crate) async fn $one<Scope: TransactionScope>(
            scope: &mut Scope,
            id: FrameId,
        ) -> Result<Option<$row>, StoreError>
        where
            <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
            for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
            for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
            for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
        {
            queries::read_forward_row(scope, $side.into(), id.to_string())
                .await
                .map_err(StoreError::backend)?
                .map(|raw| {
                    let row = $decode(raw)?;
                    if row.forward_frame != id {
                        return Err(invalid("forward_frame_id"));
                    }
                    Ok(row)
                })
                .transpose()
        }
    };
}
read_correlations!(
    pending,
    pending_by_frame,
    "origin",
    PendingForward,
    decode_pending
);
read_correlations!(
    delivered,
    delivered_by_frame,
    "holder",
    DeliveredForward,
    decode_delivered
);

pub(crate) async fn save_pending<Scope: TransactionScope>(
    scope: &mut Scope,
    row: &PendingForward,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let (phase, holder) = match &row.phase {
        ForwardPhase::Routing => ("routing", None),
        ForwardPhase::AwaitingHolder { installation } => {
            let mut machines = crate::generated_enrollment::read_enrollments(
                scope,
                Some(installation.as_str().into()),
                None,
                None,
                None,
            )
            .await
            .map_err(StoreError::backend)?;
            if machines.len() != 1 {
                return Err(invalid("expected_holder_machine_id"));
            }
            let machine = machines
                .pop()
                .ok_or_else(|| invalid("expected_holder_machine_id"))?;
            let holder =
                crate::machine_enrollment_record::identity_from_stored_enrollment(machine)?.peer;
            ("awaiting_holder", Some(holder.to_string()))
        }
    };
    queries::store_forward_row(
        scope,
        durable_steps::Uuid::now_v7().to_string(),
        row.forward_frame.to_string(),
        "origin".into(),
        row.request_frame.to_string(),
        row.session.peer.to_string(),
        row.session.session_id.to_string(),
        row.connection.to_string(),
        holder,
        row.project.as_str().into(),
        phase.into(),
        row.deadline.unix_millis().to_string(),
        row.created_at.unix_millis().to_string(),
        row.updated_at.unix_millis().to_string(),
    )
    .await
    .map_err(StoreError::backend)
}

pub(crate) async fn save_delivered<Scope: TransactionScope>(
    scope: &mut Scope,
    row: &DeliveredForward,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    queries::store_forward_row(
        scope,
        durable_steps::Uuid::now_v7().to_string(),
        row.forward_frame.to_string(),
        "holder".into(),
        row.request_frame.to_string(),
        row.origin_session.peer.to_string(),
        row.origin_session.session_id.to_string(),
        row.connection.to_string(),
        Some(row.holder.to_string()),
        row.project.as_str().into(),
        "delivered".into(),
        row.deadline.unix_millis().to_string(),
        row.created_at.unix_millis().to_string(),
        row.updated_at.unix_millis().to_string(),
    )
    .await
    .map_err(StoreError::backend)
}
macro_rules! delete_correlation {
    ($name:ident,$side:literal) => {
        pub(crate) async fn $name<Scope: TransactionScope>(
            scope: &mut Scope,
            id: FrameId,
        ) -> Result<(), StoreError>
        where
            <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
            for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
        {
            queries::delete_forward_row(scope, $side.into(), id.to_string())
                .await
                .map_err(StoreError::backend)
        }
    };
}
delete_correlation!(remove_pending, "origin");
delete_correlation!(remove_delivered, "holder");

#[cfg(test)]
#[path = "forward_store_test.rs"]
mod tests;
