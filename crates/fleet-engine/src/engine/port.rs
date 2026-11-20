//! The engine's only view of the runtime that hosts it (P3, module direction):
//! engine/ names this port and never WorkspaceRuntime, so the module graph has
//! one direction. `crate::engine_wiring` implements it for WorkspaceRuntime.
use super::alarm_source::{TimedRows, WholeUnit, WholeUnitOutcome};
use central_engine_model::EngineTime;
use central_storage::StoreFailure;
use fleet_contracts::CentralObservation;
use fleet_contracts::{
    AlarmNotPublished, EngineSubject, EventCompletion, EventCompletionHook, LegacyUnitOutcome,
    MailDeliveryReady, ObservedControl,
};
use fleet_runtime::DueBatchState;
use fleet_runtime_ports::CentralStore;
use std::future::Future;

pub trait EngineHost: Send + Sync + Sized {
    type Timed: TimedRows<Self>;
    type WholeKind: Copy + Send + Sync + 'static;
    type Host: CentralStore;
    fn host(&self) -> &Self::Host;
    /// The one injected clock.
    fn now(&self) -> Result<EngineTime, StoreFailure>;
    fn ids(&self) -> &dyn fleet_runtime_ports::IdSource;
    fn due_batches(&self) -> &DueBatchState;
    fn observe(&self, observation: CentralObservation);
    fn offer_mail(&self, delivery: MailDeliveryReady);
    /// Today's organization stamp check (P3): `Ok(false)` when unprovisioned.
    fn organization_is_provisioned(
        &self,
    ) -> impl Future<Output = Result<bool, StoreFailure>> + Send;
    fn timed_rows(&self) -> &Self::Timed;
    fn alarm_inputs(&self) -> &'static str;
    fn ordered_whole_units(&self) -> &'static [Self::WholeKind];
    fn whole_unit(&self, kind: Self::WholeKind) -> WholeUnit;
    fn run_whole_unit(
        &self,
        kind: Self::WholeKind,
        now: EngineTime,
        subject: &EngineSubject,
        observed: &ObservedControl,
    ) -> impl Future<Output = Result<WholeUnitOutcome, LegacyUnitOutcome>> + Send;
    #[cfg(feature = "test-support")]
    fn pauses(&self) -> &fleet_runtime::PausePoints;
    #[cfg(feature = "test-support")]
    fn record_engine_completion_entry(&self);
}

/// The engine bound to one host. Every engine operation is a method here, so
/// none is attributed to WorkspaceRuntime.
pub struct Engine<'a, R: EngineHost>(pub(crate) &'a R);

impl<'a, R: EngineHost> Engine<'a, R> {
    pub fn new(host: &'a R) -> Self {
        Self(host)
    }
    pub(crate) fn host(&self) -> &'a R::Host {
        self.0.host()
    }
    pub(crate) fn now(&self) -> Result<EngineTime, StoreFailure> {
        self.0.now()
    }
    pub(crate) fn ids(&self) -> &'a dyn fleet_runtime_ports::IdSource {
        self.0.ids()
    }
    pub(crate) fn due_batches(&self) -> &'a DueBatchState {
        self.0.due_batches()
    }
    pub(crate) fn observe(&self, observation: CentralObservation) {
        self.0.observe(observation);
    }
    #[cfg(feature = "test-support")]
    pub(crate) fn pauses(&self) -> &'a fleet_runtime::PausePoints {
        self.0.pauses()
    }
}

impl<R: EngineHost> EventCompletionHook for Engine<'_, R> {
    async fn complete_event(&self) -> Result<EventCompletion, AlarmNotPublished> {
        self.after_event().await
    }
}
