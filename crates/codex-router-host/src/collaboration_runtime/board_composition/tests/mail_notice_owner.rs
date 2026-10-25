//! Observe SDK admission through the mail runtime's exact borrowed physical owner.
use super::ledger::TestError;
use collaboration_service::{
    BoardAvailability, MachineIdentity, MailDeliveryRuntime, MailDeliverySession,
    MailPushDeliveryProps, SessionDeliveryRouter, SessionMessageDelivery,
    SubscriptionDeliveryService, SubscriptionDeliveryServiceProps, SystemSubscriptionClock,
    TargetPresenceProbe,
};
use durable_steps::{AtomicStore, CallFailure, StoreError};
use durable_steps::{storage::TransactionScope, turso_store::TursoScope};
use fleet_sync_delivery::{
    DeliveryConsumer, DeliveryError, DeliveryLimits, GateDecision, LocalDelivery, Outbox,
    SubmitItem, delivery_rollback, minimum_settlement_allowance,
};
use fleet_sync_protocol::{DeliveryHeader, DeliveryItem, ItemId, PeerId};
use fleet_sync_tokio_runner::{CallEnvelope, CallRouter, TokioHost};
use interaction_storage::{InteractionAtomicStore, InteractionStore};
use runtime_primitives::SystemClock;
use serde_json::Value;
use std::{io, path::Path, sync::Arc};
use tokio::sync::Mutex;

struct NoObservationCalls;
impl CallRouter for NoObservationCalls {
    async fn dispatch(&self, _: CallEnvelope) -> Result<Value, CallFailure> {
        Err(CallFailure::AnswerLost)
    }
}
type ObserverHost = TokioHost<NoObservationCalls, SystemClock>;
type ObserverStore = InteractionAtomicStore<ObserverHost>;
type ObserverDatabase = <TursoScope<'static, ObserverHost> as TransactionScope>::Database;
type ArrangedSubmit = SubmitItem<
    ObserverStore,
    ObserverDatabase,
    collaboration_service::RouterDeliveryTypes,
    AdmissionMarker,
>;

// This marker supplies only the SDK's stable task kind for journal arrangement.
// The actual MailDeliveryRuntime registers and executes its own real consumer.
struct AdmissionMarker;
impl DeliveryConsumer<TursoScope<'_, ObserverHost>> for AdmissionMarker {
    type Protocol = collaboration_service::RouterConsumerProtocol;
    async fn release_gate(
        &self,
        _: &mut TursoScope<'_, ObserverHost>,
        _: &DeliveryItem<collaboration_service::RouterDeliveryTypes>,
    ) -> Result<GateDecision, DeliveryError> {
        panic!("arrangement marker must never execute a release gate")
    }
    async fn deliver_locally(
        &self,
        _: &DeliveryItem<collaboration_service::RouterDeliveryTypes>,
        _: runtime_primitives::CallId,
    ) -> LocalDelivery<collaboration_service::RouterDeliveryTypes> {
        panic!("arrangement marker must never execute native delivery")
    }
    fn may_cancel(
        &self,
        _: &collaboration_mail_protocol::MailSender,
        _: &DeliveryHeader<collaboration_service::RouterDeliveryTypes>,
    ) -> bool {
        panic!("arrangement marker must never authorize cancellation")
    }
}

pub(super) struct MailOwner {
    pub mail: Arc<MailDeliveryRuntime>,
    pub(super) owner: Arc<InteractionStore>,
    store: ObserverStore,
    pub(super) actors: SubscriptionDeliveryService,
    pub(super) automation: Arc<Mutex<automation_storage::AutomationStore>>,
}
impl MailOwner {
    pub async fn outbox_status(
        &self,
        item: ItemId,
    ) -> Result<
        fleet_sync_delivery::OutboxStatus<collaboration_mail_protocol::CarrierAcceptance>,
        TestError,
    > {
        let limits = DeliveryLimits::new::<collaboration_service::RouterDeliveryTypes>(
            64 * 1024 * 1024,
            10_000,
            64 * 1024 * 1024,
            10_000,
            64 * 1024 * 1024,
            minimum_settlement_allowance::<collaboration_service::RouterDeliveryTypes>()?,
        )?;
        Ok(self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    Outbox::<_, collaboration_service::RouterDeliveryTypes>::new(scope, limits)
                        .status(item)
                        .await
                        .map_err(delivery_rollback)
                })
            })
            .await?)
    }
    pub async fn open(
        root: &Path,
        peer: PeerId,
        session: MailDeliverySession,
    ) -> Result<Self, TestError> {
        let delivery = Arc::new(SessionDeliveryRouter::new(Vec::new()));
        let machine = MachineIdentity::new(
            "00000000-0000-4000-8000-000000000001"
                .to_owned()
                .try_into()?,
            Some("notice fixture"),
        )?;
        Self::open_for_machine(root, peer, session, machine, delivery).await
    }
    pub async fn open_for_machine<C: SessionMessageDelivery + TargetPresenceProbe + 'static>(
        root: &Path,
        peer: PeerId,
        session: MailDeliverySession,
        machine: MachineIdentity,
        delivery: Arc<C>,
    ) -> Result<Self, TestError> {
        let owner = Arc::new(InteractionStore::open(root.join("interaction.sqlite")).await?);
        let automation = Arc::new(Mutex::new(
            automation_storage::AutomationStore::open(&root.join("automation.sqlite")).await?,
        ));
        let actors = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
            board_availability: BoardAvailability::Unavailable,
            push_store: owner.clone(),
            delivery: delivery.clone(),
            presence: delivery.clone(),
            machine_identity: machine.clone(),
            clock: Arc::new(SystemSubscriptionClock),
        });
        actors.start().await?;
        let mail = Arc::new(MailDeliveryRuntime::new(
            peer,
            MailPushDeliveryProps {
                interactions: owner.clone(),
                actors: actors.clone(),
                presence: delivery,
                machine,
                clock: Arc::new(SystemSubscriptionClock),
            },
            Some(session),
        )?);
        let store = ObserverStore::new(
            owner.clone(),
            Arc::new(TokioHost::new(Arc::new(NoObservationCalls), SystemClock)),
        );
        Ok(Self {
            mail,
            owner,
            store,
            actors,
            automation,
        })
    }
    pub async fn state(&self) -> Result<Option<Value>, TestError> {
        Ok(self
            .store
            .transaction(|scope| {
                Box::pin(async move {
                    let rows = fleet_sync_delivery::test_support::snapshot(scope)
                        .await
                        .map_err(StoreError::backend)?;
                    rows.into_iter()
                        .find(|row| row.category == "inboxState")
                        .map(|row| {
                            serde_json::from_str(&row.content.ok_or_else(|| {
                                StoreError::backend("inbox state content missing")
                            })?)
                            .map_err(StoreError::backend)
                        })
                        .transpose()
                })
            })
            .await?)
    }
    pub async fn local_outcome(
        &self,
        peer: PeerId,
        item: ItemId,
    ) -> Result<
        Option<
            fleet_sync_protocol::RecipientOutcome<collaboration_mail_protocol::CarrierAcceptance>,
        >,
        TestError,
    > {
        let limits = DeliveryLimits::new::<collaboration_service::RouterDeliveryTypes>(
            64 * 1024 * 1024,
            10_000,
            64 * 1024 * 1024,
            10_000,
            64 * 1024 * 1024,
            minimum_settlement_allowance::<collaboration_service::RouterDeliveryTypes>()?,
        )?;
        Ok(self.store.transaction(move |scope| {
            Box::pin(async move {
                fleet_sync_delivery::Inbox::<_, collaboration_service::RouterDeliveryTypes>::new(scope, peer, limits)
                    .local_outcome(item).await.map_err(delivery_rollback)
            })
        }).await?)
    }

    // Fault only Router-owned progress, through the same physical writer as the SDK.
    pub async fn retirement_progress_sql(&self, statement: &'static str) -> Result<(), TestError> {
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    sqlx::raw_sql(statement).execute(scope.executor()).await?;
                    Ok(())
                })
            })
            .await?;
        Ok(())
    }

    pub async fn hold_writer(
        &self,
    ) -> Result<tokio::task::JoinHandle<Result<(), StoreError>>, TestError> {
        let store = ObserverStore::new(
            self.owner.clone(),
            Arc::new(TokioHost::new(Arc::new(NoObservationCalls), SystemClock)),
        );
        let (entered, observed) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            store
                .transaction(move |_scope| {
                    Box::pin(async move {
                        entered
                            .send(())
                            .map_err(|_| StoreError::backend("writer observer closed"))?;
                        std::future::pending::<Result<(), StoreError>>().await
                    })
                })
                .await
        });
        tokio::time::timeout(super::BOUND, observed).await??;
        Ok(task)
    }
    pub async fn rows(
        &self,
    ) -> Result<Vec<fleet_sync_delivery::test_support::DeliveryObservation>, TestError> {
        Ok(self
            .store
            .transaction(|scope| {
                Box::pin(async move {
                    fleet_sync_delivery::test_support::snapshot(scope)
                        .await
                        .map_err(StoreError::backend)
                })
            })
            .await?)
    }
    pub async fn enqueue(
        &self,
        item: DeliveryItem<collaboration_service::RouterDeliveryTypes>,
        relay: PeerId,
    ) -> Result<(ItemId, runtime_primitives::OperationId), TestError> {
        let id = item.id();
        let limits = DeliveryLimits::new::<collaboration_service::RouterDeliveryTypes>(
            64 * 1024 * 1024,
            10_000,
            64 * 1024 * 1024,
            10_000,
            64 * 1024 * 1024,
            minimum_settlement_allowance::<collaboration_service::RouterDeliveryTypes>()?,
        )?;
        let operation = self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    let (answer, admitted) =
                        Outbox::<_, collaboration_service::RouterDeliveryTypes>::new(scope, limits)
                            .enqueue_and_admit::<ArrangedSubmit>(item, relay)
                            .await
                            .map_err(delivery_rollback)?;
                    if !matches!(answer, fleet_sync_delivery::OutboxAdmission::Queued { .. })
                        || admitted.is_none()
                    {
                        return Err(StoreError::backend(
                            "fixture did not admit a fresh real outbox task",
                        ));
                    }
                    admitted
                        .map(|admitted| admitted.id())
                        .ok_or_else(|| StoreError::backend("fixture submission operation missing"))
                })
            })
            .await?;
        Ok((id, operation))
    }
    pub async fn enqueue_with_epoch(
        &self,
        item: DeliveryItem<collaboration_service::RouterDeliveryTypes>,
        epoch: message_board::EpochNumber,
    ) -> Result<(ItemId, runtime_primitives::OperationId), TestError> {
        use durable_steps::{Task as _, storage::JournalStorage as _};
        let id = item.id();
        let key = fleet_sync_delivery::delivery_request_key(&id)?;
        let admitted = self
            .mail
            .enqueue_project_item(item, epoch)
            .await
            .map_err(|error| {
                io::Error::other(format!("actual project Outbox admission: {error:?}"))
            })?;
        if !matches!(
            admitted,
            fleet_sync_delivery::OutboxAdmission::Queued { .. }
        ) {
            return Err(io::Error::other("expected fresh project custody").into());
        }
        // Read the actual committed journal. Never admit a task here: that
        // would hide a missing production admission from the wire proof.
        let operation = self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    for candidate in scope
                        .due(runtime_primitives::SyncTime::from_unix_millis(i64::MAX))
                        .await?
                    {
                        if let Some(record) = scope.operation(candidate).await?
                            && record.kind == ArrangedSubmit::KIND
                            && record.request_key == key
                        {
                            return Ok(candidate);
                        }
                    }
                    Err(StoreError::backend(
                        "production did not admit the actual project submit operation",
                    ))
                })
            })
            .await?;
        Ok((id, operation))
    }
    pub async fn operation(
        &self,
        operation: runtime_primitives::OperationId,
    ) -> Result<
        (
            Option<durable_steps::storage::OperationRecord>,
            Vec<durable_steps::storage::StepRecord>,
        ),
        TestError,
    > {
        use durable_steps::storage::JournalStorage;
        Ok(self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    let record = scope.operation(operation).await?;
                    let count = scope.step_count(operation).await?;
                    let mut steps = Vec::new();
                    for position in 0..count {
                        if let Some(step) = scope.step(operation, position).await? {
                            steps.push(step);
                        }
                    }
                    Ok((record, steps))
                })
            })
            .await?)
    }
    pub async fn status_cursor(&self) -> Result<u64, TestError> {
        let limits = DeliveryLimits::new::<collaboration_service::RouterDeliveryTypes>(
            64 * 1024 * 1024,
            10_000,
            64 * 1024 * 1024,
            10_000,
            64 * 1024 * 1024,
            minimum_settlement_allowance::<collaboration_service::RouterDeliveryTypes>()?,
        )?;
        Ok(self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    Outbox::<_, collaboration_service::RouterDeliveryTypes>::new(scope, limits)
                        .status_cursor()
                        .await
                        .map(|cursor| cursor.value())
                        .map_err(delivery_rollback)
                })
            })
            .await?)
    }
    pub async fn close(self) -> Result<(), TestError> {
        let Self {
            mail,
            owner,
            store,
            actors,
            automation,
        } = self;
        actors.shutdown().await;
        drop(mail);
        drop(actors);
        drop(store);
        Arc::try_unwrap(automation)
            .map_err(|_| io::Error::other("notice actor retained"))?
            .into_inner()
            .close()
            .await?;
        Arc::try_unwrap(owner)
            .map_err(|_| io::Error::other("mail observation owner retained"))?
            .close()
            .await?;
        Ok(())
    }
}
