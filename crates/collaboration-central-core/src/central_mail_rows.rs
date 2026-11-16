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
pub(crate) fn insert_message(
    message: &mail::MessageRecord,
) -> Result<GuardedStatement, StoreFailure> {
    let kind = match message.body {
        mail::MessageBody::Direct { .. } => "direct",
        mail::MessageBody::Notice { .. } => "notice",
        mail::MessageBody::Wake { .. } => "wake",
        mail::MessageBody::Automation { .. } => "automation",
    };
    let (state, failure) = match &message.state {
        mail::MessageState::Queued => ("queued", SqlValue::Null),
        mail::MessageState::Delivered => ("delivered", SqlValue::Null),
        mail::MessageState::Cancelled => ("cancelled", SqlValue::Null),
        mail::MessageState::Failed { reason } => ("failed", text(reason)),
    };
    let source = message.source.as_ref();
    Ok(any(
        "INSERT INTO messages(message_id,sender,target_machine_id,target_session,message_kind,body,state,failure_reason,source_change_id,recipient_actor,source_workspace_id,source_thread_id,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(source_change_id,recipient_actor) WHERE source_change_id IS NOT NULL DO NOTHING",
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
        ],
    ))
}
