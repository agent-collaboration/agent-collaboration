//! Composes the retained engine with the runtime's execution capabilities.
use crate::engine::{
    legacy_units::LegacyUnitOutcome,
    port::{Engine, EngineHost},
};
use crate::{MailDeliveryReady, WorkspaceRuntime};
use central_engine_model::EngineTime;
use central_storage::{GuardedStatement, StoreFailure};
use fleet_contracts::{
    AlarmNotPublished, CentralObservation, EventCompletion, EventCompletionHook,
};
use fleet_runtime::{DueBatchState, TurnGuard};
use fleet_runtime_ports::{CentralStore, WorkspaceStores};

impl<H: CentralStore, S: WorkspaceStores> EngineHost for WorkspaceRuntime<H, S> {
    type Host = H;
    fn host(&self) -> &H {
        self.execution.statement_host()
    }
    fn now(&self) -> Result<EngineTime, StoreFailure> {
        self.execution.now()
    }
    fn ids(&self) -> &dyn fleet_runtime_ports::IdSource {
        self.execution.ids()
    }
    fn due_batches(&self) -> &DueBatchState {
        self.execution.due_batches()
    }
    fn observe(&self, observation: CentralObservation) {
        self.execution.observe(observation);
    }
    fn offer_mail(&self, delivery: MailDeliveryReady) {
        (self.mail_delivery_ready)(delivery);
    }
    async fn organization_is_provisioned(&self) -> Result<bool, StoreFailure> {
        self.alarm_organization_is_provisioned().await
    }
    async fn plan_due_windows(
        &self,
        now: EngineTime,
    ) -> Result<(Vec<GuardedStatement>, Vec<MailDeliveryReady>), LegacyUnitOutcome> {
        WorkspaceRuntime::plan_due_windows(self, now).await
    }
    async fn drain_due_workspaces(&self, now: EngineTime) -> Result<usize, LegacyUnitOutcome> {
        WorkspaceRuntime::drain_due_workspaces(self, now).await
    }
    fn try_central_turn(&self) -> Option<TurnGuard<'_>> {
        self.execution.try_central_turn()
    }
    #[cfg(feature = "test-support")]
    fn pauses(&self) -> &fleet_runtime::PausePoints {
        self.execution.pause_points()
    }
    #[cfg(feature = "test-support")]
    fn record_engine_completion_entry(&self) {
        self.execution.record_engine_completion_entry();
    }
    #[cfg(feature = "test-support")]
    fn count_legacy_invocation(&self, count: &dyn Fn(&mut fleet_runtime::LegacyUnitInvocations)) {
        self.execution.count_legacy_invocation(count);
    }
}

impl<R: EngineHost> EventCompletionHook for Engine<'_, R> {
    async fn complete_event(&self) -> Result<EventCompletion, AlarmNotPublished> {
        self.after_event().await
    }
}

impl<H: CentralStore, S: WorkspaceStores> WorkspaceRuntime<H, S> {
    /// Await the engine's completion after the event's own work.
    pub async fn after_event(&self) -> Result<EventCompletion, AlarmNotPublished> {
        self.execution.after_event(&Engine::new(self)).await
    }
}
