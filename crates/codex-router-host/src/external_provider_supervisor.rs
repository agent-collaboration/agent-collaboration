//! Host-owned admission and settlement for external ACP provider operations.

mod provider_operation_admission;
mod provider_prompt_send;
mod provider_queued_api_prompt;
pub(crate) use provider_queued_api_prompt::AdmittedApiPrompt;
mod provider_operation_failure;
mod provider_prompt_contents;
pub(crate) use provider_prompt_contents::dispatch_refusal_reason;
pub use provider_prompt_contents::{PromptBusyPolicy, ProviderPromptContentsRequest};
mod provider_conversation_backend;
mod provider_session_inspection;
mod provider_session_lifecycle;
mod provider_session_scratch;
mod provider_settings_control;
use provider_operation_failure::{
    admission_failure, invalid_setting_failure, prompt_runtime_failure, runtime_failure,
};

use crate::provider_operation_settlement::{
    ProviderOperationCompletion, effective_settings, optional_message_text,
    provider_applied_setting, provider_failed_setting, provider_session_record,
};
use crate::provider_queue_operation_registry::ProviderQueueOperationRegistry;
use crate::{ExternalProviderRuntime, ExternalProviderRuntimeError};
use collaboration_contracts::ProviderConversationBackend;
use collaboration_contracts::ProviderConversationFuture;
use collaboration_contracts::ProviderSessionInspectFuture;
use collaboration_contracts::ProviderSettingsFuture;
use collaboration_protocol::{
    ConversationAdmissionState, ConversationBindingIdentity, ConversationCancelRequest,
    ConversationCloseRequest, ConversationCreateRequest, ConversationLoadRequest,
    ConversationOperationFailure, ConversationOperationFailureKind,
    ConversationOperationFailureStage, ConversationOperationReconcileRequest,
    ConversationOperationSettlement, ConversationOperationShowRequest,
    ConversationOperationSnapshot, ConversationOperationSubmission,
    ConversationOperationWaitOutput, ConversationOperationWaitRequest,
    ConversationOperationWaitResult, ConversationOutputUnavailableReason,
    ConversationPromptRequest, ConversationResumeRequest, EndpointRef, NonEmptyText, OperationId,
    ProviderBindingIdentity, ProviderIdentity, ProviderOperationEffect, ProviderOperationKind,
    ProviderOperationStage, ProviderPromptStopReason, ProviderReconciliationState,
    ProviderSessionInspectRequest, ProviderSettingsAcceptRequest, ProviderSettingsSetRequest,
    SessionId, SessionRef,
};
use collaboration_runtime::ProviderOperationAdmission;
use collaboration_runtime::ProviderOperationAdmissionResult;
use collaboration_runtime::ProviderOperationRecord;
use collaboration_runtime::ProviderOperationStore;
use collaboration_service::conversation_operation_snapshot as snapshot_from_record;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex, Notify, mpsc};

const MAX_RETAINED_SETTLEMENTS: usize = 256;

pub struct ExternalProviderBinding {
    pub identity: ProviderBindingIdentity,
    pub runtime: ExternalProviderRuntime,
}

/// What a session was asked to run in and why its actual mode differs.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct RequestedModeRecord {
    requested_mode: Option<String>,
    mode_reason: Option<collaboration_protocol::ProviderModeReason>,
}

#[derive(Clone)]
pub struct ExternalProviderSupervisor {
    inner: Arc<SupervisorInner>,
}

struct SupervisorInner {
    bindings: HashMap<EndpointRef, RuntimeBinding>,
    store: Arc<Mutex<ProviderOperationStore>>,
    live_operations: StdMutex<LiveOperations>,
    /// Moves each time a live operation publishes its result, so schedulers and
    /// provider retention wake on an ACP completion that writes no row.
    completions: tokio::sync::watch::Sender<u64>,
    queued_operations: ProviderQueueOperationRegistry,
    hub: Option<Arc<collaboration_runtime::ProviderSessionEventHub>>,
    /// `<router root>/scratch`: each Claude or Cursor session created here gets
    /// its own `session-<uuidv7>` folder in it (R4). `None` grants no folder.
    scratch_root: Option<PathBuf>,
    history_unavailable: Mutex<HashSet<SessionRef>>,
    settings_catalogs: Arc<Mutex<HashMap<SessionRef, acp_client_runtime::ProviderSettingsCatalog>>>,
    /// The mode each session created here was asked for, and why its actual
    /// mode differs (R3). In memory: after a Host restart show reports only the
    /// agent's actual mode.
    requested_modes: Mutex<HashMap<SessionRef, RequestedModeRecord>>,
    catalog_tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    started_at_ms: i64,
    /// The approver of each prompt this Host dispatched, until it settles: a
    /// message steered into that turn is approved by it (D1 C1).
    turn_approvers: StdMutex<HashMap<OperationId, collaboration_protocol::ProviderIdentity>>,
    /// The session line API prompts wait in when the agent can't steer (D1
    /// R12), given once the delivery route is composed. Weak: the line holds
    /// this supervisor.
    message_fifo: std::sync::OnceLock<
        std::sync::Weak<crate::provider_acp_message_fifo::ProviderAcpMessageFifo>,
    >,
    #[cfg(test)]
    admission_test_pause: StdMutex<Option<AdmissionTestPause>>,
    /// Pauses the next queued API dispatch right after it claims its entry.
    #[cfg(test)]
    dispatch_test_pause: StdMutex<Option<AdmissionTestPause>>,
    /// Pauses the next admitted dispatch after its submission is spawned and
    /// before its dispatch observation is awaited.
    #[cfg(test)]
    observation_test_pause: StdMutex<Option<AdmissionTestPause>>,
}

#[cfg(test)]
struct AdmissionTestPause {
    entered: tokio::sync::oneshot::Sender<()>,
    release: tokio::sync::oneshot::Receiver<()>,
}

struct RuntimeBinding {
    identity: ProviderBindingIdentity,
    runtime: Arc<ExternalProviderRuntime>,
}

#[derive(Default)]
struct LiveOperations {
    operations: HashMap<OperationId, Arc<LiveOperation>>,
    terminal_order: VecDeque<OperationId>,
}

struct LiveOperation {
    result: Mutex<Option<Result<ConversationOperationSettlement, ConversationOperationFailure>>>,
    /// Set once the result is published. Retention reads it without the result lock, which
    /// a reader of the result may be holding.
    published: AtomicBool,
    changed: Notify,
}

/// A held operation's result, for a test that locks it as a reader would.
#[cfg(test)]
pub(crate) struct HeldResult(Arc<LiveOperation>);

#[cfg(test)]
impl HeldResult {
    /// Takes the result lock now, without waiting, or `None` when it is taken.
    pub(crate) fn try_hold(&self) -> Option<impl Sized + '_> {
        self.0.result.try_lock().ok()
    }
}

impl LiveOperation {
    fn pending() -> Self {
        Self {
            result: Mutex::new(None),
            published: AtomicBool::new(false),
            changed: Notify::new(),
        }
    }
}

impl ExternalProviderSupervisor {
    pub(crate) fn queued_operation_registry(&self) -> &ProviderQueueOperationRegistry {
        &self.inner.queued_operations
    }

    pub(crate) fn runtime_for(
        &self,
        endpoint: &EndpointRef,
    ) -> Option<Arc<ExternalProviderRuntime>> {
        self.inner
            .bindings
            .get(endpoint)
            .map(|entry| Arc::clone(&entry.runtime))
    }

    /// Announces a completion as a live operation publishing its result does.
    #[cfg(test)]
    pub(crate) fn announce_completion(&self) {
        self.inner
            .completions
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }

    /// Pauses the next queued API dispatch just after it claims its entry:
    /// the first receiver fires there, and sending on the returned sender lets
    /// it go on.
    #[cfg(test)]
    pub(crate) fn pause_next_queued_dispatch_after_claim(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered, entered_receiver) = tokio::sync::oneshot::channel();
        let (release_sender, release) = tokio::sync::oneshot::channel();
        *self
            .inner
            .dispatch_test_pause
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(AdmissionTestPause { entered, release });
        (entered_receiver, release_sender)
    }

    /// Pauses the next admitted dispatch with its submission spawned and its
    /// dispatch observation not yet awaited.
    #[cfg(test)]
    pub(crate) fn pause_next_dispatch_before_observation(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered, entered_receiver) = tokio::sync::oneshot::channel();
        let (release_sender, release) = tokio::sync::oneshot::channel();
        *self
            .inner
            .observation_test_pause
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(AdmissionTestPause { entered, release });
        (entered_receiver, release_sender)
    }

    /// Holds `operation_id` live, as admitting it to a provider does.
    #[cfg(test)]
    pub(crate) fn hold_live_operation(&self, operation_id: OperationId) {
        self.inner
            .live_operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .operations
            .insert(operation_id, Arc::new(LiveOperation::pending()));
    }

    /// The result lock of a held operation, which a test holds as a reader of the result
    /// would.
    #[cfg(test)]
    pub(crate) fn held_result_for_test(&self, operation_id: &OperationId) -> Option<HeldResult> {
        self.inner
            .live_operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .operations
            .get(operation_id)
            .cloned()
            .map(HeldResult)
    }

    /// Publishes a held operation's result through the path a finished operation takes,
    /// with no store write. Returns whether the operation was held.
    #[cfg(test)]
    pub(crate) async fn publish_held_result(&self, operation_id: &OperationId) -> bool {
        let live = self
            .inner
            .live_operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .operations
            .get(operation_id)
            .cloned();
        let Some(live) = live else {
            return false;
        };
        let result = Err(unavailable_failure(operation_id.clone(), None));
        self.inner
            .publish_result(operation_id.clone(), &live, result)
            .await;
        true
    }

    /// A receiver that wakes after each live operation publishes its result. Subscribe
    /// before checking an operation, so a completion in between is not missed.
    #[must_use]
    pub fn subscribe_completions(&self) -> tokio::sync::watch::Receiver<u64> {
        self.inner.completions.subscribe()
    }

    #[must_use]
    pub fn live_operation_ids(&self) -> std::collections::HashSet<OperationId> {
        self.inner
            .live_operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .operations
            .iter()
            .filter(|(_, operation)| !operation.published.load(Ordering::Acquire))
            .map(|(operation_id, _)| operation_id.clone())
            .collect()
    }
    pub fn new(
        bindings: Vec<ExternalProviderBinding>,
        store: Arc<Mutex<ProviderOperationStore>>,
    ) -> Result<Self, &'static str> {
        Self::new_with_hub(bindings, store, None, None)
    }

    pub(crate) fn new_with_hub(
        bindings: Vec<ExternalProviderBinding>,
        store: Arc<Mutex<ProviderOperationStore>>,
        hub: Option<Arc<collaboration_runtime::ProviderSessionEventHub>>,
        scratch_root: Option<PathBuf>,
    ) -> Result<Self, &'static str> {
        let mut runtimes = HashMap::with_capacity(bindings.len());
        let mut catalog_tasks = Vec::with_capacity(bindings.len());
        let settings_catalogs = Arc::new(Mutex::new(HashMap::new()));
        for binding in bindings {
            let endpoint = binding.identity.endpoint.clone();
            if runtimes.contains_key(&endpoint) {
                return Err("external provider endpoint binding must be unique");
            }
            let runtime = Arc::new(binding.runtime);
            let (refresh_sender, mut refresh_receiver) = mpsc::unbounded_channel::<String>();
            runtime.install_catalog_refresh(refresh_sender);
            let task_runtime = Arc::clone(&runtime);
            let task_catalogs = Arc::clone(&settings_catalogs);
            let task_endpoint = endpoint.clone();
            catalog_tasks.push(tokio::spawn(async move {
                while let Some(session_id) = refresh_receiver.recv().await {
                    let Some(catalog) = task_runtime.settings_catalog(&session_id).await else {
                        continue;
                    };
                    let Ok(session_id) = SessionId::try_from(session_id) else {
                        continue;
                    };
                    let target = SessionRef {
                        endpoint: task_endpoint.clone(),
                        session_id,
                    };
                    task_catalogs.lock().await.insert(target, catalog.clone());
                }
            }));
            runtimes.insert(
                endpoint,
                RuntimeBinding {
                    identity: binding.identity,
                    runtime,
                },
            );
        }
        Ok(Self {
            inner: Arc::new(SupervisorInner {
                bindings: runtimes,
                store,
                live_operations: StdMutex::new(LiveOperations::default()),
                turn_approvers: StdMutex::new(HashMap::new()),
                message_fifo: std::sync::OnceLock::new(),
                completions: tokio::sync::watch::channel(0).0,
                queued_operations: ProviderQueueOperationRegistry::default(),
                hub,
                scratch_root,
                history_unavailable: Mutex::new(HashSet::new()),
                settings_catalogs,
                requested_modes: Mutex::new(HashMap::new()),
                catalog_tasks: Mutex::new(catalog_tasks),
                started_at_ms: now_ms(),
                #[cfg(test)]
                admission_test_pause: StdMutex::new(None),
                #[cfg(test)]
                dispatch_test_pause: StdMutex::new(None),
                #[cfg(test)]
                observation_test_pause: StdMutex::new(None),
            }),
        })
    }

    /// Tells `observer` of every live event of every session on `endpoint`.
    pub(crate) fn install_live_event_observer(
        &self,
        endpoint: &EndpointRef,
        observer: Arc<dyn crate::provider_session_event_sink::LiveSessionEventObserver>,
    ) -> bool {
        self.inner.bindings.get(endpoint).is_some_and(|binding| {
            binding.runtime.install_live_event_observer(observer);
            true
        })
    }

    pub async fn install_approval_broker(
        &self,
        broker: Arc<collaboration_runtime::ServiceInteractionBroker>,
    ) {
        for binding in self.inner.bindings.values() {
            binding
                .runtime
                .install_approval_broker(Arc::clone(&broker))
                .await;
        }
    }

    #[expect(
        clippy::result_large_err,
        reason = "The typed failure retains the full published operation payload."
    )]
    fn runtime_binding(
        &self,
        endpoint: &EndpointRef,
        operation_id: &OperationId,
        target: Option<SessionRef>,
    ) -> Result<(ProviderBindingIdentity, Arc<ExternalProviderRuntime>), ConversationOperationFailure>
    {
        let binding = self.inner.bindings.get(endpoint).ok_or_else(|| {
            failure(
                ConversationOperationFailureKind::NotFound,
                ConversationOperationFailureStage::Binding,
                ProviderOperationEffect::None,
                "external provider binding was not found",
                operation_id.clone(),
                target.clone(),
            )
        })?;
        if binding.runtime.retirement().is_cancelled() {
            return Err(failure(
                ConversationOperationFailureKind::Unavailable,
                ConversationOperationFailureStage::Binding,
                ProviderOperationEffect::None,
                "external provider binding is retired",
                operation_id.clone(),
                target,
            ));
        }
        Ok((binding.identity.clone(), Arc::clone(&binding.runtime)))
    }

    #[expect(
        clippy::result_large_err,
        reason = "The typed failure retains the full published operation payload."
    )]
    async fn prepare_operation(
        &self,
        operation_id: OperationId,
        operation_kind: ProviderOperationKind,
        binding: ProviderBindingIdentity,
        known_target: Option<&SessionRef>,
    ) -> Result<PreparedOperation, ConversationOperationFailure> {
        self.admit_operation(
            operation_id,
            operation_kind,
            binding,
            known_target,
            provider_operation_admission::DispatchMark::AtAdmission,
        )
        .await
    }

    fn spawn_operation<F>(&self, operation_id: OperationId, live: Arc<LiveOperation>, operation: F)
    where
        F: std::future::Future<Output = ProviderOperationCompletion> + Send + 'static,
    {
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            let completion = operation.await;
            inner.finish(operation_id, live, completion).await;
        });
    }

    #[expect(
        clippy::result_large_err,
        reason = "The typed failure retains the full published operation payload."
    )]
    async fn inspect_record(
        &self,
        operation_id: &OperationId,
    ) -> Result<ProviderOperationRecord, ConversationOperationFailure> {
        self.inner
            .store
            .lock()
            .await
            .inspect(operation_id)
            .await
            .map_err(|_| unavailable_failure(operation_id.clone(), None))?
            .ok_or_else(|| {
                failure(
                    ConversationOperationFailureKind::NotFound,
                    ConversationOperationFailureStage::Settlement,
                    ProviderOperationEffect::None,
                    "the Router has no record of this operation ID",
                    operation_id.clone(),
                    None,
                )
            })
    }

    pub async fn shutdown(&self) -> Result<(), &'static str> {
        for binding in self.inner.bindings.values() {
            binding.runtime.retirement().cancel();
        }
        for binding in self.inner.bindings.values() {
            tokio::time::timeout(Duration::from_secs(10), binding.runtime.shutdown())
                .await
                .map_err(|_| "external provider runtime shutdown timed out")?;
            if binding.runtime.shutdown_failed() {
                return Err("external provider runtime owner failed to join");
            }
        }
        for task in self.inner.catalog_tasks.lock().await.drain(..) {
            tokio::time::timeout(Duration::from_secs(10), task)
                .await
                .map_err(|_| "provider model catalog task drain timed out")?
                .map_err(|_| "provider model catalog task failed")?;
        }
        let operations = self
            .inner
            .live_operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .operations
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for operation in operations {
            tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    let changed = operation.changed.notified();
                    if operation.result.lock().await.is_some() {
                        break;
                    }
                    changed.await;
                }
            })
            .await
            .map_err(|_| "external provider operation drain timed out")?;
        }
        Ok(())
    }
}

enum PreparedOperation {
    Existing(ConversationOperationSnapshot),
    Admitted {
        snapshot: ConversationOperationSnapshot,
        live: Arc<LiveOperation>,
    },
}

impl SupervisorInner {
    async fn record_settings_catalog(
        &self,
        target: SessionRef,
        catalog: acp_client_runtime::ProviderSettingsCatalog,
    ) {
        self.settings_catalogs.lock().await.insert(target, catalog);
    }

    async fn record_requested_mode(&self, target: SessionRef, record: RequestedModeRecord) {
        self.requested_modes.lock().await.insert(target, record);
    }

    async fn finish(
        &self,
        operation_id: OperationId,
        live: Arc<LiveOperation>,
        completion: ProviderOperationCompletion,
    ) {
        self.turn_approvers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&operation_id);
        let result = match completion {
            ProviderOperationCompletion::Success {
                settlement,
                target,
                session_record,
            } => {
                let terminal_stop_reason = match &settlement {
                    // The durable summary column is an old closed string enum.
                    // Keep a future value in the live settlement without writing
                    // an unreadable value into rollback-visible storage.
                    ConversationOperationSettlement::PromptCompleted {
                        stop_reason: ProviderPromptStopReason::Unknown(_),
                        ..
                    } => None,
                    ConversationOperationSettlement::PromptCompleted { stop_reason, .. } => {
                        Some(stop_reason.clone())
                    }
                    _ => None,
                };
                let mut store = self.store.lock().await;
                if crate::provider_operation_settlement::persist_provider_success(
                    &mut store,
                    &operation_id,
                    target.as_ref(),
                    session_record.as_deref(),
                    terminal_stop_reason,
                )
                .await
                .is_err()
                {
                    Err(applied_storage_failure(operation_id.clone(), target))
                } else {
                    Ok(settlement)
                }
            }
            ProviderOperationCompletion::Failure(operation_failure) => {
                let reconciliation = if operation_failure.effect == ProviderOperationEffect::Unknown
                {
                    ProviderReconciliationState::Unresolved
                } else {
                    ProviderReconciliationState::Confirmed
                };
                let mut store = self.store.lock().await;
                let target = operation_failure.target.clone();
                let target_record_failed =
                    if operation_failure.effect == ProviderOperationEffect::Applied {
                        match target.as_ref() {
                            Some(target) => store
                                .record_target(&operation_id, target, now_ms())
                                .await
                                .is_err(),
                            None => false,
                        }
                    } else {
                        false
                    };
                if target_record_failed {
                    Err(applied_storage_failure(operation_id.clone(), target))
                } else {
                    let _ = store
                        .record_terminal(
                            &operation_id,
                            operation_failure.effect,
                            reconciliation,
                            None,
                            now_ms(),
                        )
                        .await;
                    Err(operation_failure)
                }
            }
            ProviderOperationCompletion::FailureWithSession {
                failure,
                session_record,
            } => {
                let mut store = self.store.lock().await;
                if store
                    .settle_session_operation(&operation_id, &session_record)
                    .await
                    .is_err()
                {
                    Err(applied_storage_failure(
                        operation_id.clone(),
                        Some(session_record.target.clone()),
                    ))
                } else {
                    Err(failure)
                }
            }
        };
        self.publish_result(operation_id, &live, result).await;
    }

    /// Publishes a live operation's result. Its protection from retention ends here, with
    /// no store write, so the completion signal moves only after the result is set.
    async fn publish_result(
        &self,
        operation_id: OperationId,
        live: &LiveOperation,
        result: Result<ConversationOperationSettlement, ConversationOperationFailure>,
    ) {
        *live.result.lock().await = Some(result);
        live.published.store(true, Ordering::Release);
        live.changed.notify_waiters();
        self.completions
            .send_modify(|revision| *revision = revision.wrapping_add(1));
        let mut operations = self
            .live_operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        operations.terminal_order.push_back(operation_id);
        while operations.terminal_order.len() > MAX_RETAINED_SETTLEMENTS {
            if let Some(expired) = operations.terminal_order.pop_front() {
                operations.operations.remove(&expired);
            }
        }
    }
}

#[expect(
    clippy::result_large_err,
    reason = "The typed failure retains the full published operation payload."
)]
fn retain_operation<T, F>(
    operation_id: OperationId,
    target: Option<SessionRef>,
    operation: F,
) -> ProviderConversationFuture<'static, T>
where
    T: Send + 'static,
    F: std::future::Future<Output = Result<T, ConversationOperationFailure>> + Send + 'static,
{
    let (reply, result) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let _ = reply.send(operation.await);
    });
    Box::pin(async move {
        result.await.unwrap_or_else(|_| {
            Err(failure(
                ConversationOperationFailureKind::Unavailable,
                ConversationOperationFailureStage::Settlement,
                ProviderOperationEffect::Unknown,
                "provider operation owner stopped before returning admission",
                operation_id,
                target,
            ))
        })
    })
}

fn existing_submission(
    operation: ConversationOperationSnapshot,
) -> ConversationOperationSubmission {
    ConversationOperationSubmission {
        admission: ConversationAdmissionState::Existing,
        operation,
    }
}

fn admitted_submission(
    operation: ConversationOperationSnapshot,
) -> ConversationOperationSubmission {
    ConversationOperationSubmission {
        admission: ConversationAdmissionState::Admitted,
        operation,
    }
}

fn unavailable_failure(
    operation_id: OperationId,
    target: Option<SessionRef>,
) -> ConversationOperationFailure {
    failure(
        ConversationOperationFailureKind::Unavailable,
        ConversationOperationFailureStage::Settlement,
        ProviderOperationEffect::None,
        "provider operation metadata is unavailable",
        operation_id,
        target,
    )
}

fn applied_storage_failure(
    operation_id: OperationId,
    target: Option<SessionRef>,
) -> ConversationOperationFailure {
    failure(
        ConversationOperationFailureKind::Unavailable,
        ConversationOperationFailureStage::Settlement,
        ProviderOperationEffect::Applied,
        "provider settlement could not be persisted after the operation applied",
        operation_id,
        target,
    )
}

fn snapshot_failure(
    operation_id: OperationId,
    message: &'static str,
) -> ConversationOperationFailure {
    failure(
        ConversationOperationFailureKind::ProtocolViolation,
        ConversationOperationFailureStage::Settlement,
        ProviderOperationEffect::Unknown,
        message,
        operation_id,
        None,
    )
}

#[expect(
    clippy::expect_used,
    reason = "A malformed fixed fixture must fail its test."
)]
fn failure(
    kind: ConversationOperationFailureKind,
    stage: ConversationOperationFailureStage,
    effect: ProviderOperationEffect,
    message: &str,
    operation_id: OperationId,
    target: Option<SessionRef>,
) -> ConversationOperationFailure {
    ConversationOperationFailure {
        kind,
        stage,
        effect,
        message: NonEmptyText::try_from(message.to_owned())
            .expect("provider failure message is valid"),
        operation_id: Some(operation_id),
        invalid_setting: None,
        provider_code: None,
        target,
        endpoint: None,
        availability: None,
    }
}

fn failure_with_provider_code(
    kind: ConversationOperationFailureKind,
    stage: ConversationOperationFailureStage,
    effect: ProviderOperationEffect,
    message: &'static str,
    operation_id: OperationId,
    target: Option<SessionRef>,
    provider_code: Option<i64>,
) -> ConversationOperationFailure {
    let mut operation_failure = failure(kind, stage, effect, message, operation_id, target);
    operation_failure.provider_code = provider_code;
    operation_failure
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

mod provider_delivery_submission;
#[cfg(test)]
#[path = "external_provider_supervisor/supervisor_test.rs"]
mod tests;
pub(crate) use provider_delivery_submission::ProviderPromptDispatch;
