//! Decode stored rows by column name and encode values for statement binds.
use central_storage::{CorruptReason, ReadOutcome, SqlValue, StoreFailure};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Serialize, de::DeserializeOwned};

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

/// Render the UTC millisecond text used by Central's stored timestamps.
pub fn format_utc_millis(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Encode a stored JSON bind and report the caller's field on failure.
pub fn encode_json(value: &impl Serialize, field: &str) -> Result<SqlValue, StoreFailure> {
    serde_json::to_string(value)
        .map(SqlValue::Text)
        .map_err(|_| corrupt(field))
}

#[cfg(test)]
#[path = "stored_values_test.rs"]
mod tests;
