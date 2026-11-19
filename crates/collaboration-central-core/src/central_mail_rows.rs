//! Pure mail-row encoding shared by central request paths and the engine; it
//! names no runtime, so the engine can use it without a module cycle.
use crate::guarded_statements::{any, text};
use crate::workspace_decode_error::corrupt;
use central_storage::{BatchOutcome, GuardedStatement, SqlValue, StoreFailure};
use serde::Serialize;
use workspace_model::Timestamp;
use workspace_model::central_mail as mail;
pub(crate) fn encode(value: &impl Serialize) -> Result<SqlValue, StoreFailure> {
    serde_json::to_string(value)
        .map(text)
        .map_err(|_| corrupt("mail JSON"))
}
pub(crate) fn timestamp(value: Timestamp) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
pub(crate) fn applied(outcome: BatchOutcome) -> Result<(), StoreFailure> {
    match outcome {
        BatchOutcome::Applied { .. } => Ok(()),
        _ => Err(StoreFailure::Unavailable),
    }
}
/// Central-authored notices, outcomes, wakes and automation messages.
pub(crate) fn insert_message(
    message: &mail::MessageRecord,
) -> Result<GuardedStatement, StoreFailure> {
    insert_message_due(message, None)
}

/// Delivery and cancellation deadlines for a direct message, and the sender's
/// original outbox request for correlating an outcome.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DirectMessageTerms {
    pub cancel_due_at: Timestamp,
    pub delivery_window_ends_at: Timestamp,
    pub sender_request_id: workspace_model::RequestId,
}

/// Encode a queued message with the deadline supplied by its creation path.
pub(crate) fn insert_message_due(
    message: &mail::MessageRecord,
    direct: Option<&DirectMessageTerms>,
) -> Result<GuardedStatement, StoreFailure> {
    let kind = stored_kind(&message.body);
    let (state, failure) = match &message.state {
        mail::MessageState::Queued => ("queued", SqlValue::Null),
        mail::MessageState::Delivered => ("delivered", SqlValue::Null),
        mail::MessageState::Cancelled => ("cancelled", SqlValue::Null),
        mail::MessageState::Failed { reason } => ("failed", text(reason)),
    };
    let source = message.source.as_ref();
    let delivery_window = match (&message.body, direct) {
        (mail::MessageBody::Direct { .. }, Some(terms)) => Some(terms.delivery_window_ends_at),
        (mail::MessageBody::Notice { .. } | mail::MessageBody::DeliveryOutcome { .. }, None) => {
            Some(
                message
                    .created_at
                    .checked_add_signed(crate::message_deadline::SYSTEM_MESSAGE_DELIVERY_WINDOW)
                    .ok_or_else(|| corrupt("mail_messages.delivery_window_ends_at"))?,
            )
        }
        (mail::MessageBody::Wake { .. } | mail::MessageBody::Automation { .. }, None) => None,
        _ => return Err(corrupt("mail_messages.delivery_window_ends_at")),
    };
    Ok(any(
        "INSERT INTO mail_messages(message_id,sender,target_machine_id,target_session,message_kind,body,state,failure_reason,source_change_id,recipient_actor,source_workspace_id,source_thread_id,created_at,updated_at,cancel_due_at,sender_request_id,delivery_window_ends_at) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(source_change_id,recipient_actor) WHERE source_change_id IS NOT NULL DO NOTHING",
        vec![
            text(&message.message_id),
            encode(&message.sender)?,
            text(&message.target.machine_id),
            encode(&message.target)?,
            text(kind),
            encode(&message.body)?,
            text(state),
            failure,
            source.map_or(SqlValue::Null, |source| text(&source.change_id)),
            source.map_or(SqlValue::Null, |source| text(&source.recipient_actor)),
            source.map_or(SqlValue::Null, |source| text(&source.workspace_id)),
            source
                .and_then(|source| source.thread_id.as_ref())
                .map_or(SqlValue::Null, text),
            text(timestamp(message.created_at)),
            text(timestamp(message.updated_at)),
            direct.map_or(SqlValue::Null, |terms| text(timestamp(terms.cancel_due_at))),
            direct.map_or(SqlValue::Null, |terms| text(&terms.sender_request_id)),
            delivery_window.map_or(SqlValue::Null, |due| text(timestamp(due))),
        ],
    ))
}

/// The `mail_messages.message_kind` text for a body; stored rows must match it.
pub(crate) fn stored_kind(body: &mail::MessageBody) -> &'static str {
    match body {
        mail::MessageBody::Direct { .. } => "direct",
        mail::MessageBody::Notice { .. } => "notice",
        mail::MessageBody::Wake { .. } => "wake",
        mail::MessageBody::Automation { .. } => "automation",
        mail::MessageBody::DeliveryOutcome { .. } => "deliveryOutcome",
    }
}
