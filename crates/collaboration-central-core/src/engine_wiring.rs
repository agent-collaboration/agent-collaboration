//! Wires the engine to WorkspaceRuntime (P3, module direction). The engine names
//! only `EngineHost`; this module, outside engine/, implements that port for the
//! runtime and keeps the runtime's public event entry.
use crate::WorkspaceRuntime;
use crate::engine::port::{Engine, EngineHost};
use central_engine_model::EngineTime;
use central_storage::{GuardedStatement, StoreFailure};
use fleet_contracts::{
    AlarmNotPublished, CentralObservation, EventCompletion, EventCompletionHook, LegacyUnitOutcome,
};
use fleet_runtime::{DueBatchState, TurnGuard};
use fleet_runtime_ports::{CentralStore, WorkspaceStores};

impl<H: CentralStore, S: WorkspaceStores> EngineHost for WorkspaceRuntime<H, S> {
    type Host = H;
    fn host(&self) -> &H {
        self.statement_host()
    }
    fn now(&self) -> Result<EngineTime, StoreFailure> {
        self.fleet_runtime.now()
    }
    fn ids(&self) -> &dyn fleet_runtime_ports::IdSource {
        self.fleet_runtime.ids()
    }
    fn due_batches(&self) -> &DueBatchState {
        self.fleet_runtime.due_batches()
    }
    fn observe(&self, observation: CentralObservation) {
        self.fleet_runtime.observe(observation);
    }
    async fn organization_is_provisioned(&self) -> Result<bool, StoreFailure> {
        self.alarm_organization_is_provisioned().await
    }
    async fn plan_due_windows(
        &self,
        now: EngineTime,
    ) -> Result<(Vec<GuardedStatement>, usize), LegacyUnitOutcome> {
        fleet_contracts::DueWorkSource::plan_due_windows(&self.mail(), now).await
    }
    async fn drain_due_workspaces(&self, now: EngineTime) -> Result<usize, LegacyUnitOutcome> {
        fleet_contracts::DueWorkSource::drain_due_workspaces(&self.mail(), now).await
    }
    fn try_central_turn(&self) -> Option<TurnGuard<'_>> {
        self.fleet_runtime.try_central_turn()
    }
    #[cfg(feature = "test-support")]
    fn pauses(&self) -> &fleet_runtime::PausePoints {
        self.fleet_runtime.pause_points()
    }
    #[cfg(feature = "test-support")]
    fn count_legacy_invocation(&self, count: &dyn Fn(&mut fleet_runtime::LegacyUnitInvocations)) {
        self.fleet_runtime.count_legacy_invocation(count);
    }
}

impl<H: CentralStore, S: WorkspaceStores> WorkspaceRuntime<H, S> {
    /// Every event reaches this after its own work: the due batch when the
    /// spacing allows, then the alarm (P4).
    pub async fn after_event(&self) -> Result<EventCompletion, AlarmNotPublished> {
        self.fleet_runtime.after_event(self).await
    }
}

impl<H: CentralStore, S: WorkspaceStores> EventCompletionHook for WorkspaceRuntime<H, S> {
    async fn complete_event(&self) -> Result<EventCompletion, AlarmNotPublished> {
        Engine::new(self).after_event().await
    }
}
