//! Shared statement values for Core's organization and notice operations.
use central_storage::{BatchOutcome, StoreFailure};

pub(crate) fn applied(outcome: BatchOutcome) -> Result<(), StoreFailure> {
    match outcome {
        BatchOutcome::Applied { .. } => Ok(()),
        _ => Err(StoreFailure::Unavailable),
    }
}
