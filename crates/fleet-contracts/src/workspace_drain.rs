//! Mail drain operations invoked while applying workspace requests.
use crate::LegacyUnitOutcome;
use central_engine_model::EngineTime;
use central_storage::{GuardedStatement, StoreFailure};
use std::future::Future;
use workspace_model::{Timestamp, WorkspaceId};

pub trait WorkspaceDrain: Send + Sync {
    fn preset_workspace_drain(
        &self,
        workspace: &WorkspaceId,
        now: Timestamp,
    ) -> impl Future<Output = Result<(), StoreFailure>> + Send;

    fn drain_after_request(
        &self,
        workspace: &WorkspaceId,
    ) -> impl Future<Output = Result<(), StoreFailure>> + Send;
}

pub trait DueWorkSource: Send + Sync {
    fn plan_due_windows(
        &self,
        now: EngineTime,
    ) -> impl Future<Output = Result<(Vec<GuardedStatement>, usize), LegacyUnitOutcome>> + Send;

    fn drain_due_workspaces(
        &self,
        now: EngineTime,
    ) -> impl Future<Output = Result<usize, LegacyUnitOutcome>> + Send;
}
