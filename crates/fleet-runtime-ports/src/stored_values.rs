//! Statement transport rows are decoded by column name before domain construction.
use central_storage::{CorruptReason, ReadOutcome, SqlValue, StoreFailure};
use serde::de::DeserializeOwned;

use crate::workspace_decode_error::corrupt;
pub fn column<'a>(
    outcome: &ReadOutcome,
    row: &'a [SqlValue],
    name: &str,
) -> Result<&'a SqlValue, StoreFailure> {
    let index = outcome
        .columns
        .iter()
        .position(|column| column == name)
        .ok_or_else(|| StoreFailure::Corrupt {
            reason: CorruptReason::MissingColumn {
                column: name.to_owned(),
            },
        })?;
    row.get(index).ok_or_else(|| corrupt(name))
}
pub fn string<'a>(
    outcome: &ReadOutcome,
    row: &'a [SqlValue],
    name: &str,
) -> Result<&'a str, StoreFailure> {
    match column(outcome, row, name)? {
        SqlValue::Text(value) => Ok(value),
        _ => Err(corrupt(name)),
    }
}
pub fn json<T: DeserializeOwned>(value: &str, field: &str) -> Result<T, StoreFailure> {
    serde_json::from_str(value).map_err(|_| corrupt(field))
}
