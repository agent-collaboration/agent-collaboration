//! A separate actual recipient inbox; session execution stands in as held.
use super::composed_fleet_fixture::{A, B, C, Io, SERVICE_A, SERVICE_B, SERVICE_C, TestResult};
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use collaboration_central_core::CoreHost;
use durable_steps::{AtomicStore, CallId, StoreError, storage::TransactionScope};
use fleet_mail_delivery::{
    DeliveryConsumer, DeliveryError, DeliveryLimits, GateDecision, Inbox, LocalDelivery,
    LocalOutcome, SenderBinding, SenderBindingFailure,
};
use fleet_protocol::{
    DeliveryEnvelope, DeliveryHeader, DeliveryItem, DeliveryTypes, PeerId, PushResult, ReceiverId,
    Submitter,
};
use fleet_tokio_objects::{ObjectKey, ObjectStore, TokioObject};
use runtime_primitives::Clock;
use sqlx_core::{query::query, row::Row};
use std::{path::Path, sync::Arc};

type Store = TokioObject<CoreHost<Io>>;
type Attempt = fleet_mail_delivery::DeliverLocally<
    Store,
    sqlx_turso::Turso,
    BoardDeliveryTypes,
    HeldConsumer,
    InboxSenderBinding,
>;
type Report = fleet_mail_delivery::ReportOutcome<
    Store,
    sqlx_turso::Turso,
    BoardDeliveryTypes,
    InboxSenderBinding,
>;
type Removal = fleet_mail_delivery::RemoveDeliveryRecord<Store, sqlx_turso::Turso>;
type Acceptance = <BoardDeliveryTypes as DeliveryTypes>::Acceptance;
type Sender = <BoardDeliveryTypes as DeliveryTypes>::Sender;

// Independent scoped directory input stands in for the Router's authenticated
// Fleet lookup. It is populated before any received claim and never from Push.
struct InboxSenderBinding;
impl<S: TransactionScope<Database = sqlx_turso::Turso>> SenderBinding<S, BoardDeliveryTypes>
    for InboxSenderBinding
{
    #[expect(
        clippy::manual_async_fn,
        reason = "Explicit Send preserves borrowed custody transaction futures."
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
                "SELECT peer_id FROM fixture_sender_bindings WHERE service_id=?",
            )
            .bind(service.as_str())
            .fetch_optional(scope.executor())
            .await
            .map_err(|_| SenderBindingFailure::Unavailable)?
            .ok_or(SenderBindingFailure::Unknown)?;
            let raw: String = row
                .try_get("peer_id")
                .map_err(|_| SenderBindingFailure::Unavailable)?;
            let peer: PeerId = raw
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
    owner: PeerId,
}
impl RecipientInbox {
    pub(super) async fn open(path: &Path, io: Io, owner: PeerId) -> TestResult<Self> {
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
                    fleet_mail_delivery::DELIVERY_MIGRATOR
                        .run_direct(None, scope.executor(), false)
                        .await
                        .map_err(StoreError::backend)
                })
            })
            .await?;
        let now = io.now().unix_millis();
        store.transaction(move |scope|Box::pin(async move {
            query::<sqlx_turso::Turso>("CREATE TABLE IF NOT EXISTS fixture_sender_bindings(row_id TEXT NOT NULL PRIMARY KEY,service_id TEXT NOT NULL UNIQUE,peer_id TEXT NOT NULL UNIQUE,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL)")
                .execute(scope.executor()).await.map_err(StoreError::backend)?;
            for (service,peer) in [(SERVICE_A,A),(SERVICE_B,B),(SERVICE_C,C)] {
                query::<sqlx_turso::Turso>("INSERT INTO fixture_sender_bindings(row_id,service_id,peer_id,created_at,updated_at) VALUES(?,?,?,?,?) ON CONFLICT(service_id) DO NOTHING")
                    .bind(durable_steps::Uuid::now_v7().to_string()).bind(service).bind(peer).bind(now).bind(now)
                    .execute(scope.executor()).await.map_err(StoreError::backend)?;
            }
            Ok(())
        })).await?;
        Ok(Self {
            objects,
            store,
            io,
            owner,
        })
    }
    pub(super) async fn accept(
        &self,
        submitter: Submitter,
        item: DeliveryEnvelope<BoardDeliveryTypes>,
        limits: DeliveryLimits,
    ) -> TestResult<PushResult<Acceptance>> {
        let now = self.io.now();
        let owner = self.owner;
        Ok(self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    Inbox::<_, BoardDeliveryTypes, _>::new(
                        scope,
                        owner,
                        limits,
                        &InboxSenderBinding,
                    )
                    .accept_push::<Attempt, Report, Removal, _>(
                        submitter,
                        item,
                        "019f0000-0000-7000-8000-000000005240"
                            .to_owned()
                            .try_into()
                            .map_err(StoreError::backend)?,
                        &HeldConsumer,
                        now,
                    )
                    .await
                    .map_err(fleet_mail_delivery::delivery_rollback)
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
    pub(super) async fn contains(&self, item: fleet_protocol::ItemId) -> TestResult<bool> {
        Ok(self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    let row = query::<sqlx_turso::Turso>(
                        "SELECT count(*) AS items FROM _fleet_sync_inbox_items WHERE item_id=?",
                    )
                    .bind(item.as_uuid().to_string())
                    .fetch_one(scope.executor())
                    .await
                    .map_err(StoreError::backend)?;
                    Ok(row
                        .try_get::<i64, _>("items")
                        .map_err(StoreError::backend)?
                        == 1)
                })
            })
            .await?)
    }
    pub(super) async fn work_ids(&self) -> TestResult<Vec<String>> {
        Ok(self
            .store
            .transaction(|scope| {
                Box::pin(async move {
                    let rows = query::<sqlx_turso::Turso>(
                        "SELECT operation_id FROM _fleet_sync_operations ORDER BY operation_id",
                    )
                    .fetch_all(scope.executor())
                    .await
                    .map_err(StoreError::backend)?;
                    rows.into_iter()
                        .map(|row| row.try_get("operation_id").map_err(StoreError::backend))
                        .collect()
                })
            })
            .await?)
    }
    pub(super) async fn close(self) -> TestResult {
        self.objects.close().await?;
        Ok(())
    }
}
