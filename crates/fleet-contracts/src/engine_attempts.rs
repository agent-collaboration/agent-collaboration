use central_engine_model::{InstanceKey, MachineKind};
use central_storage::SqlValue;

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

/// Raw comparison values survive a decode failure, so recording it still checks
/// the exact subject that was read. The adapter owns their SQL interpretation.
#[derive(Clone, Debug)]
pub struct SubjectVersion(pub Vec<SqlValue>);

/// The control record a run saw at its gate. Its success clears only that
/// record, so a failure written after the gate survives (R3/R4).
pub enum ObservedControl {
    Absent,
    Present(AttemptGuard),
}
