//! Real recipient custody, with an independent roster and held local execution input.
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use collaboration_central_conformance::ConformanceResult;
use durable_steps::{
    AtomicStore, CallFailure, CallId, CallTarget, Engine, Host, OperationState, StoreError,
    storage::TransactionScope, turso_store::TursoStore,
};
use fleet_sync_delivery::{
    DeliverCall, DeliverLocally, DeliveryAttempt, DeliveryConsumer, DeliveryError, DeliveryLimits,
    GateDecision, Inbox, LocalDelivery, LocalDeliveryConfig, LocalOutcome, RemoveDeliveryRecord,
    ReportOutcome, SenderBinding, SenderBindingFailure, delivery_request_key, delivery_rollback,
};
use fleet_sync_protocol::{
    DeliveryEnvelope, DeliveryHeader, DeliveryItem, DeliveryItemRef, PeerId, PushResult,
    RecipientReport, Submitter,
};
use message_board::ServiceId;
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use sqlx_core::{query::query, row::Row};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
};

type Store = TursoStore<RecipientHost>;
type Acceptance = <BoardDeliveryTypes as fleet_sync_protocol::DeliveryTypes>::Acceptance;
type Sender = <BoardDeliveryTypes as fleet_sync_protocol::DeliveryTypes>::Sender;
type Attempt =
    DeliverLocally<Store, sqlx_turso::Turso, BoardDeliveryTypes, HeldConsumer, RosterBinding>;
type Report = ReportOutcome<Store, sqlx_turso::Turso, BoardDeliveryTypes, RosterBinding>;
type Removal = RemoveDeliveryRecord<Store, sqlx_turso::Turso>;
const NOW: i64 = 1_792_281_600_000;

struct RecipientHost(AtomicI64);
impl Clock for RecipientHost {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
}
impl IdSource for RecipientHost {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::now_v7()
    }
}
impl WakeScheduler for RecipientHost {
    async fn wake_at(&self, _: SyncTime) -> Result<(), WakeFailure> {
        Ok(())
    }
}
impl Host for RecipientHost {
    async fn dispatch<C: CallTarget>(
        &self,
        _: C,
        _: C::Request,
        _: CallId,
    ) -> Result<C::Answer, CallFailure> {
        if C::KIND != DeliverCall::<BoardDeliveryTypes>::KIND {
            return Err(CallFailure::AnswerLost);
        }
        // Local session execution is a disclosed component input. Custody,
        // receipt construction and report persistence remain the real SDK.
        let held = DeliverCall::<BoardDeliveryTypes>::validated_answer(LocalDelivery {
            outcome: LocalOutcome::Held { until: None },
            observation: (),
        });
        serde_json::from_value(serde_json::to_value(held).map_err(|_| CallFailure::AnswerLost)?)
            .map_err(|_| CallFailure::AnswerLost)
    }
}

struct RosterBinding;
impl<S: TransactionScope<Database = sqlx_turso::Turso>> SenderBinding<S, BoardDeliveryTypes>
    for RosterBinding
{
    #[expect(
        clippy::manual_async_fn,
        reason = "The borrowed custody transaction future must remain Send."
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
                "SELECT peer_id FROM fixture_push_senders WHERE service_id=?",
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
    fn may_cancel(&self, requester: &Sender, item: &DeliveryHeader<BoardDeliveryTypes>) -> bool {
        requester == item.sender()
    }
}

pub(super) struct PushInbox {
    store: Arc<Store>,
    host: Arc<RecipientHost>,
    owner: PeerId,
    relay: PeerId,
    limits: DeliveryLimits,
}
impl PushInbox {
    pub(super) async fn open(
        path: &Path,
        owner: PeerId,
        roster: Vec<(ServiceId, PeerId)>,
    ) -> ConformanceResult<Self> {
        let host = Arc::new(RecipientHost(AtomicI64::new(NOW)));
        let store = Arc::new(TursoStore::open_and_migrate(path, host.clone()).await?);
        store.transaction(move |scope| Box::pin(async move {
            let mut migrations = sqlx_core::migrate::Migrator::with_migrations(
                fleet_sync_delivery::DELIVERY_MIGRATOR.iter().cloned().collect::<Vec<_>>());
            migrations.dangerous_set_table_name("_fleet_sync_delivery_migrations");
            migrations.run_direct(None, scope.executor(), false).await.map_err(StoreError::backend)?;
            query::<sqlx_turso::Turso>("CREATE TABLE IF NOT EXISTS fixture_push_senders(row_id TEXT NOT NULL PRIMARY KEY,service_id TEXT NOT NULL UNIQUE,peer_id TEXT NOT NULL UNIQUE,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL)")
                .execute(scope.executor()).await.map_err(StoreError::backend)?;
            for (service, peer) in roster {
                query::<sqlx_turso::Turso>("INSERT INTO fixture_push_senders(row_id,service_id,peer_id,created_at,updated_at) VALUES(?,?,?,?,?) ON CONFLICT(service_id) DO NOTHING")
                    .bind(durable_steps::Uuid::now_v7().to_string()).bind(service.as_str()).bind(peer.to_string())
                    .bind(NOW).bind(NOW).execute(scope.executor()).await.map_err(StoreError::backend)?;
            }
            Ok(())
        })).await?;
        let limits = DeliveryLimits::new::<BoardDeliveryTypes>(
            1_000_000,
            100,
            1_000_000,
            100,
            1_000_000,
            fleet_sync_delivery::minimum_settlement_allowance::<BoardDeliveryTypes>()?,
        )?;
        Ok(Self {
            store,
            host,
            owner,
            relay: "019f0000-0000-7000-8000-000000005240"
                .to_owned()
                .try_into()?,
            limits,
        })
    }
    pub(super) async fn accept(
        &self,
        submitter: Submitter,
        item: DeliveryEnvelope<BoardDeliveryTypes>,
    ) -> ConformanceResult<PushResult<Acceptance>> {
        self.accept_with_limits(submitter, item, self.limits).await
    }
    pub(super) async fn accept_with_limits(
        &self,
        submitter: Submitter,
        item: DeliveryEnvelope<BoardDeliveryTypes>,
        limits: DeliveryLimits,
    ) -> ConformanceResult<PushResult<Acceptance>> {
        let (owner, relay, now) = (self.owner, self.relay, self.host.now());
        Ok(self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    Inbox::<_, BoardDeliveryTypes, _>::new(scope, owner, limits, &RosterBinding)
                        .accept_push::<Attempt, Report, Removal, _>(
                            submitter,
                            item,
                            relay,
                            &HeldConsumer,
                            now,
                        )
                        .await
                        .map_err(delivery_rollback)
                })
            })
            .await?)
    }
    pub(super) async fn hold_and_report(
        &self,
        item: fleet_sync_protocol::ItemId,
    ) -> ConformanceResult<RecipientReport<BoardDeliveryTypes>> {
        let mut engine = Engine::new(self.store.clone(), self.host.clone());
        engine.register(Attempt::new(
            LocalDeliveryConfig {
                owner: self.owner,
                limits: self.limits,
            },
            Arc::new(HeldConsumer),
            self.host.clone(),
            Arc::new(RosterBinding),
        ))?;
        let input = DeliveryAttempt {
            item,
            attempt: 0,
            not_before: self.host.now(),
        };
        let status = engine
            .start_and_settle::<Attempt>(&delivery_request_key(&(item, 0_u64))?, input)
            .await?;
        if !matches!(status.state, OperationState::Completed { .. }) {
            return Err(format!("local Held input did not complete: {:?}", status.state).into());
        }
        self.report(item).await
    }
    pub(super) async fn report(
        &self,
        item: fleet_sync_protocol::ItemId,
    ) -> ConformanceResult<RecipientReport<BoardDeliveryTypes>> {
        let (owner, limits) = (self.owner, self.limits);
        Ok(self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    let mut inbox = Inbox::<_, BoardDeliveryTypes, _>::new(
                        scope,
                        owner,
                        limits,
                        &RosterBinding,
                    );
                    let reference = inbox
                        .pending_report(item)
                        .await
                        .map_err(delivery_rollback)?
                        .ok_or_else(|| {
                            StoreError::backend("actual SDK custody produced no pending report")
                        })?;
                    inbox
                        .resolve_report(reference)
                        .await
                        .map_err(delivery_rollback)
                })
            })
            .await?)
    }
    pub(super) async fn payload(
        &self,
        reference: DeliveryItemRef,
    ) -> ConformanceResult<DeliveryItem<BoardDeliveryTypes>> {
        let (owner, limits) = (self.owner, self.limits);
        Ok(self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    Inbox::<_, BoardDeliveryTypes, _>::new(scope, owner, limits, &RosterBinding)
                        .resolve_delivery(&reference)
                        .await
                        .map_err(delivery_rollback)
                })
            })
            .await?)
    }
    pub(super) async fn count(&self) -> ConformanceResult<i64> {
        Ok(self
            .store
            .transaction(|scope| {
                Box::pin(async move {
                    query::<sqlx_turso::Turso>(
                        "SELECT count(*) AS items FROM _fleet_sync_inbox_items",
                    )
                    .fetch_one(scope.executor())
                    .await
                    .map_err(StoreError::backend)?
                    .try_get("items")
                    .map_err(StoreError::backend)
                })
            })
            .await?)
    }
    pub(super) async fn used_bytes(&self) -> ConformanceResult<u64> {
        let used: i64 = self.store.transaction(|scope|Box::pin(async move {
            query::<sqlx_turso::Turso>("SELECT coalesce(sum(payload_bytes+settlement_charge_bytes),0) AS bytes FROM _fleet_sync_inbox_items WHERE live=1")
                .fetch_one(scope.executor()).await.map_err(StoreError::backend)?.try_get("bytes").map_err(StoreError::backend)
        })).await?;
        Ok(u64::try_from(used)?)
    }
    pub(super) fn close(self) -> ConformanceResult<()> {
        let store = Arc::try_unwrap(self.store)
            .map_err(|_| "a task still owns the recipient connection")?;
        drop(store);
        Ok(())
    }
}
