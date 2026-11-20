//! The engine's only view of the runtime that hosts it (P3, module direction):
//! engine/ names this port and never WorkspaceRuntime, so the module graph has
//! one direction. `crate::engine_wiring` implements it for WorkspaceRuntime.
use super::legacy_units::LegacyUnitOutcome;
use crate::MailDeliveryReady;
use central_engine_model::EngineTime;
use central_storage::{GuardedStatement, StoreFailure};
use fleet_contracts::CentralObservation;
use fleet_runtime::DueBatchState;
use fleet_runtime_ports::CentralStore;
use std::future::Future;

pub(crate) trait EngineHost: Send + Sync {
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
    /// The notice-window unit's own planning, unchanged notice code (P5).
    fn plan_due_windows(
        &self,
        now: EngineTime,
    ) -> impl Future<
        Output = Result<(Vec<GuardedStatement>, Vec<MailDeliveryReady>), LegacyUnitOutcome>,
    > + Send;
    /// The workspace-drain unit, unchanged notice code (P5).
    fn drain_due_workspaces(
        &self,
        now: EngineTime,
    ) -> impl Future<Output = Result<usize, LegacyUnitOutcome>> + Send;
    /// The central turn the notice-window unit commits under, taken without
    /// waiting (P5 change 7): `None` when another event holds it.
    fn try_central_turn(&self) -> Option<fleet_runtime::TurnGuard<'_>>;
    #[cfg(feature = "test-support")]
    fn pauses(&self) -> &fleet_runtime::PausePoints;
    #[cfg(feature = "test-support")]
    fn record_engine_completion_entry(&self);
    #[cfg(feature = "test-support")]
    fn count_legacy_invocation(&self, count: &dyn Fn(&mut fleet_runtime::LegacyUnitInvocations));
}

/// The engine bound to one host. Every engine operation is a method here, so
/// none is attributed to WorkspaceRuntime.
pub(crate) struct Engine<'a, R: EngineHost>(pub(crate) &'a R);

impl<'a, R: EngineHost> Engine<'a, R> {
    pub(crate) fn new(host: &'a R) -> Self {
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
