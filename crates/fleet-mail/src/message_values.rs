//! Storage values used by Mail row statements.
use central_storage::{SqlValue, StoreFailure};
use fleet_runtime_ports::{guarded_statements::text, workspace_decode_error::corrupt};
use serde::Serialize;
use workspace_model::Timestamp;

pub(crate) fn encode(value: &impl Serialize) -> Result<SqlValue, StoreFailure> {
    serde_json::to_string(value)
        .map(text)
        .map_err(|_| corrupt("mail JSON"))
}
pub(crate) fn timestamp(value: Timestamp) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
