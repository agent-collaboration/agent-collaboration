//! Only correlation identities and deadlines are durable; bodies never enter these rows.
use crate::generated_forwards as queries;
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_sync_protocol::{ConnectionId, FrameId};
use fleet_sync_session::SessionKey;
use message_board::{ProjectId, ServiceId};
use runtime_primitives::SyncTime;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
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
    pub sender: ServiceId,
    pub project: ProjectId,
    pub holder: fleet_sync_protocol::PeerId,
    pub deadline: SyncTime,
}

fn decode<Row: DeserializeOwned>(
    row: queries::StoredForwardRow,
) -> Result<(FrameId, Row), StoreError> {
    let forward =
        FrameId::try_from(row.forward_frame.ok_or_else(corrupt)?).map_err(|_| corrupt())?;
    let value = serde_json::from_str(&row.record.ok_or_else(corrupt)?).map_err(|_| corrupt())?;
    Ok((forward, value))
}
fn corrupt() -> StoreError {
    StoreError::Corrupt {
        field: "forward_correlations".into(),
        message: "invalid stored correlation identity or phase".into(),
    }
}

macro_rules! correlation_store {
    ($read_all:ident,$read_one:ident,$write:ident,$delete:ident,$row:ty,$all:ident,$one:ident,$store:ident,$remove:ident) => {
        pub(crate) async fn $read_all<Scope: TransactionScope>(
            scope: &mut Scope,
        ) -> Result<Vec<$row>, StoreError>
        where
            <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
            for<'value> String: Type<Scope::Database>
                + Encode<'value, Scope::Database>
                + Decode<'value, Scope::Database>,
            for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
            for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
        {
            queries::$all(scope)
                .await
                .map_err(StoreError::backend)?
                .into_iter()
                .map(|raw| {
                    let (id, row) = decode::<$row>(raw)?;
                    if id != row.forward_frame {
                        return Err(corrupt());
                    }
                    Ok(row)
                })
                .collect()
        }
        pub(crate) async fn $read_one<Scope: TransactionScope>(
            scope: &mut Scope,
            id: FrameId,
        ) -> Result<Option<$row>, StoreError>
        where
            <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
            for<'value> String: Type<Scope::Database>
                + Encode<'value, Scope::Database>
                + Decode<'value, Scope::Database>,
            for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
            for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
        {
            queries::$one(scope, id.to_string())
                .await
                .map_err(StoreError::backend)?
                .map(|raw| {
                    let (key, row) = decode::<$row>(raw)?;
                    if key != id || row.forward_frame != id {
                        return Err(corrupt());
                    }
                    Ok(row)
                })
                .transpose()
        }
        pub(crate) async fn $write<Scope: TransactionScope>(
            scope: &mut Scope,
            row: &$row,
        ) -> Result<(), StoreError>
        where
            <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
            for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
        {
            queries::$store(
                scope,
                row.forward_frame.to_string(),
                serde_json::to_string(row).map_err(StoreError::backend)?,
            )
            .await
            .map_err(StoreError::backend)
        }
        pub(crate) async fn $delete<Scope: TransactionScope>(
            scope: &mut Scope,
            id: FrameId,
        ) -> Result<(), StoreError>
        where
            <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
            for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
        {
            queries::$remove(scope, id.to_string())
                .await
                .map_err(StoreError::backend)
        }
    };
}
correlation_store!(
    pending,
    pending_by_frame,
    save_pending,
    remove_pending,
    PendingForward,
    read_pending_forwards,
    read_pending_forward,
    store_pending_forward,
    delete_pending_forward
);
correlation_store!(
    delivered,
    delivered_by_frame,
    save_delivered,
    remove_delivered,
    DeliveredForward,
    read_delivered_forwards,
    read_delivered_forward,
    store_delivered_forward,
    delete_delivered_forward
);
