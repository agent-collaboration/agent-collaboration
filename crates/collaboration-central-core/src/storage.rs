//! The only storage vocabulary central core uses.
//!
//! Core never picks a database engine. It builds statements (SQL text plus typed
//! arguments) and hands a batch to its host, which runs every statement in one
//! transaction: stock SQLx SQLite on Tokio, the SQLx Durable Object driver inside
//! the object's storage transaction on Cloudflare. The same shapes travel to a
//! workspace store through [`crate::CentralPlatform::call_store`].

use serde::Serialize;

/// One SQL statement and its positional arguments.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Statement {
    pub sql: &'static str,
    pub args: Vec<SqlValue>,
}

/// The values a statement binds or a read returns, the part both hosts keep alike.
/// No REAL: a Durable Object reads a whole-number REAL back as an integer, and no
/// schema stores one.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum SqlValue {
    Null,
    Integer(SafeInteger),
    Text(String),
    Blob(Vec<u8>),
}

/// An integer within ±(2^53−1): a Durable Object's SQLite reaches Rust through
/// JavaScript numbers, so both hosts refuse anything wider at construction.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, serde::Deserialize)]
#[serde(try_from = "i64", into = "i64")]
pub struct SafeInteger(i64);

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{value} is outside ±(2^53−1)")]
pub struct UnsafeInteger {
    pub value: i64,
}

impl SafeInteger {
    /// 2^53 − 1, the largest integer a JavaScript number holds exactly.
    pub const MAX: Self = Self(9_007_199_254_740_991);
    pub const MIN: Self = Self(-9_007_199_254_740_991);

    pub const fn new(value: i64) -> Result<Self, UnsafeInteger> {
        if value > Self::MAX.0 || value < Self::MIN.0 {
            return Err(UnsafeInteger { value });
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl TryFrom<i64> for SafeInteger {
    type Error = UnsafeInteger;
    fn try_from(value: i64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<SafeInteger> for i64 {
    fn from(value: SafeInteger) -> Self {
        value.0
    }
}

/// A guard on the rows a statement affects; a miss rolls the whole batch back.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "rows", rename_all = "camelCase")]
pub enum RowExpectation {
    Any,
    Exactly(u64),
    AtLeastOne,
}

impl RowExpectation {
    /// Whether `rows_affected` satisfies this guard.
    pub const fn admits(self, rows_affected: u64) -> bool {
        match self {
            Self::Any => true,
            Self::Exactly(expected) => rows_affected == expected,
            Self::AtLeastOne => rows_affected >= 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GuardedStatement {
    pub statement: Statement,
    pub expect: RowExpectation,
}

/// Statements the host runs atomically, in order, in one transaction.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatementBatch {
    pub statements: Vec<GuardedStatement>,
    /// Central's own database only; ignored by workspace stores.
    pub alarm: AlarmUpdate,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", content = "query", rename_all = "camelCase")]
pub enum AlarmUpdate {
    Unchanged,
    /// Core's Timers module supplies the query; the host runs it last in the same
    /// transaction and sets the alarm to its one result (or clears it on NULL).
    /// The host knows no business table: it only runs the statement and applies the time.
    /// The query returns one row with one column: the alarm time as INTEGER Unix
    /// milliseconds, or NULL to clear the alarm. Any other answer is `StoreFailure::Corrupt`.
    SetFromQuery(Statement),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, serde::Deserialize)]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub enum BatchOutcome {
    #[serde(rename_all = "camelCase")]
    Applied { rows_affected: Vec<u64> },
    /// Rolled back.
    #[serde(rename_all = "camelCase")]
    GuardFailed { index: u32, rows_affected: u64 },
    /// Rolled back.
    #[serde(rename_all = "camelCase")]
    ConstraintViolated { index: u32, kind: ConstraintKind },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConstraintKind {
    Unique,
    ForeignKey,
    NotNull,
    CheckBoolean,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadOutcome {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<SqlValue>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, serde::Deserialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StoreFailure {
    #[error("the store is unavailable")]
    Unavailable,
    #[error("the store's answer was lost")]
    AnswerLost,
    #[error("the store holds corrupt data: {reason:?}")]
    Corrupt { reason: CorruptReason },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CorruptReason {
    MissingColumn { column: String },
    UnknownKind { column: String },
    BadValue { column: String },
}

/// A request to one workspace store.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", content = "call", rename_all = "camelCase")]
pub enum StoreCall {
    Batch(StatementBatch),
    Read(Statement),
}

#[derive(Clone, Debug, PartialEq, Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "reply", rename_all = "camelCase")]
pub enum StoreReply {
    Batch(BatchOutcome),
    Read(ReadOutcome),
}

#[cfg(test)]
#[path = "storage_test.rs"]
mod tests;
