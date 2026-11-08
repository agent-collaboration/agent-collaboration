//! The only writer of `central_work_attempts`.
//!
//! A row exists only while something is pending or failing: it is created in
//! the transaction that records the work's first failure and deleted in the
//! transaction that finishes the work. Owners still decide what runs; this
//! table only says when a failing subject may be tried again and why it failed.
//! Domain deadlines stay on their own rows.

pub(crate) mod socket_requests;

use crate::generated_work_attempts as queries;
use durable_steps::{StoreError, Uuid, storage::TransactionScope};
use runtime_primitives::SyncTime;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::time::Duration;

/// The first retry after a failure; each further failure doubles it.
pub(crate) const FIRST_RETRY: Duration = Duration::from_secs(30);
/// Retries never wait longer than this.
pub(crate) const LONGEST_RETRY: Duration = Duration::from_secs(10 * 60);
/// A subject still failing this long after its first failure is given up:
/// suspended, never scheduled again, until an admin retries or discards it.
pub(crate) const RETIRE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// What kind of work a row tracks. Stored as its snake_case name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AttemptKind {
    Push,
    Status,
    SummaryCall,
    Settlement,
    SessionClose,
    NoticeDelivery,
    NoticeExpiry,
    MailRetention,
    /// Owed pushes whose item's validity ended (expired or left to its report).
    PushExpiry,
    FeedRetention,
    PartExpiry,
    ForwardExpiry,
    Lease,
    Open,
    SourceRead,
    AlarmWrite,
    Handler,
}

impl AttemptKind {
    const ALL: [Self; 17] = [
        Self::Push,
        Self::Status,
        Self::SummaryCall,
        Self::Settlement,
        Self::SessionClose,
        Self::NoticeDelivery,
        Self::NoticeExpiry,
        Self::MailRetention,
        Self::PushExpiry,
        Self::FeedRetention,
        Self::PartExpiry,
        Self::ForwardExpiry,
        Self::Lease,
        Self::Open,
        Self::SourceRead,
        Self::AlarmWrite,
        Self::Handler,
    ];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Push => "push",
            Self::Status => "status",
            Self::SummaryCall => "summary_call",
            Self::Settlement => "settlement",
            Self::SessionClose => "session_close",
            Self::NoticeDelivery => "notice_delivery",
            Self::NoticeExpiry => "notice_expiry",
            Self::MailRetention => "mail_retention",
            Self::PushExpiry => "push_expiry",
            Self::FeedRetention => "feed_retention",
            Self::PartExpiry => "part_expiry",
            Self::ForwardExpiry => "forward_expiry",
            Self::Lease => "lease",
            Self::Open => "open",
            Self::SourceRead => "source_read",
            Self::AlarmWrite => "alarm_write",
            Self::Handler => "handler",
        }
    }

    fn parse(stored: &str) -> Result<Self, StoreError> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == stored)
            .ok_or_else(|| corrupt("kind", format!("unknown attempt kind {stored:?}")))
    }
}

/// Why the latest attempt failed, in a class an operator can act on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ErrorClass {
    /// A stored row failed validation; retrying cannot fix it without repair.
    CorruptRow,
    /// Storage refused or failed the work.
    Storage,
    /// An owed answer did not arrive before its retry time (a resend went out).
    Unanswered,
    /// The receiver refused for now (mailbox full, sender binding unavailable);
    /// the request is resent on the backoff.
    RetryableRefusal,
}

impl ErrorClass {
    const fn as_str(self) -> &'static str {
        match self {
            Self::CorruptRow => "corrupt_row",
            Self::Storage => "storage",
            Self::Unanswered => "unanswered",
            Self::RetryableRefusal => "retryable_refusal",
        }
    }

    fn parse(stored: &str) -> Result<Self, StoreError> {
        match stored {
            "corrupt_row" => Ok(Self::CorruptRow),
            "storage" => Ok(Self::Storage),
            "unanswered" => Ok(Self::Unanswered),
            "retryable_refusal" => Ok(Self::RetryableRefusal),
            other => Err(corrupt(
                "last_error_class",
                format!("unknown error class {other:?}"),
            )),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AttemptFailure {
    pub(crate) class: ErrorClass,
    pub(crate) message: String,
}

impl From<&StoreError> for AttemptFailure {
    fn from(failure: &StoreError) -> Self {
        Self {
            class: match failure {
                StoreError::Corrupt { .. } => ErrorClass::CorruptRow,
                _ => ErrorClass::Storage,
            },
            message: failure.to_string(),
        }
    }
}

/// The send a socket request is waiting on (slice 4 writes these).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SocketSend {
    pub(crate) send_id: Uuid,
    pub(crate) connection: Uuid,
    pub(crate) session: Uuid,
    pub(crate) sent_version: Option<u64>,
}

/// Each state carries exactly the columns that state allows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AttemptState {
    /// Not sent, or failed and waiting; due at `next_attempt_at` if it has one.
    Waiting { next_attempt_at: Option<SyncTime> },
    /// Sent and waiting for its answer until `next_attempt_at`.
    InFlight {
        send: SocketSend,
        next_attempt_at: SyncTime,
    },
    /// Suspended after 24 h of failures: never due, never woken.
    GivenUp,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AttemptRow {
    pub(crate) id: Uuid,
    pub(crate) subject: Uuid,
    pub(crate) kind: AttemptKind,
    pub(crate) state: AttemptState,
    pub(crate) attempts: u32,
    pub(crate) first_failed_at: Option<SyncTime>,
    pub(crate) last_error: Option<AttemptFailure>,
    pub(crate) created_at: SyncTime,
    pub(crate) updated_at: SyncTime,
}

/// What a recorded failure means for the subject.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AttemptOutcome {
    Retry { at: SyncTime },
    GivenUp,
}

/// The wait before the attempt after `failures` consecutive failures.
pub(crate) fn backoff(failures: u32) -> Duration {
    let doublings = failures.saturating_sub(1).min(16);
    FIRST_RETRY
        .saturating_mul(1_u32 << doublings)
        .min(LONGEST_RETRY)
}

/// Record one failed attempt of `kind` on `subject` (ending any send it had
/// in flight), in the caller's
/// transaction (never the failing work's own, which rolled back). `id` is the
/// new row's UUIDv7 if no row exists yet.
pub(crate) async fn failed<Scope: TransactionScope>(
    scope: &mut Scope,
    id: Uuid,
    subject: Uuid,
    kind: AttemptKind,
    failure: &AttemptFailure,
    now: SyncTime,
) -> Result<AttemptOutcome, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Decode<'value, Scope::Database> + Encode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let Some(row) = attempt(scope, subject, kind).await? else {
        let at = now.after(backoff(1));
        queries::insert_work_attempt_failure(
            scope,
            id.to_string(),
            subject.to_string(),
            kind.as_str().into(),
            "waiting".into(),
            Some(at.unix_millis().to_string()),
            now.unix_millis().to_string(),
            failure.class.as_str().into(),
            failure.message.clone(),
        )
        .await
        .map_err(StoreError::backend)?;
        return Ok(AttemptOutcome::Retry { at });
    };
    let attempts = row.attempts.saturating_add(1);
    let first_failed_at = row.first_failed_at.unwrap_or(now);
    let outcome = if now >= first_failed_at.after(RETIRE_AFTER) {
        AttemptOutcome::GivenUp
    } else {
        AttemptOutcome::Retry {
            at: now.after(backoff(attempts)),
        }
    };
    let (state, next_attempt_at) = match outcome {
        AttemptOutcome::Retry { at } => ("waiting", Some(at.unix_millis().to_string())),
        AttemptOutcome::GivenUp => ("given_up", None),
    };
    queries::update_work_attempt_failure(
        scope,
        row.id.to_string(),
        state.into(),
        next_attempt_at,
        attempts.to_string(),
        first_failed_at.unix_millis().to_string(),
        failure.class.as_str().into(),
        failure.message.clone(),
        now.unix_millis().to_string(),
    )
    .await
    .map_err(StoreError::backend)?;
    Ok(outcome)
}

/// Central owes `subject` work that is answered from outside (a Router's
/// acknowledgment): keep a waiting row due at `at`, with no failure recorded.
/// An existing row is left as it is.
pub(crate) async fn owe<Scope: TransactionScope>(
    scope: &mut Scope,
    id: Uuid,
    subject: Uuid,
    kind: AttemptKind,
    at: SyncTime,
    now: SyncTime,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    if attempt(scope, subject, kind).await?.is_some() {
        return Ok(());
    }
    queries::insert_work_attempt_owed(
        scope,
        id.to_string(),
        subject.to_string(),
        kind.as_str().into(),
        at.unix_millis().to_string(),
        now.unix_millis().to_string(),
    )
    .await
    .map_err(StoreError::backend)
}

/// Subjects of `kind` due at `now` (not given up), oldest first, at most 500.
pub(crate) async fn due_subjects<Scope: TransactionScope>(
    scope: &mut Scope,
    kind: AttemptKind,
    now: SyncTime,
) -> Result<Vec<Uuid>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    queries::read_due_work_attempts(scope, kind.as_str().into(), now.unix_millis().to_string())
        .await
        .map_err(StoreError::backend)?
        .into_iter()
        .map(|row| private_id(row.subject, "subject"))
        .collect()
}

/// The work finished: forget its row, in the transaction that finished it.
pub(crate) async fn succeeded<Scope: TransactionScope>(
    scope: &mut Scope,
    subject: Uuid,
    kind: AttemptKind,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    queries::delete_work_attempt(scope, subject.to_string(), kind.as_str().into())
        .await
        .map(|_| ())
        .map_err(StoreError::backend)
}

pub(crate) async fn attempt<Scope: TransactionScope>(
    scope: &mut Scope,
    subject: Uuid,
    kind: AttemptKind,
) -> Result<Option<AttemptRow>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let mut rows = queries::read_work_attempt(scope, subject.to_string(), kind.as_str().into())
        .await
        .map_err(StoreError::backend)?;
    if rows.len() > 1 {
        return Err(corrupt("subject", "two rows for one subject and kind"));
    }
    rows.pop().map(decode).transpose()
}

/// How a subject's attempt row changes when its own work is due.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SubjectSchedule {
    /// No row: the subject follows its own due time.
    Unscheduled,
    /// Failing or waiting on an answer: due at this time instead.
    RetryAt(SyncTime),
    /// Given up, or waiting with no time (an unsent socket request): never due.
    NotDue,
}

impl SubjectSchedule {
    /// Whether work governed by this schedule may run at `now`.
    pub(crate) fn allows(self, now: SyncTime) -> bool {
        match self {
            Self::Unscheduled => true,
            Self::RetryAt(at) => at <= now,
            Self::NotDue => false,
        }
    }
}

pub(crate) async fn schedule<Scope: TransactionScope>(
    scope: &mut Scope,
    subject: Uuid,
    kind: AttemptKind,
) -> Result<SubjectSchedule, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    Ok(
        match attempt(scope, subject, kind).await?.map(|row| row.state) {
            None => SubjectSchedule::Unscheduled,
            Some(AttemptState::Waiting {
                next_attempt_at: Some(at),
            })
            | Some(AttemptState::InFlight {
                next_attempt_at: at,
                ..
            }) => SubjectSchedule::RetryAt(at),
            Some(AttemptState::Waiting {
                next_attempt_at: None,
            })
            | Some(AttemptState::GivenUp) => SubjectSchedule::NotDue,
        },
    )
}

/// When a subject that isn't given up may next be tried, if any is waiting.
pub(crate) async fn next_retry<Scope: TransactionScope>(
    scope: &mut Scope,
) -> Result<Option<SyncTime>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let row = queries::read_next_work_retry(scope)
        .await
        .map_err(StoreError::backend)?
        .pop();
    match row {
        None => Ok(None),
        Some(row) => millis(
            row.next_attempt_at,
            row.next_attempt_class,
            "next_attempt_at",
        )
        .map(|at| at.map(SyncTime::from_unix_millis)),
    }
}

fn decode(row: queries::StoredWorkAttempt) -> Result<AttemptRow, StoreError> {
    let id = private_id(row.id, "id")?;
    let subject = private_id(row.subject, "subject")?;
    let kind = AttemptKind::parse(&required(row.kind, "kind")?)?;
    let next_attempt_at = millis(
        row.next_attempt_at,
        row.next_attempt_class,
        "next_attempt_at",
    )?
    .map(SyncTime::from_unix_millis);
    let send_id = row
        .send_id
        .map(|value| private_id(Some(value), "send_id"))
        .transpose()?;
    let connection = row
        .connection_id
        .map(|value| private_id(Some(value), "connection_id"))
        .transpose()?;
    let session = row
        .session_id
        .map(|value| private_id(Some(value), "session_id"))
        .transpose()?;
    let sent_version = millis(row.sent_version, row.sent_version_class, "sent_version")?
        .map(|version| u64::try_from(version).map_err(|error| corrupt("sent_version", error)))
        .transpose()?;
    let state = match required(row.state, "state")?.as_str() {
        "waiting" if send_id.is_none() && connection.is_none() && session.is_none() => {
            AttemptState::Waiting { next_attempt_at }
        }
        "in_flight" => match (send_id, connection, session, next_attempt_at) {
            (Some(send_id), Some(connection), Some(session), Some(next_attempt_at)) => {
                AttemptState::InFlight {
                    send: SocketSend {
                        send_id,
                        connection,
                        session,
                        sent_version,
                    },
                    next_attempt_at,
                }
            }
            _ => {
                return Err(corrupt(
                    "state",
                    "in_flight row without its send or retry time",
                ));
            }
        },
        "given_up" if next_attempt_at.is_none() && send_id.is_none() => AttemptState::GivenUp,
        "waiting" | "given_up" => {
            return Err(corrupt("state", "state carries columns it does not allow"));
        }
        other => return Err(corrupt("state", format!("unknown attempt state {other:?}"))),
    };
    if sent_version.is_some() && !matches!(state, AttemptState::InFlight { .. }) {
        return Err(corrupt("sent_version", "a version without a send"));
    }
    let attempts = millis(row.attempts, row.attempts_class, "attempts")?
        .ok_or_else(|| corrupt("attempts", "required column is NULL"))?;
    let attempts = u32::try_from(attempts).map_err(|error| corrupt("attempts", error))?;
    let first_failed_at = millis(
        row.first_failed_at,
        row.first_failed_class,
        "first_failed_at",
    )?
    .map(SyncTime::from_unix_millis);
    let last_error = match (row.last_error_class, row.last_error) {
        (Some(class), Some(message)) => Some(AttemptFailure {
            class: ErrorClass::parse(&class)?,
            message,
        }),
        (None, None) => None,
        _ => {
            return Err(corrupt(
                "last_error",
                "error class and message must come together",
            ));
        }
    };
    if (attempts > 0) != first_failed_at.is_some() || (attempts > 0) != last_error.is_some() {
        return Err(corrupt(
            "attempts",
            "failure count, first failure and last error disagree",
        ));
    }
    Ok(AttemptRow {
        id,
        subject,
        kind,
        state,
        attempts,
        first_failed_at,
        last_error,
        created_at: SyncTime::from_unix_millis(
            millis(row.created_at, row.created_class, "created_at")?
                .ok_or_else(|| corrupt("created_at", "required column is NULL"))?,
        ),
        updated_at: SyncTime::from_unix_millis(
            millis(row.updated_at, row.updated_class, "updated_at")?
                .ok_or_else(|| corrupt("updated_at", "required column is NULL"))?,
        ),
    })
}

fn required(value: Option<String>, field: &str) -> Result<String, StoreError> {
    value.ok_or_else(|| corrupt(field, "required column is NULL"))
}

fn private_id(value: Option<String>, field: &str) -> Result<Uuid, StoreError> {
    let value = required(value, field)?;
    let id = Uuid::parse_str(&value).map_err(|error| corrupt(field, error))?;
    if !runtime_primitives::uuid_v7(id) || id.to_string() != value {
        return Err(corrupt(field, "identity must be canonical UUIDv7"));
    }
    Ok(id)
}

/// A canonical SQLite integer, or `None` for SQL NULL.
fn millis(
    value: Option<String>,
    class: Option<String>,
    field: &str,
) -> Result<Option<i64>, StoreError> {
    match (value, required(class, field)?.as_str()) {
        (None, "null") => Ok(None),
        (Some(value), "integer") => {
            let parsed = value
                .parse::<i64>()
                .map_err(|error| corrupt(field, error))?;
            if parsed.to_string() != value {
                return Err(corrupt(field, "integer is not canonical"));
            }
            Ok(Some(parsed))
        }
        _ => Err(corrupt(field, "column is not an integer or NULL")),
    }
}

fn corrupt(field: &str, message: impl std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        field: format!("central_work_attempts.{field}"),
        message: message.to_string(),
    }
}

#[cfg(test)]
#[path = "work_attempts_test.rs"]
mod tests;
