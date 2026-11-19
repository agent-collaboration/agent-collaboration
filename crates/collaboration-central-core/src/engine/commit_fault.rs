//! Test-support faults are consumed at the actual host batch boundary, after
//! core has constructed the real transaction. No stand-in replaces storage.
use central_engine_model::InstanceKey;
use central_storage::{BatchOutcome, SqlValue, Statement, StatementBatch, StoreFailure};
use serde::{Deserialize, Serialize};

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
