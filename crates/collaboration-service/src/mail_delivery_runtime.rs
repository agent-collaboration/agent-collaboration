//! One SDK engine owns local mail custody and native outcome projection.
use crate::MailPushDelivery;
use crate::MailPushDeliveryProps;
use crate::SubscriptionClock;
use collaboration_contracts::LoadPolicy;
use collaboration_contracts::RouterConsumerProtocol;
use collaboration_contracts::RouterDeliveryTypes;
use collaboration_mail_protocol::{CarrierAcceptance, MailSender};
use collaboration_protocol::{DeliveryReceipt, PushMessageSendResult, PushRecordDraft};
use collaboration_runtime::MailDeliverySession;
use collaboration_runtime::outgoing_mail_message;
use collaboration_runtime::project_mail_cancel_outcome;
use collaboration_runtime::project_mail_outcome;
use collaboration_runtime::project_received_mail;
use durable_steps::{
    AtomicStore, CallFailure, CallId, CallTarget, Engine, NotSentReason, OperationState,
    StoreError, storage::TransactionScope, turso_store::TursoScope,
};
use fleet_sync_delivery::{
    DeliverCall, DeliverLocally, DeliveryConsumer, DeliveryError, DeliveryLimits, GateDecision,
    Inbox, LocalDelivery, LocalDeliveryConfig, ReceiveItems, ReportOutcome, SubmitItem,
    minimum_settlement_allowance,
};
use fleet_sync_protocol::{
    DeliveryHeader, DeliveryItem, DeliveryItemRef, DeliveryStream, ItemId, PeerId, RecipientOutcome,
};
use fleet_sync_tokio_runner::{CallEnvelope, CallRouter, TokioHost, TokioRunner};
use interaction_storage::{
    InteractionAtomicStore, InteractionStore, mark_mail_push_attempted_in_scope,
    read_mail_push_in_scope,
};
use runtime_primitives::{Clock, SyncTime, WakeScheduler};
use serde_json::Value;
use std::sync::{Arc, OnceLock, Weak};
use tokio::sync::watch;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

mod frozen_cancel_outcome;
mod frozen_handoff_status;
pub(crate) use frozen_cancel_outcome::{FrozenCancelSettlement, RemoteCancelReplay};
mod held_readiness;
#[cfg(feature = "test-support")]
mod inbound_wake_probe;
mod installation_directory;
mod local_admission;
mod local_cancellation;
mod local_project_admission;
mod poll_handover;
mod project_admission;
mod project_cancellation;
pub(crate) use project_cancellation::other_canonical_cancel;
mod remote_admission;
mod remote_wakes;
pub(crate) use remote_wakes::LocalWakeOwner;
mod remote_calls;
mod remote_status;
mod terminal_retirement;
pub(crate) use installation_directory::InstallationPeerResolutionError;
pub use local_cancellation::LocalCancelReference;

type MailHost = TokioHost<MailCallRouter, crate::SubscriptionRuntimeClock>;
type MailStore = InteractionAtomicStore<MailHost>;
type MailDatabase = <TursoScope<'static, MailHost> as TransactionScope>::Database;
type LocalMailTask = DeliverLocally<MailStore, MailDatabase, RouterDeliveryTypes, MailConsumer>;
type SubmitMailTask = SubmitItem<MailStore, MailDatabase, RouterDeliveryTypes, MailConsumer>;
type ReceiveMailTask = ReceiveItems<MailStore, MailDatabase, RouterDeliveryTypes, MailConsumer>;
type ReportMailTask = ReportOutcome<MailStore, MailDatabase, RouterDeliveryTypes>;

#[derive(Clone)]
pub struct MailDeliveryRuntime {
    #[cfg(test)]
    pub(crate) cancel_admission_probe: Option<std::path::PathBuf>,
    #[cfg(test)]
    pub(crate) project_admission_probe: Option<std::path::PathBuf>,
    settlement_drives: Arc<std::sync::Mutex<TaskTracker>>,
    project_replication:
        Arc<OnceLock<Arc<dyn collaboration_contracts::ProjectReplicationPosition>>>,
    owner: Arc<InteractionStore>,
    store: Arc<MailStore>,
    host: Arc<MailHost>,
    engine: Arc<Engine<MailStore, MailHost>>,
    config: LocalDeliveryConfig,
    consumer: Arc<MailConsumer>,
    reader_actors: crate::SubscriptionDeliveryService,
    service: message_board::ServiceId,
    clock: Arc<dyn SubscriptionClock>,
    session: Option<MailDeliverySession>,
}

impl MailDeliveryRuntime {
    #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
    pub async fn runtime_test_has_local_cancel_delivery(
        &self,
        cancel: ItemId,
    ) -> Result<bool, StoreError> {
        let item = cancel
            .as_uuid()
            .to_string()
            .try_into()
            .map_err(StoreError::backend)?;
        self.owner
            .local_cancel_delivery(&item)
            .await
            .map(|delivery| delivery.is_some())
            .map_err(StoreError::backend)
    }

    #[cfg(feature = "runtime-test-support")]
    pub async fn runtime_test_install_mail_fault(
        &self,
        fault: interaction_storage::MailStoreTestFault,
    ) -> Result<(), StoreError> {
        self.owner
            .runtime_test_install_mail_fault(fault)
            .await
            .map_err(StoreError::backend)
    }

    /// The existing reader-delivery owner composed with this physical mail runtime.
    #[must_use]
    pub fn subscription_delivery(&self) -> &crate::SubscriptionDeliveryService {
        &self.reader_actors
    }

    #[cfg(test)]
    pub(crate) async fn drive_due_for_test(&self) -> Result<(), StoreError> {
        self.engine.drive_due().await.map(|_| ())
    }

    #[cfg(test)]
    pub(crate) fn without_registered_task_for_test(mut self) -> Self {
        // The real SDK rejects settlement after the admission transaction commits.
        self.engine = Arc::new(Engine::new(self.store.clone(), self.host.clone()));
        self
    }

    /// Construction registers work but starts no background task or native call.
    pub fn new(
        peer: PeerId,
        props: MailPushDeliveryProps,
        session: Option<MailDeliverySession>,
    ) -> Result<Self, StoreError> {
        // PD4 §3's quotas reserve the SDK-computed settlement allowance.
        let limits = DeliveryLimits::new::<RouterDeliveryTypes>(
            1024 * 1024 * 1024,
            200_000,
            256 * 1024 * 1024,
            50_000,
            256 * 1024 * 1024,
            minimum_settlement_allowance::<RouterDeliveryTypes>().map_err(StoreError::backend)?,
        )
        .map_err(StoreError::backend)?;
        Self::with_limits(peer, props, limits, session)
    }

    #[cfg(test)]
    pub(crate) fn with_test_limits(
        peer: PeerId,
        props: MailPushDeliveryProps,
        limits: DeliveryLimits,
    ) -> Result<Self, StoreError> {
        Self::with_limits(peer, props, limits, None)
    }

    #[cfg(test)]
    pub(crate) fn with_test_session_limits(
        peer: PeerId,
        props: MailPushDeliveryProps,
        limits: DeliveryLimits,
        session: MailDeliverySession,
    ) -> Result<Self, StoreError> {
        Self::with_limits(peer, props, limits, Some(session))
    }

    fn with_limits(
        peer: PeerId,
        props: MailPushDeliveryProps,
        limits: DeliveryLimits,
        session: Option<MailDeliverySession>,
    ) -> Result<Self, StoreError> {
        if session.as_ref().is_some_and(|session| session.peer != peer) {
            return Err(StoreError::backend(
                "mail session peer does not match the installation peer",
            ));
        }
        let owner = props.interactions.clone();
        let clock = props.clock.clone();
        let service = String::from(props.machine.service_id().clone())
            .try_into()
            .map_err(StoreError::backend)?;
        let config = LocalDeliveryConfig {
            owner: peer,
            limits,
        };
        let reader_actors = props.actors.clone();
        let consumer = Arc::new(MailConsumer {
            reader_actors: reader_actors.clone(),
            carrier: MailPushDelivery::new(props),
            wakes: OnceLock::new(),
            clock: clock.clone(),
        });
        let project_replication = Arc::new(OnceLock::new());
        let router = Arc::new(MailCallRouter {
            project_replication: project_replication.clone(),
            store: OnceLock::new(),
            consumer: consumer.clone(),
            config,
            session: session.clone(),
        });
        let host = Arc::new(TokioHost::new(router.clone(), crate::SubscriptionRuntimeClock::new(clock.clone())));
        let store = Arc::new(InteractionAtomicStore::new(owner.clone(), host.clone()));
        router
            .store
            .set(Arc::downgrade(&store))
            .map_err(|_| StoreError::backend("mail store was already installed"))?;
        let mut engine = Engine::new(store.clone(), host.clone());
        engine.register(LocalMailTask::new(config, consumer.clone(), host.clone()))?;
        if let Some(session) = &session {
            let remote = session.task_config(config)?;
            engine.register(SubmitMailTask::new(limits, consumer.clone()))?;
            engine.register(ReceiveMailTask::new(remote, consumer.clone()))?;
            engine.register(ReportMailTask::new(remote))?;
            engine.register(terminal_retirement::RetireTerminalMail { limits })?;
        }
        Ok(Self {
            project_replication,
            #[cfg(test)]
            cancel_admission_probe: None,
            #[cfg(test)]
            project_admission_probe: None,
            settlement_drives: Arc::new(std::sync::Mutex::new(TaskTracker::new())),
            owner,
            store,
            host,
            engine: Arc::new(engine),
            config,
            consumer,
            reader_actors,
            service,
            clock,
            session,
        })
    }

    /// Admission commits before the session may acknowledge its hint.
    pub async fn prompt_receive(&self, stream: DeliveryStream) -> Result<(), StoreError> {
        self.admit_receive_cycles(vec![stream]).await
    }

    pub async fn prompt_receive_all(&self) -> Result<(), StoreError> {
        self.admit_receive_cycles(vec![DeliveryStream::Controls, DeliveryStream::Items])
            .await
    }

    async fn admit_receive_cycles(&self, streams: Vec<DeliveryStream>) -> Result<(), StoreError> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| StoreError::backend("remote mail session is not attached"))?;
        if !*session.welcomed.borrow() {
            return Err(StoreError::backend("remote mail session is not welcomed"));
        }
        let config = self.config;
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    for stream in streams {
                        Inbox::<_, RouterDeliveryTypes>::new(scope, config.owner, config.limits)
                            .begin_receive_and_admit::<ReceiveMailTask>(stream)
                            .await
                            .map_err(fleet_sync_delivery::delivery_rollback)?;
                    }
                    Ok(())
                })
            })
            .await?;
        self.host.wake_at(self.host.now()).await.map_err(|_| {
            StoreError::backend("mail receive admission committed but the runner wake failed")
        })
    }

    /// Refuses attaching mail to a different installation or physical interaction owner.
    pub fn validate_owner(
        &self,
        identity: &collaboration_contracts::RouterIdentity,
        interactions: Option<&Arc<interaction_storage::InteractionStore>>,
    ) -> Result<(), String> {
        if self.service.as_str() == String::from(identity.service_id().clone())
            && identity.installation_peer() == Some(self.config.owner)
            && interactions.is_some_and(|owner| Arc::ptr_eq(owner, &self.owner))
        {
            Ok(())
        } else {
            Err(
                "mail runtime has a different service, installation peer or physical store owner"
                    .to_owned(),
            )
        }
    }

    fn close_settlement_admission(&self) -> Result<TaskTracker, StoreError> {
        let settlements = self
            .settlement_drives
            .lock()
            .map_err(|_| StoreError::backend("mail settlement admission lock poisoned"))?;
        settlements.close();
        Ok(settlements.clone())
    }

    #[cfg(test)]
    pub(crate) fn settlement_admission_closed_for_test(&self) -> Result<bool, StoreError> {
        self.settlement_drives
            .lock()
            .map(|settlements| settlements.is_closed())
            .map_err(|_| StoreError::backend("mail settlement admission lock poisoned"))
    }

    /// Host monitors this future and drains in-flight native work during shutdown.
    pub async fn run(&self, shutdown: CancellationToken) -> Result<(), StoreError> {
        for target in self
            .owner
            .held_mail_targets()
            .await
            .map_err(StoreError::backend)?
        {
            self.reader_actors.prompt_native_mail_outcome(target);
        }
        let (stop, stopped) = watch::channel(false);
        let runner = TokioRunner::new(self.engine.clone(), self.host.clone());
        let running = runner.run(stopped);
        tokio::pin!(running);
        tokio::select! {
            result = &mut running => {
                let settlements = self.close_settlement_admission()?;
                settlements.wait().await;
                result
            },
            () = shutdown.cancelled() => {
                let settlements = self.close_settlement_admission()?;
                stop.send_replace(true);
                let (result, ()) = tokio::join!(running, settlements.wait());
                result
            }
        }
    }
}

struct MailConsumer {
    reader_actors: crate::SubscriptionDeliveryService,
    carrier: MailPushDelivery,
    clock: Arc<dyn SubscriptionClock>,
    wakes: OnceLock<LocalWakeOwner>,
}
impl DeliveryConsumer<TursoScope<'_, MailHost>> for MailConsumer {
    type Protocol = RouterConsumerProtocol;
    async fn on_received(
        &self,
        scope: &mut TursoScope<'_, MailHost>,
        item: &DeliveryItem<RouterDeliveryTypes>,
    ) -> Result<(), DeliveryError> {
        project_received_mail(scope, item, self.clock.now()).await?;
        if collaboration_runtime::received_mail_push::is_poll_mail(item) {
            let target = item.recipient().address.clone().try_into().map_err(|_| {
                DeliveryError::Corrupt {
                    field: "Poll recipient",
                }
            })?;
            // This hint waits behind the receive transaction. It also discovers
            // new remote items for a wait that was already registered.
            self.reader_actors.prompt_native_mail_outcome(target);
        }
        Ok(())
    }
    async fn on_outcome(
        &self,
        scope: &mut TursoScope<'_, MailHost>,
        item: ItemId,
        outcome: &RecipientOutcome<CarrierAcceptance>,
        observation: Option<&collaboration_contracts::RouterLocalObservation>,
    ) -> Result<(), DeliveryError> {
        let pending_poll = interaction_storage::read_pending_poll_in_scope(scope, item)
            .await
            .map_err(|error| DeliveryError::Storage(error.to_string()))?;
        let poll_outcome = pending_poll.is_some();
        project_mail_outcome(scope, item, outcome, observation, self.clock.now()).await?;
        if let Some(pending) = pending_poll
            && !matches!(outcome, RecipientOutcome::Held)
        {
            self.reader_actors
                .prompt_native_mail_outcome(pending.recipient);
        }
        {
            let push =
                item.as_uuid()
                    .to_string()
                    .try_into()
                    .map_err(|_| DeliveryError::Corrupt {
                        field: "native mail outcome identity",
                    })?;
            let record = interaction_storage::read_mail_push_in_scope(scope, &push)
                .await
                .map_err(|error| DeliveryError::Storage(error.to_string()))?;
            if !poll_outcome
                && matches!(
                    outcome,
                    RecipientOutcome::Held | RecipientOutcome::Prevented { .. }
                )
                && let Some(record) = &record
            {
                self.reader_actors
                    .prompt_native_mail_outcome(record.target.clone());
            }
            if !matches!(outcome, RecipientOutcome::Held)
                && record.as_ref().is_none_or(|record| {
                    matches!(
                        record.kind,
                        collaboration_protocol::PushKind::SubscriptionActivity
                            | collaboration_protocol::PushKind::BoardNotification
                    )
                })
            {
                self.reader_actors.prompt_project_mail_outcome(item);
            }
        }
        Ok(())
    }
    async fn on_cancel_outcome(
        &self,
        scope: &mut TursoScope<'_, MailHost>,
        cancel: ItemId,
        target: ItemId,
        outcome: &RecipientOutcome<CarrierAcceptance>,
    ) -> Result<(), DeliveryError> {
        project_mail_cancel_outcome(scope, cancel, target, outcome, self.clock.now()).await
    }
    async fn release_gate(
        &self,
        _: &mut TursoScope<'_, MailHost>,
        _: &DeliveryItem<RouterDeliveryTypes>,
    ) -> Result<GateDecision, DeliveryError> {
        // The publisher checks local project eligibility before admission. Frozen
        // recipient policy and cancellation remain in the carrier callbacks.
        Ok(GateDecision::Release)
    }
    async fn deliver_locally(
        &self,
        item: &DeliveryItem<RouterDeliveryTypes>,
        call: CallId,
    ) -> LocalDelivery<RouterDeliveryTypes> {
        self.deliver_with_load_policy(item, call, LoadPolicy::LoadedOnly)
            .await
    }
    fn may_cancel(&self, sender: &MailSender, item: &DeliveryHeader<RouterDeliveryTypes>) -> bool {
        sender == item.sender()
    }
}

impl MailConsumer {
    async fn deliver_with_load_policy(
        &self,
        item: &DeliveryItem<RouterDeliveryTypes>,
        call: CallId,
        direct_load_policy: LoadPolicy,
    ) -> LocalDelivery<RouterDeliveryTypes> {
        match self
            .carrier
            .deliver_push_with_load_policy(item, &call, direct_load_policy)
            .await
        {
            Ok(Some(answer)) => answer,
            Ok(None) => LocalDelivery {
                outcome: fleet_sync_delivery::LocalOutcome::Held { until: None },
                observation: DeliveryReceipt {
                    outcome: collaboration_protocol::DeliveryOutcome::NotSubmitted {
                        retryable: true,
                        reason: "Poll item waits for a matching waiter".to_owned(),
                    },
                    reachability: None,
                    client: None,
                }
                .into(),
            },
            Err(_) => LocalDelivery {
                outcome: fleet_sync_delivery::LocalOutcome::Uncertain,
                observation: DeliveryReceipt {
                    outcome: collaboration_protocol::DeliveryOutcome::Unknown,
                    reachability: None,
                    client: None,
                }
                .into(),
            },
        }
    }
}

struct MailCallRouter {
    project_replication:
        Arc<OnceLock<Arc<dyn collaboration_contracts::ProjectReplicationPosition>>>,
    store: OnceLock<Weak<MailStore>>,
    consumer: Arc<MailConsumer>,
    config: LocalDeliveryConfig,
    session: Option<MailDeliverySession>,
}
impl CallRouter for MailCallRouter {
    async fn dispatch(&self, envelope: CallEnvelope) -> Result<Value, CallFailure> {
        if envelope.kind != DeliverCall::<RouterDeliveryTypes>::KIND {
            return self.dispatch_remote(envelope).await;
        }
        let _: DeliverCall<RouterDeliveryTypes> =
            serde_json::from_value(envelope.target).map_err(|_| CallFailure::AnswerLost)?;
        let reference: DeliveryItemRef =
            serde_json::from_value(envelope.request).map_err(|_| CallFailure::AnswerLost)?;
        let store = self
            .store
            .get()
            .and_then(Weak::upgrade)
            .ok_or(CallFailure::AnswerLost)?;
        let config = self.config;
        let (resolved, direct_load_policy) = store
            .transaction(move |scope| {
                Box::pin(async move {
                    let mut inbox =
                        Inbox::<_, RouterDeliveryTypes>::new(scope, config.owner, config.limits);
                    let resolved = inbox
                        .resolve_local_delivery(&reference)
                        .await
                        .map_err(StoreError::backend)?;
                    let item = resolved.item();
                    // SDK custody retains Held for later attempt identities, even
                    // after readiness makes the application projection Pending.
                    let first_attempt = inbox
                        .local_outcome(item.id())
                        .await
                        .map_err(StoreError::backend)?
                        .is_none();
                    let push_id = item
                        .id()
                        .as_uuid()
                        .to_string()
                        .try_into()
                        .map_err(StoreError::backend)?;
                    let mut direct_load_policy = LoadPolicy::LoadedOnly;
                    if !collaboration_runtime::received_mail_push::is_poll_mail(item)
                        && !matches!(item.body(), board_sync_protocol::BoardDeliveryBody::Wake(_))
                    {
                        let current = read_mail_push_in_scope(scope, &push_id)
                            .await
                            .map_err(StoreError::backend)?
                            .ok_or_else(|| StoreError::backend("mail push is missing"))?;
                        let draft = collaboration_runtime::received_mail_push(item, current.created_at)
                            .map_err(StoreError::backend)?
                            .ok_or_else(|| StoreError::backend("native mail has no push"))?;
                        // The envelope freezes mode/guard. Only the pre-attempt state
                        // authorizes this call to load; no value survives a replay.
                        direct_load_policy = if first_attempt {
                            collaboration_runtime::mail_push_preparation::direct_message_load_policy(
                                draft.mode,
                                draft.guard.as_ref(),
                                current.delivery_state,
                            )
                        } else {
                            LoadPolicy::LoadedOnly
                        };
                        mark_mail_push_attempted_in_scope(scope, &push_id)
                            .await
                            .map_err(StoreError::backend)?;
                    }
                    Ok((resolved, direct_load_policy))
                })
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        let item = resolved.item();
        let delivery = if let board_sync_protocol::BoardDeliveryBody::Wake(body) = item.body() {
            self.consumer.deliver_wake(item, body).await
        } else if collaboration_runtime::received_mail_push::is_poll_mail(item) {
            // The reader actor owns its input; retain the resolved custody value
            // here so its stored receipt bound still validates the returned answer.
            let poll_item = fleet_sync_protocol::DeliveryItem::new(
                item.id(),
                item.sender().clone(),
                item.recipient().clone(),
                item.valid_until(),
                item.body().clone(),
            )
            .map_err(|_| CallFailure::AnswerLost)?;
            let observed = self
                .consumer
                .reader_actors
                .attempt_poll_mail_once(Box::new(poll_item))
                .await;
            crate::mail_push_outcome::local_follow_outcome(
                observed.into_local_observation(),
                0,
                SyncTime::from_unix_millis(self.consumer.clock.now().timestamp_millis()),
            )
        } else {
            self.consumer
                .deliver_with_load_policy(item, envelope.call, direct_load_policy)
                .await
        };
        serde_json::to_value(resolved.validated_answer(delivery))
            .map_err(|_| CallFailure::AnswerLost)
    }
}

#[cfg(test)]
#[path = "startup_constructor_tests.rs"]
mod startup_constructor_tests;
