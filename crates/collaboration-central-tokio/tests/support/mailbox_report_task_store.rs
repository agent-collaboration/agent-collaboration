//! Real SDK Local prevention followed by receive creates the report under test.
use super::report_host::ReportHost;
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use collaboration_central_conformance::ConformanceResult;
use durable_steps::{
    AtomicStore, CallId, StoreError, storage::TransactionScope, turso_store::TursoStore,
};
use fleet_sync_delivery::{
    DELIVERY_MIGRATOR, DeliveryConsumer, DeliveryError, DeliveryLimits, GateDecision, Inbox,
    LocalDelivery, LocalOutcome, ReportRef, SenderBinding, SenderBindingFailure, delivery_rollback,
};
use fleet_sync_protocol::{
    DeliveryCancel, DeliveryHeader, DeliveryItem, DeliveryStream, FetchResult, RecipientOutcome,
    Submitter,
};
use runtime_primitives::Clock;
use sqlx_core::{query::query, row::Row};
use std::{path::Path, sync::Arc};

type Sender = <BoardDeliveryTypes as fleet_sync_protocol::DeliveryTypes>::Sender;
pub(super) type ReportStore = TursoStore<ReportHost>;
pub(super) struct ReportSenderBinding;
impl<S: TransactionScope<Database = sqlx_turso::Turso>> SenderBinding<S, BoardDeliveryTypes>
    for ReportSenderBinding
{
    #[expect(
        clippy::manual_async_fn,
        reason = "Keep the borrowed custody future explicitly Send."
    )]
    fn resolve_submitter<'a>(
        &'a self,
        scope: &'a mut S,
        sender: &'a Sender,
    ) -> impl std::future::Future<Output = Result<Submitter, SenderBindingFailure>> + Send + 'a
    {
        async move {
            let service = match sender {
                Sender::Project { project } => {
                    return project
                        .as_str()
                        .to_owned()
                        .try_into()
                        .map(Submitter::Project)
                        .map_err(|_| SenderBindingFailure::Conflict);
                }
                Sender::Session(session) => &session.endpoint.service_id,
                Sender::HumanUser { installation } => installation,
            };
            let row = query::<sqlx_turso::Turso>(
                "SELECT peer_id FROM fixture_report_senders WHERE service_id=?",
            )
            .bind(service.as_str())
            .fetch_optional(scope.executor())
            .await
            .map_err(|_| SenderBindingFailure::Unavailable)?
            .ok_or(SenderBindingFailure::Unknown)?;
            let raw: String = row
                .try_get("peer_id")
                .map_err(|_| SenderBindingFailure::Unavailable)?;
            let peer: fleet_sync_protocol::PeerId = raw
                .clone()
                .try_into()
                .map_err(|_| SenderBindingFailure::Unavailable)?;
            if peer.to_string() != raw {
                return Err(SenderBindingFailure::Unavailable);
            }
            Ok(Submitter::Machine(peer))
        }
    }
}
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
    let store = Arc::new(TursoStore::open_and_migrate(path, host.clone()).await?);
    let now = host.now().unix_millis();
    store
        .transaction(move |scope| {
            Box::pin(async move {
                let mut migrations = sqlx_core::migrate::Migrator::with_migrations(
                    DELIVERY_MIGRATOR.iter().cloned().collect::<Vec<_>>(),
                );
                migrations.dangerous_set_table_name("_fleet_sync_delivery_migrations");
                migrations
                    .run_direct(None, scope.executor(), false)
                    .await
                    .map_err(StoreError::backend)?;
                // Independent enrolled fixture identities, seeded before any
                // received sender claim; local session execution is separate.
                query::<sqlx_turso::Turso>("CREATE TABLE IF NOT EXISTS fixture_report_senders(row_id TEXT NOT NULL PRIMARY KEY,service_id TEXT NOT NULL UNIQUE,peer_id TEXT NOT NULL UNIQUE,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL)")
                    .execute(scope.executor()).await.map_err(StoreError::backend)?;
                for (service,peer) in [
                    (super::super::mailbox_rule_fixture::ORIGIN,"019f0000-0000-7000-8000-000000001081"),
                    (super::super::mailbox_rule_fixture::DESTINATION,"019f0000-0000-7000-8000-000000001082"),
                ] {
                    query::<sqlx_turso::Turso>("INSERT INTO fixture_report_senders(row_id,service_id,peer_id,created_at,updated_at) VALUES(?,?,?,?,?) ON CONFLICT(service_id) DO NOTHING")
                        .bind(durable_steps::Uuid::now_v7().to_string()).bind(service).bind(peer).bind(now).bind(now).execute(scope.executor()).await.map_err(StoreError::backend)?;
                }
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
        let mut inbox = Inbox::<_, BoardDeliveryTypes,_>::new(scope, owner, limits,&ReportSenderBinding);
        inbox.enqueue_local(item, &LocalCancelConsumer, now).await.map_err(delivery_rollback)?;
        let local = inbox.enqueue_local_cancel(cancel, &LocalCancelConsumer, now).await.map_err(delivery_rollback)?;
        if !local.effects.reports.is_empty() { return Err(StoreError::backend("Local cancellation emitted a remote report")); }
        let receiving = inbox.begin_receive(DeliveryStream::Items, now).await.map_err(delivery_rollback)?
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
