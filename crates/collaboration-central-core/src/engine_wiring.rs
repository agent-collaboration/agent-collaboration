//! Composes the retained engine with the runtime's execution capabilities.
use crate::workspace_runtime::engine_admin_dispatch::{KIND_REGISTRY, finish_registered_discard};
use crate::workspace_runtime::engine_dispatch::{CoreTimedRows, WholeKind};
use crate::{WorkspaceRuntime, organization_admin_contract::EngineKindName};
use central_engine_model::EngineTime;
use central_storage::StoreFailure;
use fleet_contracts::{
    AlarmNotPublished, CentralObservation, EngineSubject, EventCompletion, LegacyUnitOutcome,
    MailDeliveryReady, ObservedControl,
};
use fleet_engine::{
    DiscardOutcome, Engine, EngineHost, KindRegistry, RegisteredDiscard, WholeUnit,
    WholeUnitOutcome,
};
use fleet_runtime::DueBatchState;
use fleet_runtime_ports::{CentralStore, WorkspaceStores};

impl<H: CentralStore, S: WorkspaceStores> EngineHost for WorkspaceRuntime<H, S> {
    type Host = H;
    type Timed = CoreTimedRows;
    type WholeKind = WholeKind;
    type AdminKind = EngineKindName;
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
    fn timed_rows(&self) -> &CoreTimedRows {
        &CoreTimedRows
    }
    fn alarm_inputs(&self) -> &'static str {
        crate::engine::query_registry::ALARM_INPUTS
    }
    fn admin_kinds(&self) -> &'static KindRegistry<'static, Self::AdminKind> {
        &KIND_REGISTRY
    }
    async fn finish_registered_discard(
        &self,
        discard: RegisteredDiscard<'_, Self>,
    ) -> Result<DiscardOutcome, StoreFailure> {
        finish_registered_discard(discard).await
    }
    fn ordered_whole_units(&self) -> &'static [WholeKind] {
        &[WholeKind::Windows, WholeKind::Drains]
    }
    fn whole_unit(&self, kind: WholeKind) -> WholeUnit {
        kind.description()
    }
    async fn run_whole_unit(
        &self,
        kind: WholeKind,
        now: EngineTime,
        subject: &EngineSubject,
        observed: &ObservedControl,
    ) -> Result<WholeUnitOutcome, LegacyUnitOutcome> {
        self.execute_whole_unit(kind, now, subject, observed).await
    }
    #[cfg(feature = "test-support")]
    fn pauses(&self) -> &fleet_runtime::PausePoints {
        self.execution.pause_points()
    }
    #[cfg(feature = "test-support")]
    fn record_engine_completion_entry(&self) {
        self.execution.record_engine_completion_entry();
    }
}

impl<H: CentralStore, S: WorkspaceStores> WorkspaceRuntime<H, S> {
    /// Await the engine's completion after the event's own work.
    pub async fn after_event(&self) -> Result<EventCompletion, AlarmNotPublished> {
        self.execution.after_event(&Engine::new(self)).await
    }
}
