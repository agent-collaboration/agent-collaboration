//! The injected capabilities and execution state for one organization.

use crate::central_turn::{CentralTurn, TurnGuard};
use crate::{AlarmSchedule, DueBatchState, WorkspaceTurns};
use central_engine_model::EngineTime;
use central_storage::{
    BatchOutcome, ReadOutcome, Statement, StatementBatch, StoreCall, StoreFailure, StoreReply,
};
use fleet_contracts::{
    AlarmNotPublished, CentralObservation, EventCompletion, EventCompletionHook,
};
use fleet_identity_protocol::OrganizationId;
use fleet_runtime_ports::workspace_decode_error::corrupt;
use fleet_runtime_ports::{CentralStore, Clock, IdSource, WorkspaceStores};
use std::sync::Arc;
use workspace_ids::WorkspaceId;

pub struct FleetRuntime<Host: CentralStore, Stores: WorkspaceStores> {
    organization: OrganizationId,
    host: Arc<Host>,
    stores: Stores,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdSource>,
    due_batches: Arc<DueBatchState>,
    turns: WorkspaceTurns,
    central_turn: CentralTurn,
    observer: Arc<dyn fleet_runtime_ports::Observer<CentralObservation>>,
    #[cfg(feature = "test-support")]
    pauses: crate::PausePoints,
    #[cfg(feature = "test-support")]
    legacy_invocations: std::sync::Mutex<crate::LegacyUnitInvocations>,
    #[cfg(feature = "test-support")]
    lost_answers: std::sync::Mutex<std::collections::BTreeSet<workspace_model::RequestId>>,
}

impl<Host: CentralStore, Stores: WorkspaceStores> FleetRuntime<Host, Stores> {
    /// Opens one organization on its prepared ports. The alarm publisher and
    /// runtime share the same due-batch state.
    pub fn open(
        organization: OrganizationId,
        host: Arc<Host>,
        stores: Stores,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdSource>,
        due_batches: Arc<DueBatchState>,
        observer: Arc<dyn fleet_runtime_ports::Observer<CentralObservation>>,
    ) -> Self {
        Self {
            organization,
            host,
            stores,
            clock,
            ids,
            due_batches,
            observer,
            turns: WorkspaceTurns::default(),
            central_turn: CentralTurn::default(),
            #[cfg(feature = "test-support")]
            pauses: crate::PausePoints::default(),
            #[cfg(feature = "test-support")]
            legacy_invocations: Default::default(),
            #[cfg(feature = "test-support")]
            lost_answers: Default::default(),
        }
    }

    pub fn organization(&self) -> &OrganizationId {
        &self.organization
    }

    pub fn statement_host(&self) -> &Host {
        &self.host
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

    pub fn workspace_turns(&self) -> &WorkspaceTurns {
        &self.turns
    }

    pub fn observe(&self, observation: CentralObservation) {
        self.observer.observe(observation);
    }

    pub async fn take_central_turn(&self) -> TurnGuard<'_> {
        self.central_turn.take().await
    }

    pub fn try_central_turn(&self) -> Option<TurnGuard<'_>> {
        self.central_turn.try_take()
    }

    pub fn mint<T: TryFrom<String>>(&self) -> Result<T, StoreFailure> {
        T::try_from(self.ids.new_id().to_string()).map_err(|_| corrupt("minted id"))
    }

    pub fn now(&self) -> Result<EngineTime, StoreFailure> {
        chrono::DateTime::from_timestamp_millis(self.clock.now().unix_millis())
            .ok_or_else(|| corrupt("clock"))
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
            StoreReply::Batch(_) => Err(corrupt("store reply")),
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
            StoreReply::Read(_) => Err(corrupt("store reply")),
        }
    }

    /// Completes an event through the composed feature before returning its alarm result.
    pub async fn after_event(
        &self,
        hook: &impl EventCompletionHook,
    ) -> Result<EventCompletion, AlarmNotPublished> {
        hook.complete_event().await
    }

    pub fn alarm_schedule<Deadline: Copy>(&self) -> AlarmSchedule<Deadline> {
        AlarmSchedule::new()
    }

    #[cfg(feature = "test-support")]
    pub async fn until_central_waiting(&self, count: usize) {
        self.central_turn.until_waiting(count).await;
    }

    #[cfg(feature = "test-support")]
    pub fn pause_points(&self) -> &crate::PausePoints {
        &self.pauses
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
    pub fn legacy_unit_invocations(&self) -> crate::LegacyUnitInvocations {
        *self
            .legacy_invocations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    #[cfg(feature = "test-support")]
    pub fn count_legacy_invocation(&self, count: impl FnOnce(&mut crate::LegacyUnitInvocations)) {
        count(
            &mut self
                .legacy_invocations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
    }
}
