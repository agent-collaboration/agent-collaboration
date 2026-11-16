//! The engine's only view of the runtime that hosts it (P3, module direction):
//! engine/ names this port and never WorkspaceRuntime, so the module graph has
//! one direction. `crate::engine_wiring` implements it for WorkspaceRuntime.
use super::{DueBatchState, legacy_units::LegacyUnitOutcome};
use crate::{CentralObservation, ObjectHost};
use central_engine_model::EngineTime;
use central_storage::{GuardedStatement, StoreFailure};
use std::future::Future;

pub(crate) trait EngineHost: Send + Sync {
    type Host: ObjectHost;
    fn host(&self) -> &Self::Host;
    /// The one injected clock.
    fn now(&self) -> Result<EngineTime, StoreFailure>;
    fn ids(&self) -> &dyn runtime_primitives::IdSource;
    fn due_batches(&self) -> &DueBatchState;
    fn observe(&self, observation: CentralObservation);
    /// Today's organization stamp check (P3): `Ok(false)` when unprovisioned.
    fn organization_is_provisioned(
        &self,
    ) -> impl Future<Output = Result<bool, StoreFailure>> + Send;
    /// The notice-window unit's own planning, unchanged notice code (P5).
    fn plan_due_windows(
        &self,
        now: EngineTime,
    ) -> impl Future<Output = Result<(Vec<GuardedStatement>, usize), LegacyUnitOutcome>> + Send;
    /// The workspace-drain unit, unchanged notice code (P5).
    fn drain_due_workspaces(
        &self,
        now: EngineTime,
    ) -> impl Future<Output = Result<usize, LegacyUnitOutcome>> + Send;
    /// The central turn the notice-window unit commits under, as today.
    fn central_turn(&self) -> impl Future<Output = tokio::sync::MutexGuard<'_, ()>> + Send;
    #[cfg(feature = "test-support")]
    fn pauses(&self) -> &crate::PausePoints;
    #[cfg(feature = "test-support")]
    fn count_legacy_invocation(&self, count: &dyn Fn(&mut LegacyUnitInvocations));
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
    pub(crate) fn ids(&self) -> &'a dyn runtime_primitives::IdSource {
        self.0.ids()
    }
    pub(crate) fn due_batches(&self) -> &'a DueBatchState {
        self.0.due_batches()
    }
    pub(crate) fn observe(&self, observation: CentralObservation) {
        self.0.observe(observation);
    }
    #[cfg(feature = "test-support")]
    pub(crate) fn pauses(&self) -> &'a crate::PausePoints {
        self.0.pauses()
    }
}

/// Test-support counts of whole notice-unit runs (V2 control gate).
#[cfg(feature = "test-support")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyUnitInvocations {
    /// The due batch ran the notice-window unit.
    pub windows: u64,
    /// The due batch ran the workspace-drain unit.
    pub drains: u64,
    /// A request-triggered drain of one workspace ran (P5 change 5).
    pub request_drains: u64,
}
