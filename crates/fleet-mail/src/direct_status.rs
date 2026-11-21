//! Read the validated status of an actor's direct message.
use crate::direct_message_rows::{DirectMessageStatus, decode_direct_message};
use central_storage::{Statement, StoreFailure};
use fleet_runtime_ports::stored_values::encode_json;
use fleet_runtime_ports::{
    CentralStore, guarded_statements::text, workspace_decode_error::corrupt,
};
use workspace_ids::MessageId;
use workspace_model::{
    ActorIdentityId,
    central_mail::{MessageCancellation, MessageSender, MessageStatus},
};

/// The caller holds the central turn and has resolved the admitted caller's actor.
pub async fn direct_message_status_held(
    host: &impl CentralStore,
    actor: &ActorIdentityId,
    message_id: &MessageId,
) -> Result<MessageStatus, StoreFailure> {
    let rows = host.read(Statement {
        sql: "SELECT message_id,sender,message_kind,state,failure_reason,cancel_due_at,sender_request_id,created_at,updated_at FROM mail_messages WHERE message_id=? AND sender=? AND message_kind='direct'",
        args: vec![text(message_id), encode_json(&MessageSender::Actor(actor.clone()), "mail JSON")?],
    }).await?;
    let row = match rows.rows.as_slice() {
        [] => return Ok(MessageStatus::NotFound {}),
        [row] => row,
        _ => return Err(corrupt("mail_messages.message_id")),
    };
    let status = match decode_direct_message(&rows, row)?.status {
        DirectMessageStatus::Queued => MessageStatus::Queued {},
        DirectMessageStatus::Delivered => MessageStatus::Delivered {},
        DirectMessageStatus::Failed(reason) => MessageStatus::Failed { reason },
        DirectMessageStatus::Cancelled => MessageStatus::Cancelled {
            reason: MessageCancellation::NotDeliveredWithin15Minutes,
        },
    };
    Ok(status)
}
