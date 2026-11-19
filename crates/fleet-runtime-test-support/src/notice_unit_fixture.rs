//! Test-only notice units for composing requests and the engine in isolation.
use central_engine_model::EngineTime;
use central_storage::{GuardedStatement, StoreFailure};
use fleet_contracts::{DueWorkSource, LegacyUnitOutcome, WorkspaceDrain};
use std::{
    collections::BTreeMap,
    sync::{Mutex, PoisonError},
};
use workspace_ids::WorkspaceId;
use workspace_model::Timestamp;

#[derive(Default)]
pub struct InMemoryNoticeUnits {
    state: Mutex<NoticeUnitState>,
}

#[derive(Default)]
struct NoticeUnitState {
    scheduled: BTreeMap<WorkspaceId, Timestamp>,
    drained: Vec<WorkspaceId>,
}

impl InMemoryNoticeUnits {
    pub fn scheduled(&self) -> Vec<(WorkspaceId, Timestamp)> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .scheduled
            .iter()
            .map(|(workspace, due)| (workspace.clone(), *due))
            .collect()
    }

    pub fn drained(&self) -> Vec<WorkspaceId> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drained
            .clone()
    }
}

impl WorkspaceDrain for InMemoryNoticeUnits {
    async fn preset_workspace_drain(
        &self,
        workspace: &WorkspaceId,
        now: Timestamp,
    ) -> Result<(), StoreFailure> {
        let due = now
            .checked_add_signed(chrono::Duration::seconds(5))
            .ok_or_else(|| fleet_runtime_ports::workspace_decode_error::corrupt("drain_due_at"))?;
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .scheduled
            .entry(workspace.clone())
            .or_insert(due);
        Ok(())
    }

    async fn drain_after_request(&self, workspace: &WorkspaceId) -> Result<(), StoreFailure> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.scheduled.remove(workspace);
        state.drained.push(workspace.clone());
        Ok(())
    }
}

impl DueWorkSource for InMemoryNoticeUnits {
    async fn plan_due_windows(
        &self,
        _: EngineTime,
    ) -> Result<(Vec<GuardedStatement>, usize), LegacyUnitOutcome> {
        Ok((Vec::new(), 0))
    }

    async fn drain_due_workspaces(&self, now: EngineTime) -> Result<usize, LegacyUnitOutcome> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let mut due = Vec::new();
        state.scheduled.retain(|workspace, mark| {
            if *mark > now {
                return true;
            }
            due.push(workspace.clone());
            false
        });
        let count = due.len();
        state.drained.extend(due);
        Ok(count)
    }
}
