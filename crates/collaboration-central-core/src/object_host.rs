//! The statement-only capability shared by modern central adapters.
use central_storage::{BatchOutcome, ReadOutcome, Statement, StatementBatch, StoreFailure};
use std::future::Future;

pub trait ObjectHost: Send + Sync + 'static {
    /// Runs every statement in one transaction; a failed guard or constraint rolls it all
    /// back. `AlarmUpdate::SetFromQuery` runs last in that transaction and sets the alarm.
    fn run_batch(
        &self,
        batch: StatementBatch,
    ) -> impl Future<Output = Result<BatchOutcome, StoreFailure>> + Send;
    fn read(
        &self,
        statement: Statement,
    ) -> impl Future<Output = Result<ReadOutcome, StoreFailure>> + Send;
}
