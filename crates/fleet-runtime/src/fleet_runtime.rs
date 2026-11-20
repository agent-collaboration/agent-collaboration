//! Injected capabilities and execution state for one organization.
#[cfg(feature = "test-support")]
use crate::LegacyUnitInvocations;
use crate::{AlarmSchedule, DueBatchState, WorkspaceTurns};
use central_storage::{
    BatchOutcome, ReadOutcome, Statement, StatementBatch, StoreCall, StoreFailure, StoreReply,
};
use fleet_contracts::{
    AlarmNotPublished, CentralObservation, EventCompletion, EventCompletionHook,
};
use fleet_identity_protocol::OrganizationId;
use fleet_runtime_ports::{CentralStore, Clock, IdSource, Observer, WorkspaceStores};
use std::sync::Arc;
use workspace_ids::WorkspaceId;

pub struct TurnGuard<'a> {
    _held: tokio::sync::MutexGuard<'a, ()>,
}

pub struct FleetRuntime<Host: CentralStore, Stores: WorkspaceStores> {
    organization: OrganizationId,
    host: Arc<Host>,
    stores: Stores,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdSource>,
    due_batches: Arc<DueBatchState>,
    turns: WorkspaceTurns,
    observer: Arc<dyn Observer<CentralObservation>>,
    central_turn: tokio::sync::Mutex<()>,
    #[cfg(feature = "test-support")]
    completed_alarms: tokio::sync::watch::Sender<u64>,
    #[cfg(feature = "test-support")]
    completion_attempts: std::sync::atomic::AtomicU64,
    #[cfg(feature = "test-support")]
    engine_completion_entries: std::sync::atomic::AtomicU64,
    /// Test-support record of every alarm drive that failed, in order.
    #[cfg(feature = "test-support")]
    failed_alarms: tokio::sync::watch::Sender<Vec<String>>,
    #[cfg(feature = "test-support")]
    central_waiting: tokio::sync::watch::Sender<usize>,
    /// Test-support count of machine admissions waiting on the central turn,
    /// kept apart from every other wait on it.
    #[cfg(feature = "test-support")]
    admission_waiting: tokio::sync::watch::Sender<usize>,
    #[cfg(feature = "test-support")]
    pauses: crate::PausePoints,
    #[cfg(feature = "test-support")]
    legacy_invocations: std::sync::Mutex<LegacyUnitInvocations>,
    #[cfg(feature = "test-support")]
    lost_answers: std::sync::Mutex<std::collections::BTreeSet<workspace_model::RequestId>>,
}
impl<Host: CentralStore, Stores: WorkspaceStores> FleetRuntime<Host, Stores> {
    pub fn open(
        organization: OrganizationId,
        host: Arc<Host>,
        stores: Stores,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdSource>,
        due_batches: Arc<DueBatchState>,
        observer: Arc<dyn Observer<CentralObservation>>,
    ) -> Self {
        Self {
            organization,
            host,
            stores,
            clock,
            ids,
            due_batches,
            turns: WorkspaceTurns::default(),
            central_turn: tokio::sync::Mutex::new(()),
            #[cfg(feature = "test-support")]
            completed_alarms: tokio::sync::watch::channel(0).0,
            #[cfg(feature = "test-support")]
            completion_attempts: Default::default(),
            #[cfg(feature = "test-support")]
            engine_completion_entries: Default::default(),
            #[cfg(feature = "test-support")]
            failed_alarms: tokio::sync::watch::channel(Vec::new()).0,
            #[cfg(feature = "test-support")]
            legacy_invocations: Default::default(),
            observer,
            #[cfg(feature = "test-support")]
            central_waiting: tokio::sync::watch::channel(0).0,
            #[cfg(feature = "test-support")]
            admission_waiting: tokio::sync::watch::channel(0).0,
            #[cfg(feature = "test-support")]
            pauses: crate::PausePoints::default(),
            #[cfg(feature = "test-support")]
            lost_answers: Default::default(),
        }
    }
    pub fn organization(&self) -> &OrganizationId {
        &self.organization
    }
    pub fn clock(&self) -> &dyn Clock {
        self.clock.as_ref()
    }
    pub fn ids(&self) -> &dyn IdSource {
        self.ids.as_ref()
    }
    pub fn due_batches(&self) -> &DueBatchState {
        self.due_batches.as_ref()
    }

    pub fn try_central_turn(&self) -> Option<TurnGuard<'_>> {
        self.central_turn
            .try_lock()
            .ok()
            .map(|held| TurnGuard { _held: held })
    }

    pub fn observe(&self, observation: CentralObservation) {
        self.observer.observe(observation);
    }

    pub async fn after_event(
        &self,
        hook: &impl EventCompletionHook,
    ) -> Result<EventCompletion, AlarmNotPublished> {
        #[cfg(feature = "test-support")]
        self.completion_attempts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        hook.complete_event().await
    }

    /// Counts requests and alarm callbacks that enter the borrowed completion hook.
    #[cfg(feature = "test-support")]
    pub fn completion_attempts(&self) -> u64 {
        self.completion_attempts
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Counts entries to the underlying engine, including a direct call that bypasses the hook.
    #[cfg(feature = "test-support")]
    pub fn engine_completion_entries(&self) -> u64 {
        self.engine_completion_entries
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    #[cfg(feature = "test-support")]
    pub fn record_engine_completion_entry(&self) {
        self.engine_completion_entries
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn alarm_schedule<Deadline: Copy>(&self) -> AlarmSchedule<Deadline> {
        AlarmSchedule::new()
    }

    pub fn statement_host(&self) -> &Host {
        &self.host
    }
    /// The due-batch start and backlog bit the next publication will use.
    #[cfg(feature = "test-support")]
    pub fn due_batch_snapshot(&self) -> crate::DueBatchSnapshot {
        self.due_batches.snapshot()
    }
    /// Every unit the due batches started, in start order (R2/R3 visit order).
    #[cfg(feature = "test-support")]
    pub fn due_unit_starts(&self) -> Vec<crate::DueUnitStart> {
        self.due_batches.unit_starts()
    }
    /// Test-support record of every claimed due-batch start, one per batch.
    #[cfg(feature = "test-support")]
    pub fn due_batch_starts(&self) -> Vec<i64> {
        self.due_batches.claimed_starts()
    }
    #[cfg(feature = "test-support")]
    pub fn legacy_unit_invocations(&self) -> LegacyUnitInvocations {
        *self
            .legacy_invocations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    #[cfg(feature = "test-support")]
    pub fn count_legacy_invocation(&self, count: impl FnOnce(&mut LegacyUnitInvocations)) {
        count(
            &mut self
                .legacy_invocations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
    }
    #[cfg(feature = "test-support")]
    pub fn alarm_completion_count(&self) -> u64 {
        *self.completed_alarms.borrow()
    }
    #[cfg(feature = "test-support")]
    pub fn mark_alarm_completed(&self) {
        self.completed_alarms
            .send_modify(|count| *count = count.saturating_add(1));
    }
    #[cfg(feature = "test-support")]
    pub async fn until_alarm_completed(&self, count: u64) {
        let mut completed = self.completed_alarms.subscribe();
        let _ = completed.wait_for(|current| *current >= count).await;
    }
    /// The host's alarm driver reports a failed drive (it retries at the next mark).
    #[cfg(feature = "test-support")]
    pub fn mark_alarm_failed(&self, failure: String) {
        self.failed_alarms
            .send_modify(|failures| failures.push(failure));
    }
    #[cfg(feature = "test-support")]
    pub fn alarm_failures(&self) -> Vec<String> {
        self.failed_alarms.borrow().clone()
    }
    #[cfg(feature = "test-support")]
    pub async fn until_alarm_failed(&self, count: usize) {
        let mut failed = self.failed_alarms.subscribe();
        let _ = failed.wait_for(|failures| failures.len() >= count).await;
    }
    pub async fn take_central_turn(&self) -> TurnGuard<'_> {
        let held = {
            #[cfg(not(feature = "test-support"))]
            {
                self.central_turn.lock().await
            }
            #[cfg(feature = "test-support")]
            {
                self.take_central_turn_counted(&self.central_waiting).await
            }
        };
        TurnGuard { _held: held }
    }
    /// Machine admission and restoration take the same central turn; test support
    /// counts their waits separately from the work they admit.
    pub async fn take_central_turn_for_admission(&self) -> TurnGuard<'_> {
        let held = {
            #[cfg(not(feature = "test-support"))]
            {
                self.central_turn.lock().await
            }
            #[cfg(feature = "test-support")]
            {
                self.take_central_turn_counted(&self.admission_waiting)
                    .await
            }
        };
        TurnGuard { _held: held }
    }
    #[cfg(feature = "test-support")]
    async fn take_central_turn_counted(
        &self,
        counter: &tokio::sync::watch::Sender<usize>,
    ) -> tokio::sync::MutexGuard<'_, ()> {
        {
            use std::future::Future;
            let mut acquisition = std::pin::pin!(self.central_turn.lock());
            let mut waiting = None;
            std::future::poll_fn(|context| match acquisition.as_mut().poll(context) {
                std::task::Poll::Pending => {
                    waiting.get_or_insert_with(|| CentralWaiter::enter(counter.clone()));
                    std::task::Poll::Pending
                }
                ready @ std::task::Poll::Ready(_) => ready,
            })
            .await
        }
    }
    #[cfg(feature = "test-support")]
    pub async fn until_central_waiting(&self, count: usize) {
        let mut waiting = self.central_waiting.subscribe();
        let _ = waiting.wait_for(|waiting| *waiting >= count).await;
    }
    #[cfg(feature = "test-support")]
    pub async fn until_admission_waiting(&self, count: usize) {
        let mut waiting = self.admission_waiting.subscribe();
        let _ = waiting.wait_for(|waiting| *waiting >= count).await;
    }
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn admission_waiting_now(&self) -> usize {
        *self.admission_waiting.borrow()
    }
    pub fn mint<T: TryFrom<String>>(&self) -> Result<T, StoreFailure> {
        T::try_from(self.ids.new_id().to_string())
            .map_err(|_| fleet_runtime_ports::workspace_decode_error::corrupt("minted id"))
    }
    pub fn now(&self) -> Result<central_engine_model::EngineTime, StoreFailure> {
        chrono::DateTime::from_timestamp_millis(self.clock.now().unix_millis())
            .ok_or_else(|| fleet_runtime_ports::workspace_decode_error::corrupt("clock"))
    }
    pub async fn read_workspace(
        &self,
        workspace: &WorkspaceId,
        statement: Statement,
    ) -> Result<ReadOutcome, StoreFailure> {
        match self
            .stores
            .call_workspace(workspace, StoreCall::Read(statement))
            .await?
        {
            StoreReply::Read(rows) => Ok(rows),
            StoreReply::Batch(_) => Err(fleet_runtime_ports::workspace_decode_error::corrupt(
                "store reply",
            )),
        }
    }
    pub async fn batch_workspace(
        &self,
        workspace: &WorkspaceId,
        batch: StatementBatch,
    ) -> Result<BatchOutcome, StoreFailure> {
        match self
            .stores
            .call_workspace(workspace, StoreCall::Batch(batch))
            .await?
        {
            StoreReply::Batch(outcome) => Ok(outcome),
            StoreReply::Read(_) => Err(fleet_runtime_ports::workspace_decode_error::corrupt(
                "store reply",
            )),
        }
    }
    #[cfg(feature = "test-support")]
    pub fn pause_points(&self) -> &crate::PausePoints {
        &self.pauses
    }
    pub fn workspace_turns(&self) -> &WorkspaceTurns {
        &self.turns
    }
    #[cfg(feature = "test-support")]
    pub fn lose_reply_after_apply(&self, request: workspace_model::RequestId) {
        self.lost_answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(request);
    }
    #[cfg(feature = "test-support")]
    pub fn take_lost_reply(&self, request: &workspace_model::RequestId) -> bool {
        self.lost_answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(request)
    }
    #[cfg(feature = "test-support")]
    pub async fn inspect_workspace(
        &self,
        workspace: &WorkspaceId,
        statement: Statement,
    ) -> Result<ReadOutcome, StoreFailure> {
        self.read_workspace(workspace, statement).await
    }
}

#[cfg(feature = "test-support")]
struct CentralWaiter(tokio::sync::watch::Sender<usize>);
#[cfg(feature = "test-support")]
impl CentralWaiter {
    fn enter(waiting: tokio::sync::watch::Sender<usize>) -> Self {
        waiting.send_modify(|count| *count += 1);
        Self(waiting)
    }
}
#[cfg(feature = "test-support")]
impl Drop for CentralWaiter {
    fn drop(&mut self) {
        self.0.send_modify(|count| *count -= 1);
    }
}
