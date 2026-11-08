//! Every custody owner row, as stored, for a conformance snapshot.
//!
//! Lossless: one entry per row, stored strings passed through unchanged, and a
//! value that fails to decode is refused (`CorruptRow`), never coerced or
//! dropped.

use super::sql_error;
use crate::delivery_storage_rows::{read_outbox_owner_rows, read_status_owed_rows};
use crate::{DeliveryError, ItemId};
use durable_steps::{Uuid, storage::TransactionScope};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode, types::Type,
};

/// One `_fleet_sync_outbox_items` row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboxOwnerRow {
    pub item: ItemId,
    /// The row's stored shape: `item`, `recovered` or `unresolvable`.
    pub record_kind: String,
    /// The stored delivery status; none on a row without one (recovered or
    /// unresolvable).
    pub custody: Option<String>,
}

/// One `_fleet_sync_status_owed` row: a status publication still owed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusOwedRow {
    pub id: Uuid,
    pub item: ItemId,
    pub version: u64,
}

const OUTBOX: &str = "_fleet_sync_outbox_items";
const STATUS_OWED: &str = "_fleet_sync_status_owed";

fn corrupt(table: &'static str, key: &Option<String>, field: &'static str) -> DeliveryError {
    DeliveryError::CorruptRow {
        table,
        key: key.clone().unwrap_or_default(),
        field,
    }
}

/// Every outbox row, by item id.
pub async fn outbox_owner_rows<S>(scope: &mut S) -> Result<Vec<OutboxOwnerRow>, DeliveryError>
where
    S: TransactionScope,
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'row> &'row str: ColumnIndex<<S::Database as Database>::Row>,
    for<'value> Option<String>: Type<S::Database> + Decode<'value, S::Database>,
{
    read_outbox_owner_rows(scope)
        .await
        .map_err(sql_error)?
        .into_iter()
        .map(|row| {
            let item = row
                .item_id
                .as_deref()
                .and_then(|id| Uuid::parse_str(id).ok())
                .and_then(|id| ItemId::new(id).ok())
                .ok_or_else(|| corrupt(OUTBOX, &row.item_id, "item_id"))?;
            let record_kind = row
                .record_kind
                .clone()
                .ok_or_else(|| corrupt(OUTBOX, &row.item_id, "record_kind"))?;
            Ok(OutboxOwnerRow {
                item,
                record_kind,
                custody: row.status_kind,
            })
        })
        .collect()
}

/// Every owed status publication, by id.
pub async fn status_owed_rows<S>(scope: &mut S) -> Result<Vec<StatusOwedRow>, DeliveryError>
where
    S: TransactionScope,
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'row> &'row str: ColumnIndex<<S::Database as Database>::Row>,
    for<'value> Option<String>: Type<S::Database> + Decode<'value, S::Database>,
{
    read_status_owed_rows(scope)
        .await
        .map_err(sql_error)?
        .into_iter()
        .map(|row| {
            let id = row
                .id
                .as_deref()
                .and_then(|id| Uuid::parse_str(id).ok())
                .ok_or_else(|| corrupt(STATUS_OWED, &row.id, "id"))?;
            let item = row
                .item_id
                .as_deref()
                .and_then(|item| Uuid::parse_str(item).ok())
                .and_then(|item| ItemId::new(item).ok())
                .ok_or_else(|| corrupt(STATUS_OWED, &row.id, "item_id"))?;
            let version = row
                .version
                .as_deref()
                .and_then(|version| version.parse::<u64>().ok())
                .ok_or_else(|| corrupt(STATUS_OWED, &row.id, "version"))?;
            Ok(StatusOwedRow { id, item, version })
        })
        .collect()
}

#[cfg(test)]
#[path = "owner_rows_test.rs"]
mod tests;
