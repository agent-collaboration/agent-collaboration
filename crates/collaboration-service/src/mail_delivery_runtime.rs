//! One SDK engine owns local mail custody and native outcome projection.
use crate::MailPushDelivery;
use crate::MailPushDeliveryProps;
use crate::SubscriptionClock;
use crate::subscription_delivery::SubscriptionRuntimeClock;
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
    Inbox, LocalDelivery, LocalDeliveryConfig, ReportOutcome, SubmitItem,
    minimum_settlement_allowance,
};
use fleet_sync_protocol::{
    DeliveryHeader, DeliveryItem, DeliveryItemRef, ItemId, PeerId, RecipientOutcome,
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
mod custody_backfill;
mod held_readiness;
#[cfg(feature = "test-support")]
mod inbound_wake_probe;
mod installation_directory;
mod sender_binding;
pub use sender_binding::MachineSenderBinding;
mod local_admission;
mod local_calls;
mod local_cancellation;
mod local_project_admission;
mod poll_handover;
mod project_admission;
mod project_cancellation;
mod pushed_mail;
pub(crate) use project_cancellation::other_canonical_cancel;
mod remote_admission;
mod remote_wakes;
pub(crate) use remote_wakes::LocalWakeOwner;
mod remote_calls;
mod remote_status;
pub(crate) use installation_directory::InstallationPeerResolutionError;
pub use local_cancellation::LocalCancelReference;

type MailHost = TokioHost<MailCallRouter, SubscriptionRuntimeClock>;
type MailStore = InteractionAtomicStore<MailHost>;
type MailDatabase = <TursoScope<'static, MailHost> as TransactionScope>::Database;
type LocalMailTask = DeliverLocally<
    MailStore,
    MailDatabase,
    RouterDeliveryTypes,
    MailConsumer,
    MachineSenderBinding,
>;
type RemoveMailRecord = fleet_sync_delivery::RemoveDeliveryRecord<MailStore, MailDatabase>;
type SubmitMailTask =
    SubmitItem<MailStore, MailDatabase, RouterDeliveryTypes, MailConsumer, MachineSenderBinding>;
type ReportMailTask =
    ReportOutcome<MailStore, MailDatabase, RouterDeliveryTypes, MachineSenderBinding>;

#[derive(Clone)]
pub struct MailDeliveryRuntime {
    #[cfg(test)]
    pub(crate) cancel_admission_probe: Option<std::path::PathBuf>,
    #[cfg(test)]
    pub(crate) project_admission_probe: Option<std::path::PathBuf>,
    #[cfg(test)]
    pub(crate) backfill_admission_probe: Option<std::path::PathBuf>,
    settlement_drives: Arc<std::sync::Mutex<TaskTracker>>,
    project_replication:
        Arc<OnceLock<Arc<dyn collaboration_contracts::ProjectReplicationPosition>>>,
    owner: Arc<InteractionStore>,
    store: Arc<MailStore>,
    host: Arc<MailHost>,
    engine: Arc<Engine<MailStore, MailHost>>,
    config: LocalDeliveryConfig,
    consumer: Arc<MailConsumer>,
    pub(crate) binding: Arc<MachineSenderBinding>,
    reader_actors: crate::SubscriptionDeliveryService,
    service: message_board::ServiceId,
    clock: Arc<dyn SubscriptionClock>,
    session: Option<MailDeliverySession>,
}

impl MailDeliveryRuntime {
    /// Join due library removal tasks before acknowledging a debug Host clock step.
    #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
    pub async fn runtime_test_run_due_record_removals(&self) -> Result<(), StoreError> {
        use durable_steps::{
            Task,
            storage::{JournalStorage, OperationExecution},
        };
        let now = self.host.now();
        let removals = self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    let mut removals = Vec::new();
                    // Include leased operations: another runner may already own a due
                    // removal. Its real completion, rather than a yielded pass, is owed.
                    for operation in scope.due(SyncTime::from_unix_millis(i64::MAX)).await? {
                        let Some(record) = scope.operation(operation).await? else {
                            continue;
                        };
                        if record.kind != RemoveMailRecord::KIND {
                            continue;
                        }
                        if let OperationExecution::Running {
                            input,
                            next_attempt_at,
                            ..
                        } = record.execution
                            && next_attempt_at <= now
                        {
                            let item: ItemId =
                                serde_json::from_slice(&input).map_err(StoreError::backend)?;
                            removals.push((record.request_key, item));
                        }
                    }
                    Ok(removals)
                })
            })
            .await?;
        for (request, item) in removals {
            let status = self
                .engine
                .start_and_settle::<RemoveMailRecord>(&request, item)
                .await
                .map_err(StoreError::backend)?;
            if !matches!(status.state, OperationState::Completed { .. }) {
                return Err(StoreError::backend(
                    "due mail record removal did not complete",
                ));
            }
        }
        // The same Host clock also determines every other mail task's due time.
        self.host.wake_at(now).await.map_err(StoreError::backend)
    }

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
        Self::with_default_limits(peer, props, session, 256 * 1024 * 1024)
    }

    #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
    pub fn runtime_test_new_with_inbox_limit(
        peer: PeerId,
        props: MailPushDeliveryProps,
        session: Option<MailDeliverySession>,
        inbox_bytes: std::num::NonZeroU64,
    ) -> Result<Self, StoreError> {
        Self::with_default_limits(peer, props, session, inbox_bytes.get())
    }

    fn with_default_limits(
        peer: PeerId,
        props: MailPushDeliveryProps,
        session: Option<MailDeliverySession>,
        inbox_bytes: u64,
    ) -> Result<Self, StoreError> {
        // PD4 §3's quotas reserve the SDK-computed settlement allowance.
        let limits = DeliveryLimits::new::<RouterDeliveryTypes>(
            256 * 1024 * 1024,
            50_000,
            inbox_bytes,
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
        let service: message_board::ServiceId = String::from(props.machine.service_id().clone())
            .try_into()
            .map_err(StoreError::backend)?;
        let binding = Arc::new(MachineSenderBinding::new(service.clone(), peer));
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
            binding: binding.clone(),
            owner: owner.clone(),
            config,
            session: session.clone(),
        });
        let host = Arc::new(TokioHost::new(
            router.clone(),
            SubscriptionRuntimeClock(clock.clone()),
        ));
        let store = Arc::new(InteractionAtomicStore::new(owner.clone(), host.clone()));
        router
            .store
            .set(Arc::downgrade(&store))
            .map_err(|_| StoreError::backend("mail store was already installed"))?;
        let mut engine = Engine::new(store.clone(), host.clone());
        engine.register(LocalMailTask::new(
            config,
            consumer.clone(),
            host.clone(),
            binding.clone(),
        ))?;
        engine.register(RemoveMailRecord::new(
            fleet_sync_delivery::DeliveryHostRole::Machine,
        ))?;
        if let Some(session) = &session {
            let remote = session.task_config(config)?;
            engine.register(SubmitMailTask::new(
                limits,
                consumer.clone(),
                host.clone(),
                binding.clone(),
            ))?;
            engine.register(ReportMailTask::new(remote, host.clone(), binding.clone()))?;
        }
        Ok(Self {
            project_replication,
            #[cfg(test)]
            cancel_admission_probe: None,
            #[cfg(test)]
            project_admission_probe: None,
            #[cfg(test)]
            backfill_admission_probe: None,
            settlement_drives: Arc::new(std::sync::Mutex::new(TaskTracker::new())),
            owner,
            store,
            host,
            engine: Arc::new(engine),
            config,
            consumer,
            binding,
            reader_actors,
            service,
            clock,
            session,
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
        let admitted = tokio::select! { result = async { let admitted = self.initialize_record_removals().await?; self.initialize_submitter_bindings().await?; Ok::<_, StoreError>(admitted) } => result?, () = shutdown.cancelled() => return Ok(()) };
        if admitted > 0 {
            tracing::info!(admitted, "admitted existing mail custody removal records");
        }
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
        let running = async {
            tokio::try_join!(
                runner.run(stopped.clone()),
                self.run_sender_binding_lookups(stopped)
            )
            .map(|_| ())
        };
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
    binding: Arc<MachineSenderBinding>,
    owner: Arc<InteractionStore>,
    config: LocalDeliveryConfig,
    session: Option<MailDeliverySession>,
}

#[cfg(test)]
#[path = "startup_constructor_tests.rs"]
mod startup_constructor_tests;

/// Own the fixture's mail runtime task without constructing an application.
#[cfg(any(test, feature = "test-support"))]
pub struct NativeMailOwner {
    pub interactions: Arc<InteractionStore>,
    mail: Arc<MailDeliveryRuntime>,
    shutdown: CancellationToken,
    running: tokio::task::JoinHandle<Result<(), durable_steps::StoreError>>,
    pending_start: Option<tokio::sync::oneshot::Sender<()>>,
}

#[cfg(any(test, feature = "test-support"))]
impl NativeMailOwner {
    pub fn start_prepared(
        interactions: Arc<InteractionStore>,
        mail: Arc<MailDeliveryRuntime>,
        deferred_start: Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    ) -> Self {
        let shutdown = CancellationToken::new();
        let worker_mail = mail.clone();
        let worker_shutdown = shutdown.clone();
        let (pending_start, waiting) = match deferred_start {
            Some((release, waiting)) => (Some(release), Some(waiting)),
            None => (None, None),
        };
        let running = tokio::spawn(async move {
            if let Some(waiting) = waiting {
                tokio::select! {
                    () = worker_shutdown.cancelled() => return Ok(()),
                    started = waiting => if started.is_err() { return Ok(()); },
                }
            }
            worker_mail.run(worker_shutdown).await
        });
        Self {
            interactions,
            mail,
            shutdown,
            running,
            pending_start,
        }
    }
    pub fn start_worker(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.pending_start
            .take()
            .ok_or_else(|| std::io::Error::other("mail worker was not deferred"))?
            .send(())
            .map_err(|_| std::io::Error::other("deferred mail worker stopped"))?;
        Ok(())
    }

    // Only lifecycle proofs inspect SDK custody through its owning runtime.
    pub fn runtime(&self) -> &MailDeliveryRuntime {
        &self.mail
    }

    // Host composition keeps the same owner behind its weak session port.
    pub fn runtime_handle(&self) -> Arc<MailDeliveryRuntime> {
        self.mail.clone()
    }

    // Only the shutdown lifecycle proof stops the owner before its client returns.
    pub fn begin_shutdown(&self) {
        self.shutdown.cancel();
    }

    // The lifecycle proof checks the actual tracked runtime task.
    pub fn is_stopped(&self) -> bool {
        self.running.is_finished()
    }

    /// Stop native work while a caller still owns the shared physical database.
    pub async fn stop(
        self,
    ) -> Result<Arc<InteractionStore>, Box<dyn std::error::Error + Send + Sync>> {
        let Self {
            interactions,
            mail,
            shutdown,
            running,
            pending_start: _,
        } = self;
        shutdown.cancel();
        let joined = tokio::time::timeout(std::time::Duration::from_secs(10), running).await?;
        joined??;
        drop(mail);
        Ok(interactions)
    }

    // Only interrupted-handover proofs abandon an active SDK drive.
    pub async fn abort_for_crash(
        self,
    ) -> Result<Arc<InteractionStore>, Box<dyn std::error::Error + Send + Sync>> {
        let Self {
            interactions,
            mail,
            shutdown,
            running,
            pending_start: _,
        } = self;
        running.abort();
        match running.await {
            Err(error) if error.is_cancelled() => {}
            _ => {
                return Err(std::io::Error::other(
                    "SDK drive ended before the interruption checkpoint",
                )
                .into());
            }
        }
        shutdown.cancel();
        drop(mail);
        Ok(interactions)
    }

    /// Stop the served application and reader actors before closing their physical owner.
    // Host fixtures stop first, then check and close each store owner separately.
    pub async fn close(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Arc::try_unwrap(self.stop().await?)
            .map_err(|_| std::io::Error::other("fixture retained the native interaction owner"))?
            .close()
            .await?;
        Ok(())
    }
}
