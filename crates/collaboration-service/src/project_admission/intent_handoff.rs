//! Freeze a notification only after the project lock has resolved its recipient.
use super::{ProjectAdmissionError, ProjectWriteTransaction};
use board_sync_protocol::{BoardDeliveryBody, BoardDeliveryTypes};
use collaboration_mail_protocol::{
    MailSender, NotificationBody, NotificationSource, RecordPointer,
};
use fleet_sync_protocol::{DeliveryEnvelope, DeliveryItem, ItemId, PeerId, Recipient};
use message_board::{Identity, IntentState, MailItemRef, NotifyIntent, Record, ServiceId};
use message_board_storage::MailHandoffItem;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct IntentRecipientEnrollment {
    pub service: ServiceId,
    pub peer: PeerId,
}

#[derive(Clone, Debug, Serialize)]
pub struct IntentHandoffInput {
    pub item: ItemId,
    pub enrollment: IntentRecipientEnrollment,
}

pub(super) async fn freeze_intent_handoff(
    write: &mut ProjectWriteTransaction,
    intent: &NotifyIntent,
    recipient: &Identity,
    input: &IntentHandoffInput,
) -> Result<MailHandoffItem, ProjectAdmissionError> {
    let invalid = |field: &str| ProjectAdmissionError::InvalidRecord(field.to_owned());
    let Identity::Session { session } = recipient else {
        return Err(invalid("intent handoff requires a session recipient"));
    };
    // The publisher resolved this immutable pair outside the lock. NI-2 may
    // change the current recipient, so the lock must check the pair again.
    if session.endpoint.service_id != input.enrollment.service {
        return Err(invalid("intent recipient enrollment service"));
    }
    let project = intent
        .envelope
        .scope
        .project
        .clone()
        .ok_or_else(|| invalid("intent project"))?;
    let cause = write
        .record(&intent.cause)
        .await?
        .ok_or_else(|| invalid("intent cause"))?;
    let position = cause
        .envelope()
        .position
        .project_cursor()
        .ok_or_else(|| invalid("intent cause position"))?;
    let excerpt = match &cause {
        Record::Post(post) => post.text.as_str().chars().take(280).collect(),
        _ => {
            collaboration_runtime::received_mail_push::board_notification_summary(intent.cause.kind)
                .to_owned()
        }
    };
    let sender = MailSender::Project {
        project: project.clone(),
    };
    let body = BoardDeliveryBody::Notification(NotificationBody {
        excerpt: excerpt.try_into().map_err(|_| invalid("intent excerpt"))?,
        pointer: RecordPointer {
            project,
            subject: intent.cause.clone(),
            position,
        },
        source: NotificationSource::Intent {
            intent: intent.intent_id.clone(),
        },
    });
    body.validate_sender(&sender)
        .map_err(|_| invalid("intent sender"))?;
    let item = DeliveryItem::<BoardDeliveryTypes>::new(
        input.item,
        sender,
        Recipient {
            peer: input.enrollment.peer,
            address: session.clone(),
        },
        None,
        body,
    )
    .map_err(|_| invalid("intent frozen item"))?;
    Ok(MailHandoffItem::Delivery(Box::new(DeliveryEnvelope::Item(
        item,
    ))))
}

pub(super) async fn record_intent_handoff(
    write: &mut ProjectWriteTransaction,
    intent: &NotifyIntent,
    frozen: &MailHandoffItem,
) -> Result<(), ProjectAdmissionError> {
    let IntentState::HandedOff { item } = intent.state() else {
        return Err(ProjectAdmissionError::InvalidRecord(
            "intent handoff state".to_owned(),
        ));
    };
    let frozen_id: MailItemRef = frozen.id()?;
    if item != &frozen_id {
        return Err(ProjectAdmissionError::InvalidRecord(
            "intent handoff item reference".to_owned(),
        ));
    }
    write.append_mail_handoff(frozen).await?;
    Ok(())
}
