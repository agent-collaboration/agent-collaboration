//! Real SDK Local prevention followed by receive creates the report under test.
use super::report_host::ReportHost;
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use collaboration_central_conformance::ConformanceResult;
use durable_steps::{
    AtomicStore, CallId, StoreError, storage::TransactionScope, turso_store::TursoStore,
};
use fleet_sync_delivery::{
    DELIVERY_MIGRATOR, DeliveryConsumer, DeliveryError, DeliveryLimits, GateDecision, Inbox,
    LocalDelivery, LocalOutcome, ReportRef, delivery_rollback,
};
use fleet_sync_protocol::{
    DeliveryCancel, DeliveryHeader, DeliveryItem, DeliveryStream, FetchResult, RecipientOutcome,
};
use runtime_primitives::Clock;
use std::{path::Path, sync::Arc};

type Sender = <BoardDeliveryTypes as fleet_sync_protocol::DeliveryTypes>::Sender;
pub(super) type ReportStore = TursoStore<ReportHost>;
struct LocalCancelConsumer;
impl<S: TransactionScope> DeliveryConsumer<S> for LocalCancelConsumer {
    type Protocol = BoardConsumerProtocol;
    async fn release_gate(
        &self,
        _: &mut S,
        _: &DeliveryItem<BoardDeliveryTypes>,
    ) -> Result<GateDecision, DeliveryError> {
        Ok(GateDecision::Hold { until: None })
    }
    async fn deliver_locally(
        &self,
        _: &DeliveryItem<BoardDeliveryTypes>,
        _: CallId,
    ) -> LocalDelivery<BoardDeliveryTypes> {
        LocalDelivery {
            outcome: LocalOutcome::Uncertain,
            observation: (),
        }
    }
    fn may_cancel(&self, requester: &Sender, item: &DeliveryHeader<BoardDeliveryTypes>) -> bool {
        requester == item.sender()
    }
}
pub(super) fn limits() -> ConformanceResult<DeliveryLimits> {
    Ok(DeliveryLimits::new::<BoardDeliveryTypes>(
        1_000_000,
        100,
        1_000_000,
        100,
        1_000_000,
        fleet_sync_delivery::minimum_settlement_allowance::<BoardDeliveryTypes>()?,
    )?)
}
pub(super) async fn open_store(
    path: &Path,
    host: Arc<ReportHost>,
) -> ConformanceResult<Arc<ReportStore>> {
    let store = Arc::new(TursoStore::open_and_migrate(path, host).await?);
    store
        .transaction(|scope| {
            Box::pin(async move {
                let mut migrations = sqlx_core::migrate::Migrator::with_migrations(
                    DELIVERY_MIGRATOR.iter().cloned().collect::<Vec<_>>(),
                );
                migrations.dangerous_set_table_name("_fleet_sync_delivery_migrations");
                migrations
                    .run_direct(None, scope.executor(), false)
                    .await
                    .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await?;
    Ok(store)
}
pub(super) async fn prepare_report(
    store: &ReportStore,
    host: &ReportHost,
    item: DeliveryItem<BoardDeliveryTypes>,
    cancel: DeliveryCancel<BoardDeliveryTypes>,
    batch: FetchResult<BoardDeliveryTypes>,
) -> ConformanceResult<ReportRef> {
    let now = host.now();
    let limits = limits()?;
    let owner = item.recipient().peer;
    let original = item.id();
    let cancel_id = cancel.id();
    let report = store.transaction(move |scope| Box::pin(async move {
        let mut inbox = Inbox::<_, BoardDeliveryTypes>::new(scope, owner, limits);
        inbox.enqueue_local(item, &LocalCancelConsumer, now).await.map_err(delivery_rollback)?;
        let local = inbox.enqueue_local_cancel(cancel, &LocalCancelConsumer, now).await.map_err(delivery_rollback)?;
        if !local.effects.reports.is_empty() { return Err(StoreError::backend("Local cancellation emitted a remote report")); }
        let receiving = inbox.begin_receive(DeliveryStream::Items).await.map_err(delivery_rollback)?
            .ok_or_else(|| StoreError::backend("original receive cycle absent"))?;
        let effects = inbox.store_batch(receiving, batch, &LocalCancelConsumer, now).await.map_err(delivery_rollback)?;
        let [report] = effects.reports.as_slice() else { return Err(StoreError::backend("expected one preserved final report")); };
        if !effects.attempts.is_empty() || report.item != original || report.report_seq.value() != 2 {
            return Err(StoreError::backend("Local prevention did not survive receive unchanged"));
        }
        let retained = inbox.resolve_report(*report).await.map_err(delivery_rollback)?;
        if !matches!(retained.outcome, RecipientOutcome::Prevented { cancel } if cancel == cancel_id) {
            return Err(StoreError::backend("received report lost exact local prevention"));
        }
        Ok(*report)
    })).await?;
    Ok(report)
}
