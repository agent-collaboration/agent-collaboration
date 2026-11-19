//! Values shared by mail and engine attempt control.
use central_engine_model::{
    FailureRecord, InstanceKey, ItemFailureClass, MachineKind, RetirementThreshold,
};
use central_storage::{AlarmUpdate, GuardedStatement, SqlValue, StoreFailure};
use std::future::Future;
use workspace_model::WorkspaceId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EngineSubject {
    pub kind: MachineKind,
    pub key: InstanceKey,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttemptGuard {
    pub attempt_id: SqlValue,
    pub state: SqlValue,
    pub failure_count: SqlValue,
    pub last_failure: SqlValue,
    pub updated_at: SqlValue,
}

/// The control record a run saw at its gate. Its success clears only that
/// record, so a failure written after the gate survives (R3/R4).
pub enum ObservedControl {
    Absent,
    Present(AttemptGuard),
}

pub enum ControlGate {
    Run(ObservedControl),
    Skip,
    Uncertain,
    CentralDown(StoreFailure),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FailureOutcome {
    Recorded(FailureRecord),
    Superseded,
    Uncertain,
    StoreDown(StoreFailure),
}

/// Raw comparison values survive a decode failure, so recording it still checks
/// the exact subject that was read. The adapter owns their SQL interpretation.
#[derive(Clone, Debug)]
pub struct SubjectVersion(pub Vec<SqlValue>);

pub trait EngineAttemptControl: Send + Sync {
    fn control_gate(
        &self,
        subject: &EngineSubject,
        threshold: RetirementThreshold,
    ) -> impl Future<Output = ControlGate> + Send;

    fn observe_engine_store_down(&self, subject: &EngineSubject, failure: StoreFailure);

    /// The engine supplies its control-write SQL for this notice-unit failure.
    fn record_failure_naming(
        &self,
        subject: &EngineSubject,
        version: &SubjectVersion,
        class: ItemFailureClass,
        threshold: RetirementThreshold,
        workspaces: &[WorkspaceId],
    ) -> impl Future<Output = FailureOutcome> + Send;

    fn attempt_cleanup_statements(&self, subject: &EngineSubject) -> GuardedStatement;
}

pub trait AlarmRecompute: Send + Sync {
    fn alarm_update(&self) -> AlarmUpdate;
}
