//! Host notification that a committed mail row can be offered on a live link.
use workspace_ids::{MachineId, MessageId};
use workspace_model::central_mail::MessageRecord;

/// An in-memory hint. The durable row remains the delivery source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MailDeliveryReady {
    pub machine_id: MachineId,
    pub message_id: MessageId,
}

impl From<&MessageRecord> for MailDeliveryReady {
    fn from(message: &MessageRecord) -> Self {
        Self {
            machine_id: message.target.machine_id.clone(),
            message_id: message.message_id.clone(),
        }
    }
}
