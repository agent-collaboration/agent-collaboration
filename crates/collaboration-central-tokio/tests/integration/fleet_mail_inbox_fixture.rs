//! A separate actual recipient inbox; session execution stands in as held.
use super::composed_fleet_fixture::{B, Io, TestResult};
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use collaboration_central_core::CoreHost;
use durable_steps::{AtomicStore, CallId, StoreError, storage::TransactionScope};
use fleet_sync_delivery::{
    DeliveryConsumer, DeliveryError, DeliveryLimits, GateDecision, Inbox, LocalDelivery,
    LocalOutcome,
};
use fleet_sync_protocol::{
    DeliveryEnvelope, DeliveryHeader, DeliveryItem, DeliveryTypes, PushResult, ReceiverId,
};
use fleet_sync_tokio_runner::{ObjectKey, ObjectStore, TokioObject};
use runtime_primitives::Clock;
use sqlx_core::{query::query, row::Row};
use std::{path::Path, sync::Arc};

type Store = TokioObject<CoreHost<Io>>;
type Attempt =
    fleet_sync_delivery::DeliverLocally<Store, sqlx_turso::Turso, BoardDeliveryTypes, HeldConsumer>;
type Report = fleet_sync_delivery::ReportOutcome<Store, sqlx_turso::Turso, BoardDeliveryTypes>;
type Removal = fleet_sync_delivery::RemoveDeliveryRecord<Store, sqlx_turso::Turso>;
type Acceptance = <BoardDeliveryTypes as DeliveryTypes>::Acceptance;
type Sender = <BoardDeliveryTypes as DeliveryTypes>::Sender;

struct HeldConsumer;
impl<S: TransactionScope> DeliveryConsumer<S> for HeldConsumer {
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
            outcome: LocalOutcome::Held { until: None },
            observation: (),
        }
    }
    fn may_cancel(&self, _: &Sender, _: &DeliveryHeader<BoardDeliveryTypes>) -> bool {
        false
    }
}

pub(super) struct RecipientInbox {
    objects: ObjectStore<CoreHost<Io>>,
    store: Arc<Store>,
    io: Io,
}
impl RecipientInbox {
    pub(super) async fn open(path: &Path, io: Io) -> TestResult<Self> {
        let objects = ObjectStore::open(path).await?;
        let store = objects
            .object(
                ObjectKey::new("recipient-inbox")?,
                Default::default(),
                Arc::new(CoreHost::for_store(
                    io.clone(),
                    ReceiverId::new(durable_steps::Uuid::now_v7())?,
                )),
            )
            .await?;
        store
            .transaction(|scope| {
                Box::pin(async move {
                    fleet_sync_delivery::DELIVERY_MIGRATOR
                        .run_direct(None, scope.executor(), false)
                        .await
                        .map_err(StoreError::backend)
                })
            })
            .await?;
        Ok(Self { objects, store, io })
    }
    pub(super) async fn accept(
        &self,
        item: DeliveryEnvelope<BoardDeliveryTypes>,
        limits: DeliveryLimits,
    ) -> TestResult<PushResult<Acceptance>> {
        let now = self.io.now();
        Ok(self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    Inbox::<_, BoardDeliveryTypes>::new(
                        scope,
                        B.to_owned().try_into().map_err(StoreError::backend)?,
                        limits,
                    )
                    .accept_push::<Attempt, Report, Removal, _>(
                        item,
                        "019f0000-0000-7000-8000-000000005240"
                            .to_owned()
                            .try_into()
                            .map_err(StoreError::backend)?,
                        &HeldConsumer,
                        now,
                    )
                    .await
                    .map_err(fleet_sync_delivery::delivery_rollback)
                })
            })
            .await?)
    }
    pub(super) async fn count(&self) -> TestResult<i64> {
        Ok(self
            .store
            .transaction(|scope| {
                Box::pin(async move {
                    let row = query::<sqlx_turso::Turso>(
                        "SELECT count(*) AS items FROM _fleet_sync_inbox_items",
                    )
                    .fetch_one(scope.executor())
                    .await
                    .map_err(StoreError::backend)?;
                    row.try_get("items").map_err(StoreError::backend)
                })
            })
            .await?)
    }
    pub(super) async fn close(self) -> TestResult {
        self.objects.close().await?;
        Ok(())
    }
}
