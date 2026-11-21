//! Host-owned admission and settlement for external ACP provider operations.

mod provider_admission_policy;
mod provider_operation_admission;
mod provider_operation_index;
#[cfg(test)]
pub(crate) use provider_operation_index::HeldResult;
use provider_operation_index::{LiveOperation, LiveOperations, UncertaintyCells};
mod provider_operation_failure;
mod provider_operation_finish;
mod provider_operation_handoff;
#[cfg(test)]
#[path = "external_provider_supervisor/provider_operation_property_test.rs"]
mod provider_operation_properties;
mod provider_owner_delivery;
mod provider_owner_operations;
mod provider_owner_prompt;
pub(crate) use provider_owner_prompt::{SessionQueuePlan, WarmRun};
mod provider_prompt_contents;
pub use provider_prompt_contents::{PromptBusyPolicy, ProviderPromptContentsRequest};
mod provider_conversation_backend;
mod provider_full_access_guard;
mod provider_session_inspection;
mod provider_session_lifecycle;
mod provider_session_scratch;
mod provider_settings_control;
#[cfg(test)]
pub(crate) use provider_settings_control::snapshot_settings;
#[cfg(any(test, feature = "test-support"))]
mod supervisor_named_points;
#[cfg(test)]
mod supervisor_test_pauses;
use provider_operation_failure::{
    admission_failure, invalid_setting_failure, prompt_runtime_failure, runtime_failure,
};

use crate::provider_operation_custody::CustodyEnd;
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
    ProviderOperationStage, ProviderReconciliationState, ProviderSessionInspectRequest,
    ProviderSettingsAcceptRequest, ProviderSettingsSetRequest, SessionId, SessionRef,
};
use collaboration_runtime::ProviderOperationAdmission;
use collaboration_runtime::ProviderOperationAdmissionResult;
use collaboration_runtime::ProviderOperationRecord;
use collaboration_runtime::ProviderOperationStore;
use collaboration_service::conversation_operation_snapshot as snapshot_from_record;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};
use tokio::sync::Mutex;
use tokio_util::task::TaskTracker;

const MAX_RETAINED_SETTLEMENTS: usize = 256;
const OPERATION_DRAIN_BUDGET: Duration = Duration::from_secs(10);

pub struct ExternalProviderBinding {
    pub identity: ProviderBindingIdentity,
    pub runtime: ExternalProviderRuntime,
}

#[derive(Clone)]
pub struct ExternalProviderSupervisor {
    inner: Arc<SupervisorInner>,
}

struct SupervisorInner {
    bindings: HashMap<EndpointRef, RuntimeBinding>,
    store: Arc<Mutex<ProviderOperationStore>>,
    clock: Arc<dyn router_clock::RuntimeClock>,
    live_operations: Arc<StdMutex<LiveOperations>>,
    operation_tasks: TaskTracker,
    operation_cleanup_cancel: tokio_util::sync::CancellationToken,
    /// Each binding's unknown outcomes: the one place admission asks
    /// whether an earlier send's outcome is unknown (design note §14.1).
    uncertainty: UncertaintyCells,
    /// Wakes schedulers on publication and retention when custody ends.
    completions: tokio::sync::watch::Sender<u64>,
    queued_operations: ProviderQueueOperationRegistry,
    /// `<router root>/scratch`: each Claude or Cursor session created here gets
    /// its own `session-<uuidv7>` folder in it (R4). `None` grants no folder.
    scratch_root: Option<PathBuf>,
    settings_catalogs:
        Arc<Mutex<HashMap<SessionRef, provider_session_machines::ProviderSettingsCatalog>>>,
    started_at_ms: i64,
    #[cfg(test)]
    admission_test_pause: StdMutex<Option<AdmissionTestPause>>,
    #[cfg(test)]
    admission_before_reservation_test_pause: StdMutex<Option<AdmissionTestPause>>,
    #[cfg(test)]
    reconcile_test_pause: StdMutex<
        Option<(
            supervisor_test_pauses::ReconcilePausePoint,
            AdmissionTestPause,
        )>,
    >,
    #[cfg(test)]
    dispatch_mark_test_pause: StdMutex<Option<AdmissionTestPause>>,
    #[cfg(test)]
    cancel_admission_test_pause: StdMutex<Option<AdmissionTestPause>>,
    /// Pauses the next steer after its injection reply, before it reads the
    /// joined turn's approver.
    #[cfg(test)]
    steer_reply_test_pause: StdMutex<Option<AdmissionTestPause>>,
    /// Pauses the next steer with its approver records pinned, before its
    /// injection is sent.
    #[cfg(test)]
    steer_injection_test_pause: StdMutex<Option<AdmissionTestPause>>,
    /// Pauses one operation after its result is visible and waiters are notified.
    #[cfg(test)]
    result_published_test_pause: StdMutex<Option<(OperationId, AdmissionTestPause)>>,
}

#[cfg(test)]
struct AdmissionTestPause {
    entered: tokio::sync::oneshot::Sender<()>,
    release: tokio::sync::oneshot::Receiver<()>,
}

struct RuntimeBinding {
    identity: ProviderBindingIdentity,
    runtime: Arc<ExternalProviderRuntime>,
    owners: Arc<crate::session_owner_door::SessionOwnerDoor>,
}

impl ExternalProviderSupervisor {
    pub(crate) fn queued_operation_registry(&self) -> &ProviderQueueOperationRegistry {
        &self.inner.queued_operations
    }

    /// Sets the work-roots rule sessions on `endpoint` start with (D1 R18);
    /// the binding's runtime keeps it, so every path that restores one of its
    /// sessions rebuilds the same roots.
    pub(crate) fn set_work_roots(
        &self,
        endpoint: &EndpointRef,
        work_roots: crate::provider_work_roots::ProviderWorkRoots,
    ) {
        if let Some(runtime) = self.runtime_for(endpoint) {
            runtime.session_setup().set_work_roots(work_roots);
        }
    }

    fn work_roots_for(
        &self,
        endpoint: &EndpointRef,
    ) -> crate::provider_work_roots::ProviderWorkRoots {
        self.runtime_for(endpoint)
            .map(|runtime| runtime.session_setup().work_roots())
            .unwrap_or_default()
    }

    /// The provider a binding runs, for checks made before anything reaches it.
    fn provider_kind_for(
        &self,
        endpoint: &EndpointRef,
    ) -> Option<collaboration_protocol::ProviderKind> {
        self.inner
            .bindings
            .get(endpoint)
            .map(|entry| entry.identity.runtime.provider)
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

    pub(crate) fn session_owner_for(
        &self,
        target: &SessionRef,
    ) -> Result<
        Arc<crate::session_owner_door::SessionOwnerAddress>,
        crate::session_owner_door::OwnerDoorError,
    > {
        let binding = self
            .inner
            .bindings
            .get(&target.endpoint)
            .ok_or(crate::session_owner_door::OwnerDoorError::Unavailable)?;
        binding
            .owners
            .open(target.clone(), binding.runtime.session_setup().work_roots())
    }

    fn owner_door(
        &self,
        endpoint: &EndpointRef,
    ) -> Option<Arc<crate::session_owner_door::SessionOwnerDoor>> {
        self.inner
            .bindings
            .get(endpoint)
            .map(|binding| binding.owners.clone())
    }

    pub(crate) fn owner_session_snapshot(
        &self,
        target: &SessionRef,
    ) -> Option<provider_session_machines::session_owner::SessionSnapshot> {
        let owner = self
            .inner
            .bindings
            .get(&target.endpoint)?
            .owners
            .existing(target)?;
        let snapshot = owner.snapshots().borrow().clone();
        Some(snapshot)
    }

    pub(crate) fn owner_session_catalog(
        &self,
        target: &SessionRef,
    ) -> Option<provider_session_machines::ProviderSettingsCatalog> {
        self.inner
            .bindings
            .get(&target.endpoint)?
            .owners
            .existing(target)?
            .catalog()
    }

    pub(crate) fn owner_session_activity(
        &self,
        target: &SessionRef,
    ) -> crate::ProviderSessionActivity {
        use crate::ProviderSessionActivity;
        use provider_session_machines::{
            session_lifecycle_machine::SessionLifecycle, session_turn_machine::SessionTurn,
        };
        match self.owner_session_snapshot(target) {
            Some(snapshot) if matches!(snapshot.lifecycle, SessionLifecycle::Live { .. }) => {
                match snapshot.turn {
                    SessionTurn::Idle => ProviderSessionActivity::Idle,
                    _ => ProviderSessionActivity::Running,
                }
            }
            _ => ProviderSessionActivity::NotLoaded,
        }
    }

    pub(crate) async fn cancel_waiting_owner_inputs(&self) {
        for binding in self.inner.bindings.values() {
            binding.owners.cancel_waiting_inputs().await;
        }
    }

    /// Records `operation_id` as an unknown outcome on its binding's
    /// uncertainty cell, as its live owner or the start-up seed would.
    #[cfg(test)]
    pub(crate) async fn record_unknown_for_test(
        &self,
        operation_id: &OperationId,
        binding: &ProviderBindingIdentity,
        target: Option<SessionRef>,
    ) {
        if let Ok(cell) = self.inner.binding_uncertainty(binding).await
            && let Some(record) = cell.outcomes.get()
        {
            crate::binding_dispatch_permit::lock(record).record_unknown(
                operation_id.clone(),
                target,
                crate::binding_dispatch_permit::UnknownRecovery::Blocks,
            );
        }
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
        live.finish_write(CustodyEnd::NoWrite);
        self.inner
            .publish_result(
                operation_id.clone(),
                &live,
                result,
                provider_operation_finish::Retention::Settled,
            )
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
            .custodians()
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
        let clock = bindings.first().map_or_else(
            || {
                Arc::new(router_clock::SystemRuntimeClock::new())
                    as Arc<dyn router_clock::RuntimeClock>
            },
            |binding| binding.runtime.owner_clock(),
        );
        let started_at_ms = clock.utc_now().timestamp_millis();
        let queued_operations = ProviderQueueOperationRegistry::new(clock.clone());
        let owner_hub = hub.unwrap_or_else(|| {
            Arc::new(collaboration_runtime::ProviderSessionEventHub::new(
                Arc::clone(&store),
            ))
        });
        let mut runtimes = HashMap::with_capacity(bindings.len());
        let live_operations = Arc::new(StdMutex::new(LiveOperations::default()));
        let completions = tokio::sync::watch::channel(0_u64).0;
        let on_custody_end: Arc<dyn Fn() + Send + Sync> = {
            let operations = Arc::downgrade(&live_operations);
            let completions = completions.clone();
            Arc::new(move || {
                if let Some(operations) = operations.upgrade() {
                    operations
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .trim_results();
                    completions.send_modify(|revision| *revision = revision.wrapping_add(1));
                }
            })
        };
        let uncertainty = UncertaintyCells::default();
        let settings_catalogs = Arc::new(Mutex::new(HashMap::new()));
        for binding in bindings {
            let endpoint = binding.identity.endpoint.clone();
            if runtimes.contains_key(&endpoint) {
                return Err("external provider endpoint binding must be unique");
            }
            let runtime = Arc::new(binding.runtime);
            let owners = runtime.session_owner_door(Arc::clone(&store), Arc::clone(&owner_hub));
            let permit = uncertainty.slot(&endpoint);
            let operations = Arc::downgrade(&live_operations);
            let repair_operations = operations.clone();
            let on_custody_end = Arc::clone(&on_custody_end);
            let generation = binding.identity.generation.clone();
            let repair_generation = generation.clone();
            permit.lane_custody.get_or_init(|| {
                Arc::new(move |target, operation| {
                    operations
                        .upgrade()?
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .begin_lane_write(
                            operation,
                            &generation,
                            target,
                            Arc::clone(&on_custody_end),
                        )
                })
            });
            permit.late_prompt_repair.get_or_init(|| {
                Arc::new(move |target, operation, identity| {
                    repair_operations
                        .upgrade()?
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .operations
                        .get(operation)?
                        .authorize_late_prompt_repair(&repair_generation, target, identity)
                })
            });
            owners.install_binding_permit(permit);
            runtimes.insert(
                endpoint,
                RuntimeBinding {
                    identity: binding.identity,
                    runtime,
                    owners,
                },
            );
        }
        Ok(Self {
            inner: Arc::new(SupervisorInner {
                bindings: runtimes,
                store,
                clock,
                live_operations,
                operation_tasks: TaskTracker::new(),
                operation_cleanup_cancel: tokio_util::sync::CancellationToken::new(),
                uncertainty,
                completions,
                queued_operations,
                scratch_root,
                settings_catalogs,
                started_at_ms,
                #[cfg(test)]
                admission_test_pause: StdMutex::new(None),
                #[cfg(test)]
                admission_before_reservation_test_pause: StdMutex::new(None),
                #[cfg(test)]
                reconcile_test_pause: StdMutex::new(None),
                #[cfg(test)]
                dispatch_mark_test_pause: StdMutex::new(None),
                #[cfg(test)]
                cancel_admission_test_pause: StdMutex::new(None),
                #[cfg(test)]
                steer_reply_test_pause: StdMutex::new(None),
                #[cfg(test)]
                steer_injection_test_pause: StdMutex::new(None),
                #[cfg(test)]
                result_published_test_pause: StdMutex::new(None),
            }),
        })
    }

    /// Tells `observer` of every live event of every session on `endpoint`.
    pub(crate) fn install_owner_snapshot_observer(
        &self,
        endpoint: &EndpointRef,
        observer: Arc<dyn crate::session_owner_door::SessionOwnerSnapshotObserver>,
    ) -> bool {
        self.inner.bindings.get(endpoint).is_some_and(|binding| {
            binding.owners.install_snapshot_observer(observer);
            true
        })
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
        deadline: router_clock::ClockInstant,
        operation_id: OperationId,
        operation_kind: ProviderOperationKind,
        binding: ProviderBindingIdentity,
        known_target: Option<&SessionRef>,
    ) -> Result<PreparedOperation, ConversationOperationFailure> {
        self.admit_operation(
            deadline,
            operation_id,
            operation_kind,
            binding,
            known_target,
            provider_admission_policy::DispatchMark::BeforeHandoff,
        )
        .await
    }

    fn spawn_operation<F>(&self, operation_id: OperationId, live: Arc<LiveOperation>, operation: F)
    where
        F: std::future::Future<Output = ProviderOperationCompletion> + Send + 'static,
    {
        let inner = Arc::clone(&self.inner);
        self.inner.operation_tasks.spawn(async move {
            let completion = operation.await;
            inner.finish(operation_id, live, completion).await;
        });
    }

    fn spawn_operation_for_handoff<F>(
        &self,
        operation_id: OperationId,
        live: Arc<LiveOperation>,
        operation: F,
    ) where
        F: std::future::Future<Output = ProviderOperationCompletion> + Send + 'static,
    {
        let supervisor = self.clone();
        self.inner.operation_tasks.spawn(async move {
            supervisor
                .run_operation_for_handoff(operation_id, live, operation)
                .await;
        });
    }

    /// Like `spawn_operation` for a prompt: its session's turn ends only
    /// once the operation's settlement is recorded, so the session's line
    /// commits nothing that would see it unresolved (design note §13.1).
    #[expect(
        clippy::result_large_err,
        reason = "Keeps the existing typed operation failure returned by the command port."
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

    pub(crate) async fn retained_restore_settlement(
        &self,
        operation_id: &OperationId,
    ) -> Option<Result<ConversationOperationSettlement, ConversationOperationFailure>> {
        let live = self
            .inner
            .live_operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .operations
            .get(operation_id)
            .cloned()?;
        live.wait_for_result().await
    }

    pub async fn shutdown(&self) -> Result<(), &'static str> {
        let _cancel_operation_cleanup = self.inner.operation_cleanup_cancel.clone().drop_guard();
        for binding in self.inner.bindings.values() {
            binding.runtime.retirement().cancel();
        }
        for binding in self.inner.bindings.values() {
            router_clock::timeout(
                self.inner.clock.as_ref(),
                Duration::from_secs(10),
                binding.runtime.shutdown(),
            )
            .await
            .map_err(|_| "external provider runtime shutdown timed out")?;
            if binding.runtime.shutdown_failed() {
                return Err("external provider runtime owner failed to join");
            }
        }
        let drain_deadline = self
            .inner
            .clock
            .monotonic_now()
            .after(OPERATION_DRAIN_BUDGET);
        let operations = self
            .inner
            .live_operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .custodians()
            .map(|(_, operation)| Arc::clone(operation))
            .collect::<Vec<_>>();
        self.inner.operation_tasks.close();
        router_clock::timeout_at(self.inner.clock.as_ref(), drain_deadline, async {
            for operation in operations {
                loop {
                    let changed = operation.changed.notified();
                    if !operation.holds_custody() {
                        break;
                    }
                    changed.await;
                }
            }
            // A terminal receipt can precede the tracked task's final release.
            self.inner.operation_tasks.wait().await;
        })
        .await
        .map_err(|_| "external provider operation drain timed out")
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
    fn now_ms(&self) -> i64 {
        self.clock.utc_now().timestamp_millis()
    }

    fn finish_late_write(&self, live: &LiveOperation, reason: CustodyEnd) {
        if live.finish_write(reason) {
            self.live_operations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .trim_results();
            self.completions
                .send_modify(|revision| *revision = revision.wrapping_add(1));
        }
    }

    async fn record_settings_catalog(
        &self,
        target: SessionRef,
        catalog: provider_session_machines::ProviderSettingsCatalog,
    ) {
        self.settings_catalogs.lock().await.insert(target, catalog);
    }

    /// Registers the settlement for retention before making it visible to waiters.
    async fn publish_result(
        &self,
        operation_id: OperationId,
        live: &LiveOperation,
        result: Result<ConversationOperationSettlement, ConversationOperationFailure>,
        retention: provider_operation_finish::Retention,
    ) {
        #[cfg(test)]
        let publication_pause = self
            .result_published_test_pause
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take_if(|(selected, _)| *selected == operation_id)
            .map(|(_, pause)| pause);
        // Waiters can read the result slot directly before a wake, so the
        // slot stays locked until the retention index and result agree.
        let mut result_slot = live.result.lock().await;
        {
            let mut operations = self
                .live_operations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            operations.retain_result(operation_id, retention);
            *result_slot = Some(result);
        }
        drop(result_slot);
        live.changed.notify_waiters();
        self.completions
            .send_modify(|revision| *revision = revision.wrapping_add(1));
        #[cfg(test)]
        if let Some(pause) = publication_pause {
            let _entered = pause.entered.send(());
            let _released = pause.release.await;
        }
    }
}

impl ExternalProviderSupervisor {
    #[expect(
        clippy::result_large_err,
        reason = "The typed failure retains the full published operation payload."
    )]
    fn retain_operation<T, F>(
        &self,
        operation_id: OperationId,
        target: Option<SessionRef>,
        operation: F,
    ) -> ProviderConversationFuture<'static, T>
    where
        T: Send + 'static,
        F: std::future::Future<Output = Result<T, ConversationOperationFailure>> + Send + 'static,
    {
        let (reply, result) = tokio::sync::oneshot::channel();
        self.inner.operation_tasks.spawn(async move {
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

pub(crate) fn operation_store_deadline_failed(
    operation_id: OperationId,
    target: Option<SessionRef>,
    stage: ConversationOperationFailureStage,
    effect: ProviderOperationEffect,
) -> ConversationOperationFailure {
    failure(
        ConversationOperationFailureKind::Unavailable,
        stage,
        effect,
        "the store didn't respond in time",
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

#[cfg(test)]
mod provider_delivery_submission;
#[cfg(test)]
#[path = "external_provider_supervisor/supervisor_test.rs"]
mod tests;
#[cfg(test)]
pub(crate) use provider_delivery_submission::ProviderPromptDispatch;
