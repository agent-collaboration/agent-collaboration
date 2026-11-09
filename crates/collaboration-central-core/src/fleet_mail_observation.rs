//! Owner-only conformance projection of one item's committed Fleet custody.
use board_sync_protocol::BoardDeliveryTypes;
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_mail_delivery::{Outbox, OutboxStatusState};
use fleet_protocol::{DeliveryStatus, ItemId, StatusVersion};
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, query::query, row::Row, types::Type,
};

pub const FLEET_MAIL_CUSTODY_PATH: &str = "/admin/v1/conformance/fleet-mail-custody";

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetMailCustodyRequest {
    pub item: ItemId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetMailCustodyObservation {
    pub outbox_count: u64,
    pub row: FleetMailCustodyRow,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FleetMailCustodyRow {
    Absent,
    Present {
        status_kind: FleetMailStatusKind,
        status_version: StatusVersion,
        payload_present: bool,
    },
}

/// A projection of SDK status tags, without acceptance or refusal payloads.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FleetMailStatusKind {
    Queued,
    Sent,
    AtRecipient,
    Held,
    Delivered,
    Uncertain,
    Expired,
    ExpiredAtUpgrade,
    Cancelled,
    Refused,
}
impl<A> From<&DeliveryStatus<A>> for FleetMailStatusKind {
    fn from(status: &DeliveryStatus<A>) -> Self {
        match status {
            DeliveryStatus::Queued => Self::Queued,
            DeliveryStatus::Sent => Self::Sent,
            DeliveryStatus::AtRecipient => Self::AtRecipient,
            DeliveryStatus::Held => Self::Held,
            DeliveryStatus::Delivered { .. } => Self::Delivered,
            DeliveryStatus::Uncertain => Self::Uncertain,
            DeliveryStatus::Expired => Self::Expired,
            DeliveryStatus::ExpiredAtUpgrade => Self::ExpiredAtUpgrade,
            DeliveryStatus::Cancelled => Self::Cancelled,
            DeliveryStatus::Refused { .. } => Self::Refused,
        }
    }
}

pub(crate) async fn read_custody<Scope: TransactionScope>(
    scope: &mut Scope,
    item: ItemId,
) -> Result<FleetMailCustodyObservation, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    // SDK-owned tables are inspected without cloning their schema into Central's
    // production query corpus. Text counts preserve exact integers on wasm32.
    let count: String = query::<Scope::Database>(
        "SELECT CAST(count(*) AS TEXT) AS count FROM _fleet_sync_outbox_items",
    )
    .fetch_one(scope.executor())
    .await
    .map_err(StoreError::backend)?
    .try_get("count")
    .map_err(StoreError::backend)?;
    let outbox_count = count.parse().map_err(StoreError::backend)?;
    let payload = query::<Scope::Database>("SELECT CASE WHEN payload IS NULL THEN '0' ELSE '1' END AS present FROM _fleet_sync_outbox_items WHERE item_id=?")
        .bind(item.as_uuid().to_string()).fetch_optional(scope.executor()).await.map_err(StoreError::backend)?
        .map(|row| row.try_get::<String,_>("present")).transpose().map_err(StoreError::backend)?;
    // The SDK owns status decoding, including negative and retained records.
    // Its status read performs no admission, expiry, settlement or publication.
    let status = Outbox::<_, BoardDeliveryTypes, _>::new(
        scope,
        crate::fleet_outbox_limits().map_err(StoreError::backend)?,
        &crate::fleet_sender_binding::FleetSenderBinding,
    )
    .status(item)
    .await
    .map_err(fleet_mail_delivery::delivery_rollback)?;
    let row = match (payload.as_deref(), status.state) {
        (None, OutboxStatusState::Unknown) => FleetMailCustodyRow::Absent,
        (
            Some(present @ ("0" | "1")),
            OutboxStatusState::Known {
                status, version, ..
            },
        ) => FleetMailCustodyRow::Present {
            status_kind: (&status).into(),
            status_version: version,
            payload_present: present == "1",
        },
        _ => {
            return Err(StoreError::backend(
                "inconsistent Fleet custody observation",
            ));
        }
    };
    Ok(FleetMailCustodyObservation { outbox_count, row })
}

#[cfg(test)]
#[path = "fleet_mail_observation_test.rs"]
mod tests;
