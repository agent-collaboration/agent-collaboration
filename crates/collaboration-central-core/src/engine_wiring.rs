//! Wires the engine to WorkspaceRuntime (P3, module direction). The engine names
//! only `EngineHost`; this module, outside engine/, implements that port for the
//! runtime and keeps the runtime's public event entry.
use crate::engine::{
    AlarmNotPublished, DueBatchState, EventCompletion,
    legacy_units::LegacyUnitOutcome,
    port::{Engine, EngineHost},
};
use crate::{CentralObservation, ObjectHost, WorkspaceRuntime, WorkspaceStoreCalls};
use central_engine_model::EngineTime;
use central_storage::{GuardedStatement, StoreFailure};

impl<H: ObjectHost, S: WorkspaceStoreCalls> EngineHost for WorkspaceRuntime<H, S> {
    type Host = H;
    fn host(&self) -> &H {
        self.host.as_ref()
    }
    fn now(&self) -> Result<EngineTime, StoreFailure> {
        WorkspaceRuntime::now(self)
    }
    fn ids(&self) -> &dyn runtime_primitives::IdSource {
        self.ids.as_ref()
    }
    fn due_batches(&self) -> &DueBatchState {
        self.due_batches.as_ref()
    }
    fn observe(&self, observation: CentralObservation) {
        (self.signin_observer)(observation);
    }
    async fn organization_is_provisioned(&self) -> Result<bool, StoreFailure> {
        self.alarm_organization_is_provisioned().await
    }
    async fn plan_due_windows(
        &self,
        now: EngineTime,
    ) -> Result<(Vec<GuardedStatement>, usize), LegacyUnitOutcome> {
        WorkspaceRuntime::plan_due_windows(self, now).await
    }
    async fn drain_due_workspaces(&self, now: EngineTime) -> Result<usize, LegacyUnitOutcome> {
        WorkspaceRuntime::drain_due_workspaces(self, now).await
    }
    fn try_central_turn(&self) -> Option<tokio::sync::MutexGuard<'_, ()>> {
        self.central_turn.try_lock().ok()
    }
    #[cfg(feature = "test-support")]
    fn pauses(&self) -> &crate::PausePoints {
        &self.pauses
    }
    #[cfg(feature = "test-support")]
    fn count_legacy_invocation(&self, count: &dyn Fn(&mut crate::LegacyUnitInvocations)) {
        WorkspaceRuntime::count_legacy_invocation(self, count);
    }
}

impl<H: ObjectHost, S: WorkspaceStoreCalls> WorkspaceRuntime<H, S> {
    /// Every event reaches this after its own work: the due batch when the
    /// spacing allows, then the alarm (P4).
    pub async fn after_event(&self) -> Result<EventCompletion, AlarmNotPublished> {
        Engine::new(self).after_event().await
    }
}
