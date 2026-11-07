//! Worker fault injection borrows the native owner's writer transaction.
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_sync_tokio_runner::{CallEnvelope, CallRouter, TokioHost};
use interaction_storage::{InteractionAtomicStore, InteractionStore};
use runtime_primitives::SystemClock;
use std::sync::Arc;

struct NoCalls;
impl CallRouter for NoCalls {
    async fn dispatch(
        &self,
        _: CallEnvelope,
    ) -> Result<serde_json::Value, durable_steps::CallFailure> {
        Err(durable_steps::CallFailure::AnswerLost)
    }
}

pub async fn execute(
    owner: Arc<InteractionStore>,
    statement: &'static str,
    integer: Option<i64>,
) -> Result<u64, StoreError> {
    let host = Arc::new(TokioHost::new(Arc::new(NoCalls), SystemClock));
    InteractionAtomicStore::new(owner, host)
        .transaction(move |scope| {
            Box::pin(async move {
                if let Some(integer) = integer {
                    Ok(sqlx::query::<sqlx_turso::Turso>(statement)
                        .bind(integer)
                        .execute(scope.executor())
                        .await?
                        .rows_affected())
                } else {
                    Ok(sqlx::raw_sql(statement)
                        .execute(scope.executor())
                        .await?
                        .rows_affected())
                }
            })
        })
        .await
}
