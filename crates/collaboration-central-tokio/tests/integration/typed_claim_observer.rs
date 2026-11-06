//! Independent legacy-oracle presentation of typed physical claim columns; no stored JSON.
use durable_steps::StoreError;
use serde_json::{Value, json};
use sqlx_core::row::Row;
type StoredRow = <sqlx_turso::Turso as sqlx_core::database::Database>::Row;

pub fn reservation_state(row: &StoredRow) -> Result<Option<Value>, StoreError> {
    let kind: Option<String> = row.try_get("state_kind").map_err(StoreError::backend)?;
    let outcome: Option<String> = row.try_get("outcome").map_err(StoreError::backend)?;
    match (kind.as_deref(), outcome.as_deref()) {
        (None, None) => Ok(None),
        (Some("active"), None) => Ok(Some(json!({"kind":"active"}))),
        (Some("settled"), Some("granted" | "refused")) => {
            Ok(Some(json!({"kind":"settled","value":{"outcome":outcome}})))
        }
        _ => Err(StoreError::backend("unexpected physical claim state")),
    }
}
pub fn receipt_result(row: &StoredRow) -> Result<Value, StoreError> {
    let kind: String = row.try_get("result_kind").map_err(StoreError::backend)?;
    let epoch: Option<String> = row.try_get("stored_epoch").map_err(StoreError::backend)?;
    let incarnation: Option<String> = row
        .try_get("stored_incarnation")
        .map_err(StoreError::backend)?;
    let change: Option<String> = row.try_get("stored_change").map_err(StoreError::backend)?;
    match (kind.as_str(), epoch, incarnation, change) {
        ("applied" | "duplicate", None, None, None) => Ok(json!({"kind":kind})),
        ("older" | "conflict", Some(epoch), Some(incarnation), Some(change)) => Ok(
            json!({"kind":kind,"value":{"stored":{"epoch":decode_version_component(epoch)?,"incarnation":decode_version_component(incarnation)?,"change":decode_version_component(change)?}}}),
        ),
        _ => Err(StoreError::backend("unexpected physical claim receipt")),
    }
}
fn decode_version_component(value: String) -> Result<u64, StoreError> {
    let number = value.parse::<u64>().map_err(StoreError::backend)?;
    if number == 0 || number.to_string() != value {
        return Err(StoreError::backend("noncanonical receipt version"));
    }
    Ok(number)
}
