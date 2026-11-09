//! S8: an object's durable state and its shell's registrations, for conformance.
//!
//! The durable fields come from one read-only store transaction through the
//! object's runtime face, so reading never opens, repairs, runs due work or
//! writes the alarm. The shell reads its registration table before and after
//! that transaction (the passive fence) and composes the answer with
//! [`compose`]; memory and SQL are never claimed to be read atomically.
//! Every durable row is reported as stored: a value that fails to decode is a
//! refusal, never a coerced or dropped row.

use durable_steps::Uuid;
use fleet_protocol::{ConnectionId, PeerId};
use fleet_session::ConnectionRegistration;
use serde::{Deserialize, Serialize};

/// The S8 answer (seam contract v3.6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObjectSnapshot {
    pub attempts: Vec<AttemptRecord>,
    pub sessions: Vec<SessionRecord>,
    pub inventory: Vec<ConnectionId>,
    pub windows: Vec<WindowRecord>,
    pub notice_progress: Vec<NoticeProgressRecord>,
    pub feed: Option<FeedRecord>,
    pub owners: OwnersRecord,
}

/// One `central_work_attempts` row, as stored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttemptRecord {
    pub id: Uuid,
    pub subject: Uuid,
    pub kind: String,
    pub state: String,
    pub send_id: Option<Uuid>,
    pub connection_id: Option<Uuid>,
    pub session_id: Option<Uuid>,
    pub sent_version: Option<u64>,
    pub next_attempt_at: Option<i64>,
    pub attempts: u64,
    pub first_failed_at: Option<i64>,
    pub last_error_class: Option<String>,
    pub last_error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// One shell registration: every connection, in every state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionRecord {
    /// The attaching or attached session; none while opening or closed.
    pub session: Option<Uuid>,
    pub peer: PeerId,
    pub connection: ConnectionId,
    pub registration: RegistrationState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RegistrationState {
    Opening,
    Attaching,
    Attached,
    Closed,
}

/// In-flight owed requests on one attached connection's current session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowRecord {
    pub connection: ConnectionId,
    pub in_flight: u64,
}

/// One machine's notice stream progress.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NoticeProgressRecord {
    pub machine: PeerId,
    pub last_issued: u64,
    pub expired_through: u64,
    /// None until the machine acknowledges its stream.
    pub applied_through: Option<u64>,
}

/// The Fleet feed's head and floors; `effective_floor` uses the object's clock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeedRecord {
    pub head: u64,
    pub retained_through: u64,
    pub effective_floor: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnersRecord {
    pub outbox: Vec<OutboxOwnerRecord>,
    pub status_owed: Vec<StatusOwedRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OutboxOwnerRecord {
    pub item: Uuid,
    /// The stored row shape: `item`, `recovered` or `unresolvable`.
    pub record_kind: String,
    /// The stored delivery status; none on a row without one.
    pub custody: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StatusOwedRecord {
    pub id: Uuid,
    pub item: Uuid,
    pub version: u64,
}

/// One row of the shell's registration table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistrationRow {
    pub connection: ConnectionId,
    pub peer: PeerId,
    pub registration: ConnectionRegistration,
}

/// Why a snapshot was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnapshotRefusal {
    /// A stored value failed to decode.
    CorruptRow {
        table: &'static str,
        key: String,
        field: &'static str,
    },
    /// The store could not be read.
    Storage(String),
    /// The shell's registrations changed across the durable read; retry.
    Unstable,
}

impl std::fmt::Display for SnapshotRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CorruptRow { table, key, field } => {
                write!(
                    formatter,
                    "snapshot: stored {table}.{field} of {key} does not decode"
                )
            }
            Self::Storage(message) => write!(formatter, "snapshot: storage: {message}"),
            Self::Unstable => write!(formatter, "snapshot: registrations changed during the read"),
        }
    }
}

/// The durable half of a snapshot, read in one transaction.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DurableSnapshot {
    pub attempts: Vec<AttemptRecord>,
    /// In-flight push and status rows per (connection, session).
    pub in_flight: Vec<(ConnectionId, Uuid, u64)>,
    pub notice_progress: Vec<NoticeProgressRecord>,
    pub feed: Option<FeedRecord>,
    pub owners: OwnersRecord,
}

/// The passive fence: the registrations read before the durable transaction
/// and again after it must match, or the snapshot is refused as unstable.
/// Memory and SQL are never read atomically, so a change between the two reads
/// would pair registrations with rows from another moment.
pub fn fenced(
    before: Vec<RegistrationRow>,
    durable: DurableSnapshot,
    after: &[RegistrationRow],
) -> Result<ObjectSnapshot, SnapshotRefusal> {
    if before != after {
        return Err(SnapshotRefusal::Unstable);
    }
    Ok(compose(before, durable))
}

/// Build the answer from the registrations read before the durable
/// transaction (the fence already checked they did not change). A window is
/// counted only for an attached connection, over the rows sent on its current
/// session; stale and other-kind rows stay visible in `attempts` only.
pub fn compose(registrations: Vec<RegistrationRow>, durable: DurableSnapshot) -> ObjectSnapshot {
    let mut sessions = Vec::with_capacity(registrations.len());
    let mut windows = Vec::new();
    for row in &registrations {
        let (state, session) = match &row.registration {
            ConnectionRegistration::Opening => (RegistrationState::Opening, None),
            ConnectionRegistration::Attaching { session_id, .. } => {
                (RegistrationState::Attaching, Some(session_id.as_uuid()))
            }
            ConnectionRegistration::Attached { session_id, .. } => {
                (RegistrationState::Attached, Some(session_id.as_uuid()))
            }
            ConnectionRegistration::Closed => (RegistrationState::Closed, None),
        };
        if let (RegistrationState::Attached, Some(session)) = (state, session) {
            let in_flight = durable
                .in_flight
                .iter()
                .filter(|(connection, sent_on, _)| {
                    *connection == row.connection && *sent_on == session
                })
                .map(|(_, _, count)| count)
                .sum();
            windows.push(WindowRecord {
                connection: row.connection,
                in_flight,
            });
        }
        sessions.push(SessionRecord {
            session,
            peer: row.peer,
            connection: row.connection,
            registration: state,
        });
    }
    ObjectSnapshot {
        attempts: durable.attempts,
        inventory: registrations.iter().map(|row| row.connection).collect(),
        sessions,
        windows,
        notice_progress: durable.notice_progress,
        feed: durable.feed,
        owners: durable.owners,
    }
}

mod durable_read;
pub(crate) use durable_read::read_durable;

#[cfg(test)]
#[path = "conformance_snapshot_test.rs"]
mod tests;
