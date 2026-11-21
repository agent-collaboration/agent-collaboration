//! Require an applied batch for Central operations that treat other outcomes as unavailable.
use central_storage::{BatchOutcome, StoreFailure};

pub(crate) fn applied(outcome: BatchOutcome) -> Result<(), StoreFailure> {
    match outcome {
        BatchOutcome::Applied { .. } => Ok(()),
        _ => Err(StoreFailure::Unavailable),
    }
}
