//! Construct the queued record, durable statement and answer for a direct send.
use crate::{
    message_deadline::{cancel_due_at, delivery_window_ends_at},
    message_insert::{DirectMessageTerms, insert_message_due},
};
use central_storage::{GuardedStatement, StoreFailure};
use fleet_contracts::MailDeliveryReady;
use workspace_ids::{MessageId, RequestId, SessionRef};
use workspace_model::{
    ActorIdentityId, Timestamp,
    central_mail::{
        CentralMailAnswer, MessageBody, MessageRecord, MessageSender, MessageState, NoticeSource,
        SendMessageRequest,
    },
};

pub fn prepare_message_record(
    sender: MessageSender,
    target: SessionRef,
    body: MessageBody,
    source: Option<NoticeSource>,
    now: Timestamp,
    mint: impl FnOnce() -> Result<MessageId, StoreFailure>,
) -> Result<MessageRecord, StoreFailure> {
    Ok(MessageRecord {
        message_id: mint()?,
        sender,
        target,
        body,
        state: MessageState::Queued,
        source,
        created_at: now,
        updated_at: now,
    })
}

pub struct DirectSendPlan {
    pub insert: GuardedStatement,
    pub answer: CentralMailAnswer,
    pub delivery_ready: MailDeliveryReady,
}

pub fn plan_direct_send(
    request: &RequestId,
    actor: ActorIdentityId,
    send: SendMessageRequest,
    sent_at: Timestamp,
    accepted_at: Timestamp,
    mint: impl FnOnce() -> Result<MessageId, StoreFailure>,
) -> Result<DirectSendPlan, StoreFailure> {
    let message = prepare_message_record(
        MessageSender::Actor(actor),
        send.to,
        MessageBody::Direct {
            body: send.body,
            delivery: send.delivery,
            reply_to: send.reply_to,
        },
        None,
        accepted_at,
        mint,
    )?;
    let delivery_ready = MailDeliveryReady::from(&message);
    let insert = insert_message_due(
        &message,
        Some(&DirectMessageTerms {
            cancel_due_at: cancel_due_at(sent_at, accepted_at),
            delivery_window_ends_at: delivery_window_ends_at(sent_at, accepted_at),
            sender_request_id: request.clone(),
        }),
    )?;
    Ok(DirectSendPlan {
        insert,
        answer: CentralMailAnswer::Queued {
            message_id: message.message_id,
        },
        delivery_ready,
    })
}
