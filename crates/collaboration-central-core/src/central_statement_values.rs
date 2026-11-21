//! Shared statement values for Core's organization and notice operations.
use central_storage::{BatchOutcome, StoreFailure};
use workspace_model::Timestamp;

pub(crate) fn timestamp(value: Timestamp) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
pub(crate) fn applied(outcome: BatchOutcome) -> Result<(), StoreFailure> {
    match outcome {
        BatchOutcome::Applied { .. } => Ok(()),
        _ => Err(StoreFailure::Unavailable),
    }
}
