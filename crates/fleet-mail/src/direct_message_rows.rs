//! A direct message row as its delivery reducer sees it, decoded without any
//! runtime so the engine and the request paths share it with no module cycle.
use central_storage::{ReadOutcome, SqlValue, StoreFailure};
use fleet_runtime_ports::{
    stored_values::{column, json, string},
    workspace_decode_error::corrupt,
};
use workspace_ids::SessionRef;
use workspace_ids::{MessageId, RequestId};
use workspace_model::central_mail as mail;
use workspace_model::{Identity, Timestamp};

/// The columns a direct message's reducer reads; they are also its version.
pub(crate) const DIRECT_MESSAGE_COLUMNS: &str = "SELECT message_id,sender,message_kind,state,failure_reason,cancel_due_at,sender_request_id,created_at,updated_at FROM mail_messages WHERE message_id=?";

/// Where a direct message stands in `messages.state`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DirectMessageStatus {
    Queued,
    Delivered,
    Cancelled,
    Failed(mail::MessageFailureReason),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DirectMessageRow {
    pub id: MessageId,
    pub sender: workspace_model::ActorIdentityId,
    pub sender_request_id: RequestId,
    pub cancel_due_at: Timestamp,
    pub status: DirectMessageStatus,
}

fn parse<T: std::str::FromStr>(value: &str, field: &str) -> Result<T, StoreFailure> {
    value.parse().map_err(|_| corrupt(field))
}

fn present<'a>(
    rows: &'a ReadOutcome,
    row: &'a [SqlValue],
    field: &str,
) -> Result<&'a str, StoreFailure> {
    match column(rows, row, field)? {
        SqlValue::Text(value) => Ok(value),
        _ => Err(corrupt(field)),
    }
}

/// Decodes one row read with [`DIRECT_MESSAGE_COLUMNS`]. Anything that is not
/// a direct message from an actor, with a cancel mark on the 5-minute grid after
/// its creation and the sender's request id, is corrupt, never coerced.
pub(crate) fn decode_direct_message(
    rows: &ReadOutcome,
    row: &[SqlValue],
) -> Result<DirectMessageRow, StoreFailure> {
    if string(rows, row, "message_kind")? != "direct" {
        return Err(corrupt("mail_messages.message_kind"));
    }
    let mail::MessageSender::Actor(sender) =
        json(string(rows, row, "sender")?, "mail_messages.sender")?
    else {
        return Err(corrupt("mail_messages.sender"));
    };
    let created_at: Timestamp =
        parse(string(rows, row, "created_at")?, "mail_messages.created_at")?;
    let cancel_due_at: Timestamp = parse(
        present(rows, row, "cancel_due_at")?,
        "mail_messages.cancel_due_at",
    )?;
    if !central_engine_model::supported_time(cancel_due_at)
        || !central_engine_model::on_five_minute_grid(cancel_due_at)
        || cancel_due_at <= created_at
    {
        return Err(corrupt("mail_messages.cancel_due_at"));
    }
    let status = match (
        string(rows, row, "state")?,
        column(rows, row, "failure_reason")?,
    ) {
        ("queued", SqlValue::Null) => DirectMessageStatus::Queued,
        ("delivered", SqlValue::Null) => DirectMessageStatus::Delivered,
        ("cancelled", SqlValue::Null) => DirectMessageStatus::Cancelled,
        ("failed", SqlValue::Text(reason)) => DirectMessageStatus::Failed(
            serde_json::from_value(serde_json::Value::String(reason.clone()))
                .map_err(|_| corrupt("mail_messages.failure_reason"))?,
        ),
        _ => return Err(corrupt("mail_messages.state")),
    };
    Ok(DirectMessageRow {
        id: parse(string(rows, row, "message_id")?, "mail_messages.message_id")?,
        sender,
        sender_request_id: parse(
            present(rows, row, "sender_request_id")?,
            "mail_messages.sender_request_id",
        )?,
        cancel_due_at,
        status,
    })
}

/// The stored `failure_reason` text for a refusal.
pub(crate) fn refusal_text(reason: mail::MessageRefusal) -> Result<String, StoreFailure> {
    failure_text(reason.into())
}

pub(crate) fn failure_text(reason: mail::MessageFailureReason) -> Result<String, StoreFailure> {
    match serde_json::to_value(reason) {
        Ok(serde_json::Value::String(text)) => Ok(text),
        _ => Err(corrupt("mail_messages.failure_reason")),
    }
}

/// The sender's session for an actor read with
/// `SELECT identity_kind,identity FROM access_actor_identities WHERE actor_identity_id=?`:
/// an agent session is told how its message ended; a person has no session.
pub(crate) fn decode_sender_session(
    rows: &ReadOutcome,
) -> Result<Option<SessionRef>, StoreFailure> {
    let [row] = rows.rows.as_slice() else {
        return Err(corrupt("mail_messages.sender"));
    };
    let identity: Identity = json(
        string(rows, row, "identity")?,
        "access_actor_identities.identity",
    )?;
    if string(rows, row, "identity_kind")? != identity.stored_kind() {
        return Err(corrupt("access_actor_identities.identity_kind"));
    }
    match identity {
        Identity::AgentSession { session } => Ok(Some(session)),
        Identity::RouterComponent { .. } | Identity::Person { .. } => Ok(None),
    }
}

#[cfg(test)]
#[path = "direct_message_rows_test.rs"]
mod tests;
