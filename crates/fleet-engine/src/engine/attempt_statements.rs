use super::attempt_records::failure_detail;
use central_engine_model::{AttemptState, FailureRecord};
use central_storage::{GuardedStatement, SqlValue, Statement, StoreFailure};
use fleet_contracts::{AttemptGuard, EngineSubject};
use fleet_runtime_ports::stored_values::format_utc_millis;
use fleet_runtime_ports::{
    guarded_statements::{any, exact, text},
    workspace_decode_error::corrupt,
};

const READ_ATTEMPT: &str = "SELECT attempt_id,machine_kind,subject_key,state,failure_count,next_try_at,last_failure,last_failure_detail,first_failed_at,last_failed_at,retired_at,created_at,updated_at FROM engine_work_attempts WHERE machine_kind=? AND subject_key=?";

/// The literal predicate belongs to the adapter that owns the subject. No UPDATE
/// of the subject is used as a guard: that would retrigger its failed statement.
#[macro_export]
macro_rules! guarded_attempt_sql {
    ($subject_predicate:literal) => {
        concat!(
            "INSERT INTO engine_work_attempts(attempt_id,machine_kind,subject_key,state,failure_count,next_try_at,last_failure,last_failure_detail,first_failed_at,last_failed_at,retired_at,created_at,updated_at) ",
            "SELECT ?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13 WHERE ",
            "(?14=0 OR EXISTS(SELECT 1 FROM engine_work_attempts WHERE machine_kind=?2 AND subject_key=?3 AND attempt_id IS ?15 AND state IS ?16 AND failure_count IS ?17 AND last_failure IS ?18 AND updated_at IS ?19)) AND (",
            $subject_predicate,
            ") ON CONFLICT(machine_kind,subject_key) DO UPDATE SET attempt_id=excluded.attempt_id,state=excluded.state,failure_count=excluded.failure_count,next_try_at=excluded.next_try_at,last_failure=excluded.last_failure,last_failure_detail=excluded.last_failure_detail,first_failed_at=excluded.first_failed_at,last_failed_at=excluded.last_failed_at,retired_at=excluded.retired_at,created_at=excluded.created_at,updated_at=excluded.updated_at ",
            "WHERE ?14=1 AND engine_work_attempts.attempt_id IS ?15 AND engine_work_attempts.state IS ?16 AND engine_work_attempts.failure_count IS ?17 AND engine_work_attempts.last_failure IS ?18 AND engine_work_attempts.updated_at IS ?19"
        )
    };
}

pub const CONTROL_WRITE: &str = guarded_attempt_sql!("1");

pub(crate) fn read_attempt(subject: &EngineSubject) -> Statement {
    Statement {
        sql: READ_ATTEMPT,
        args: vec![text(subject.kind.0), text(&subject.key.0)],
    }
}

pub fn delete_attempt(subject: &EngineSubject) -> GuardedStatement {
    any(
        "DELETE FROM engine_work_attempts WHERE machine_kind=? AND subject_key=?",
        vec![text(subject.kind.0), text(&subject.key.0)],
    )
}

/// Deletes the control record only while it is still the one a run observed
/// (the write guard's columns, compared with IS). A record written or replaced
/// after the observation matches nothing and survives.
pub fn delete_observed_attempt(
    subject: &EngineSubject,
    observed: &AttemptGuard,
) -> GuardedStatement {
    any(
        "DELETE FROM engine_work_attempts WHERE machine_kind=? AND subject_key=? AND attempt_id IS ? AND state IS ? AND failure_count IS ? AND last_failure IS ? AND updated_at IS ?",
        vec![
            text(subject.kind.0),
            text(&subject.key.0),
            observed.attempt_id.clone(),
            observed.state.clone(),
            observed.failure_count.clone(),
            observed.last_failure.clone(),
            observed.updated_at.clone(),
        ],
    )
}

fn integer(value: i64) -> Result<SqlValue, StoreFailure> {
    value
        .try_into()
        .map(SqlValue::Integer)
        .map_err(|_| corrupt("engine_work_attempts.failure_count"))
}

/// Bind slots 1..19; an adapter's subject-version values start at slot 20.
pub(crate) fn write_attempt(
    sql: &'static str,
    subject: &EngineSubject,
    record: &FailureRecord,
    prior: Option<&AttemptGuard>,
    subject_version: Vec<SqlValue>,
) -> Result<GuardedStatement, StoreFailure> {
    let (state, next_try, retired_at) = match record.state {
        AttemptState::Failing { next_try_at } => (
            "failing",
            text(format_utc_millis(next_try_at)),
            SqlValue::Null,
        ),
        AttemptState::Retired { retired_at } => (
            "retired",
            SqlValue::Null,
            text(format_utc_millis(retired_at)),
        ),
    };
    let mut args = vec![
        text(record.attempt_id),
        text(subject.kind.0),
        text(&subject.key.0),
        text(state),
        integer(i64::from(record.failure_count.get()))?,
        next_try,
        text(record.last_failure.storage_tag()),
        failure_detail(&record.last_failure).map_or(SqlValue::Null, text),
        text(format_utc_millis(record.first_failed_at)),
        text(format_utc_millis(record.last_failed_at)),
        retired_at,
        text(format_utc_millis(record.created_at)),
        text(format_utc_millis(record.updated_at)),
        integer(i64::from(prior.is_some()))?,
    ];
    match prior {
        Some(prior) => args.extend([
            prior.attempt_id.clone(),
            prior.state.clone(),
            prior.failure_count.clone(),
            prior.last_failure.clone(),
            prior.updated_at.clone(),
        ]),
        None => args.extend([
            SqlValue::Null,
            SqlValue::Null,
            SqlValue::Null,
            SqlValue::Null,
            SqlValue::Null,
        ]),
    }
    args.extend(subject_version);
    Ok(exact(sql, args))
}

#[cfg(test)]
#[path = "attempt_statements_test.rs"]
mod tests;
