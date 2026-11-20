//! Test-support faults are consumed at the actual host batch boundary, after
//! core has constructed the real transaction. No stand-in replaces storage.
use central_engine_model::InstanceKey;
use central_storage::{
    AlarmUpdate, BatchOutcome, CorruptReason, SqlValue, Statement, StatementBatch, StoreFailure,
};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, PoisonError};

#[derive(Default)]
pub struct TestFaults {
    commit: Mutex<Option<EngineCommitFault>>,
    read: Mutex<Option<EngineReadFault>>,
    alarm_batch: Mutex<Option<StoreFailure>>,
}

impl TestFaults {
    pub fn arm_commit(&self, fault: EngineCommitFault) {
        *self.commit.lock().unwrap_or_else(PoisonError::into_inner) = Some(fault);
    }

    pub fn commit_pending(&self) -> bool {
        self.commit
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    pub fn take_commit(&self, batch: &StatementBatch) -> Option<EngineCommitFault> {
        let mut armed = self.commit.lock().unwrap_or_else(PoisonError::into_inner);
        if armed.as_ref().is_some_and(|fault| fault.matches(batch)) {
            armed.take()
        } else {
            None
        }
    }

    pub fn arm_read(&self, fault: EngineReadFault) {
        *self.read.lock().unwrap_or_else(PoisonError::into_inner) = Some(fault);
    }

    pub fn take_read_failure(&self, statement: &Statement) -> Option<StoreFailure> {
        let mut armed = self.read.lock().unwrap_or_else(PoisonError::into_inner);
        if armed.as_ref().is_some_and(|fault| fault.matches(statement)) {
            armed.take();
            Some(StoreFailure::Corrupt {
                reason: CorruptReason::BadValue {
                    column: "engine.due_list".into(),
                },
            })
        } else {
            None
        }
    }

    pub fn arm_alarm_batch(&self, failure: StoreFailure) {
        *self
            .alarm_batch
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(failure);
    }

    pub fn take_alarm_batch_failure(&self, batch: &StatementBatch) -> Option<StoreFailure> {
        if batch.statements.is_empty()
            && matches!(
                batch.alarm,
                AlarmUpdate::SetFromQuery(_) | AlarmUpdate::SetEngineFromQuery { .. }
            )
        {
            self.alarm_batch
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EngineCommitFault {
    pub key: InstanceKey,
    pub kind: EngineCommitFaultKind,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "code",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum EngineCommitFaultKind {
    NotAppliedAnswerLost,
    AppliedAnswerLost,
    Unavailable,
    DatabaseCode(i32),
    /// The batch's guarded statement reports GuardFailed, for the drain cursor
    /// whose real overlap the workspace drain turn makes impossible (P5).
    GuardFailed,
}
impl EngineCommitFault {
    pub fn matches(&self, batch: &StatementBatch) -> bool {
        batch.statements.iter().any(|statement| {
            let sql = statement.statement.sql;
            (sql.starts_with("UPDATE automation_wakes SET")
                || sql.starts_with("UPDATE automation_schedules SET")
                || sql.starts_with("UPDATE engine_probe_items SET")
                || sql.starts_with("UPDATE mail_workspace_drains SET"))
                && statement
                    .statement
                    .args
                    .iter()
                    .any(|value| matches!(value,SqlValue::Text(key) if key==&self.key.0))
        })
    }
    pub fn before(&self) -> Option<Result<BatchOutcome, StoreFailure>> {
        match self.kind {
            EngineCommitFaultKind::NotAppliedAnswerLost => Some(Err(StoreFailure::AnswerLost)),
            EngineCommitFaultKind::Unavailable => Some(Err(StoreFailure::Unavailable)),
            EngineCommitFaultKind::DatabaseCode(code) => {
                Some(if is_statement_rejected_code(code) {
                    Ok(BatchOutcome::StatementRejected { index: 0 })
                } else {
                    Err(StoreFailure::Unavailable)
                })
            }
            EngineCommitFaultKind::GuardFailed => Some(Ok(BatchOutcome::GuardFailed {
                index: 0,
                rows_affected: 0,
            })),
            EngineCommitFaultKind::AppliedAnswerLost => None,
        }
    }
}
/// SQLite's closed item-rejection codes; FULL/IOERR/BUSY and unknown codes are
/// operational store failures. Both native adapters use the same classification.
pub fn is_statement_rejected_code(code: i32) -> bool {
    matches!(
        code,
        19 | 20 | 531 | 1043 | 1811 | 2323 | 2579 | 2835 | 3091
    )
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EngineReadFault {
    pub kind: String,
}
impl EngineReadFault {
    pub fn matches(&self, statement: &Statement) -> bool {
        statement
            .sql
            .starts_with("SELECT subject_key,occurrence_ms,effective_due_ms")
            && match self.kind.as_str() {
                "wake_timer" => statement.sql.contains("FROM automation_wakes s"),
                "automation_timer" => statement.sql.contains("FROM automation_schedules s"),
                "direct_message" => statement.sql.contains("FROM mail_messages s"),
                "engine_probe" => statement.sql.contains("s.machine_kind='engine_probe'"),
                _ => false,
            }
    }
}

/// Test-support counts of whole notice-unit runs (V2 control gate).
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
