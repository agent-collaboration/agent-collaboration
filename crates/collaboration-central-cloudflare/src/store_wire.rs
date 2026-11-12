//! The owned form of core's storage shapes, for running them and for carrying a
//! store call to a workspace-store Durable Object.
//!
//! Core's `Statement` holds `sql: &'static str`, which cannot be deserialized. At
//! this edge the SQL becomes an owned `String`; nothing else changes shape.

use collaboration_central_core::storage::{
    AlarmUpdate, GuardedStatement, RowExpectation, SqlValue, Statement, StatementBatch, StoreCall,
    StoreFailure, StoreReply,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OwnedStatement {
    pub(crate) sql: String,
    pub(crate) args: Vec<SqlValue>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OwnedGuardedStatement {
    pub(crate) statement: OwnedStatement,
    pub(crate) expect: RowExpectation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "query", rename_all = "camelCase")]
pub(crate) enum OwnedAlarmUpdate {
    Unchanged,
    SetFromQuery(OwnedStatement),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OwnedBatch {
    pub(crate) statements: Vec<OwnedGuardedStatement>,
    pub(crate) alarm: OwnedAlarmUpdate,
}

/// The request body a workspace-store object receives.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "call", rename_all = "camelCase")]
pub(crate) enum OwnedStoreCall {
    Batch(OwnedBatch),
    Read(OwnedStatement),
}

/// The response body a workspace-store object answers with.
pub(crate) type StoreAnswer = Result<StoreReply, StoreFailure>;

impl From<Statement> for OwnedStatement {
    fn from(statement: Statement) -> Self {
        Self {
            sql: statement.sql.to_owned(),
            args: statement.args,
        }
    }
}

impl From<GuardedStatement> for OwnedGuardedStatement {
    fn from(guarded: GuardedStatement) -> Self {
        Self {
            statement: guarded.statement.into(),
            expect: guarded.expect,
        }
    }
}

impl From<AlarmUpdate> for OwnedAlarmUpdate {
    fn from(alarm: AlarmUpdate) -> Self {
        match alarm {
            AlarmUpdate::Unchanged => Self::Unchanged,
            AlarmUpdate::SetFromQuery(statement) => Self::SetFromQuery(statement.into()),
        }
    }
}

impl From<StatementBatch> for OwnedBatch {
    fn from(batch: StatementBatch) -> Self {
        Self {
            statements: batch.statements.into_iter().map(Into::into).collect(),
            alarm: batch.alarm.into(),
        }
    }
}

impl From<StoreCall> for OwnedStoreCall {
    fn from(call: StoreCall) -> Self {
        match call {
            StoreCall::Batch(batch) => Self::Batch(batch.into()),
            StoreCall::Read(statement) => Self::Read(statement.into()),
        }
    }
}

#[cfg(test)]
#[path = "store_wire_test.rs"]
mod tests;
