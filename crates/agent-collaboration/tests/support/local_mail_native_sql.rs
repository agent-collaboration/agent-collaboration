//! Fault and readback SQL borrows the fixture's native owner; SDK tables stay private.
use durable_steps::{
    AtomicStore, CallFailure, CallId, CallTarget, Host, NotSentReason, StoreError,
    storage::TransactionScope,
};
use interaction_storage::{InteractionAtomicStore, InteractionStore};
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use std::sync::Arc;

pub(super) struct SqlHost;
impl Clock for SqlHost {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(0)
    }
}
impl IdSource for SqlHost {
    fn new_id(&self) -> durable_steps::Uuid {
        uuid::Uuid::now_v7()
    }
}
impl WakeScheduler for SqlHost {
    async fn wake_at(&self, _: SyncTime) -> Result<(), WakeFailure> {
        Ok(())
    }
}
impl Host for SqlHost {
    async fn dispatch<T: CallTarget>(
        &self,
        _: T,
        _: T::Request,
        _: CallId,
    ) -> Result<T::Answer, CallFailure> {
        Err(CallFailure::NotSent {
            reason: NotSentReason::Overloaded,
        })
    }
}
pub async fn execute(
    owner: Arc<InteractionStore>,
    statement: &'static str,
) -> Result<(), StoreError> {
    InteractionAtomicStore::new(owner, Arc::new(SqlHost))
        .transaction(move |scope| {
            Box::pin(async move {
                sqlx::raw_sql(statement).execute(scope.executor()).await?;
                Ok(())
            })
        })
        .await
}
pub async fn counts(owner: Arc<InteractionStore>) -> Result<(i64, i64), StoreError> {
    InteractionAtomicStore::new(owner, Arc::new(SqlHost)).transaction(|scope| Box::pin(async move {
        Ok(sqlx::query_as::<sqlx_turso::Turso, (i64, i64)>("SELECT (SELECT COUNT(*) FROM message_send_answers),(SELECT COUNT(*) FROM router_pushes)")
            .fetch_one(scope.executor()).await?)
    })).await
}
pub async fn saved_answer(
    owner: Arc<InteractionStore>,
    item: String,
) -> Result<String, StoreError> {
    InteractionAtomicStore::new(owner, Arc::new(SqlHost))
        .transaction(move |scope| {
            Box::pin(async move {
                Ok(sqlx::query_scalar::<sqlx_turso::Turso, String>(
                    "SELECT result_json FROM final_read_fixture WHERE item_id=?",
                )
                .bind(item)
                .fetch_one(scope.executor())
                .await?)
            })
        })
        .await
}
pub async fn restore_answer(
    owner: Arc<InteractionStore>,
    item: String,
    result: String,
) -> Result<(), StoreError> {
    InteractionAtomicStore::new(owner, Arc::new(SqlHost))
        .transaction(move |scope| {
            Box::pin(async move {
                sqlx::query::<sqlx_turso::Turso>(
                    "UPDATE message_send_answers SET result=? WHERE item_id=?",
                )
                .bind(result)
                .bind(item)
                .execute(scope.executor())
                .await?;
                Ok(())
            })
        })
        .await
}

pub async fn retention(owner: Arc<InteractionStore>, item: String) -> Result<String, StoreError> {
    InteractionAtomicStore::new(owner, Arc::new(SqlHost))
        .transaction(move |scope| {
            Box::pin(async move {
                Ok(sqlx::query_scalar::<sqlx_turso::Turso, String>(
                    "SELECT retention FROM router_pushes WHERE push_id=?",
                )
                .bind(item)
                .fetch_one(scope.executor())
                .await?)
            })
        })
        .await
}

pub async fn local_is_compacted(
    owner: Arc<InteractionStore>,
    peer: fleet_sync_protocol::PeerId,
    reference: fleet_sync_protocol::DeliveryItemRef,
) -> Result<bool, StoreError> {
    // This read context uses PD4's limits and only the SDK's public observation methods.
    let limits =
        fleet_sync_delivery::DeliveryLimits::new::<collaboration_service::RouterDeliveryTypes>(
            1024 * 1024 * 1024,
            200_000,
            256 * 1024 * 1024,
            50_000,
            256 * 1024 * 1024,
            fleet_sync_delivery::minimum_settlement_allowance::<
                collaboration_service::RouterDeliveryTypes,
            >()
            .map_err(StoreError::backend)?,
        )
        .map_err(StoreError::backend)?;
    InteractionAtomicStore::new(owner, Arc::new(SqlHost))
        .transaction(move |scope| {
            Box::pin(async move {
                let mut inbox = fleet_sync_delivery::Inbox::<
                    _,
                    collaboration_service::RouterDeliveryTypes,
                >::new(scope, peer, limits);
                if !matches!(
                    inbox
                        .local_outcome(reference.item)
                        .await
                        .map_err(StoreError::backend)?,
                    Some(fleet_sync_protocol::RecipientOutcome::Delivered { .. })
                ) {
                    return Ok(false);
                }
                match inbox.resolve_delivery(&reference).await {
                    Err(fleet_sync_delivery::DeliveryError::CustodyMissing) => Ok(true),
                    Ok(_) => Ok(false),
                    Err(error) => Err(StoreError::backend(error)),
                }
            })
        })
        .await
}
