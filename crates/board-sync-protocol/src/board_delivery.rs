use collaboration_mail_protocol::{
    CarrierAcceptance, MailSender, MailValidationError, MessageBody, NotificationBody,
};
use fleet_sync_protocol::DeliveryTypes;
use serde::{Deserialize, Serialize};

pub struct BoardDeliveryTypes;

impl DeliveryTypes for BoardDeliveryTypes {
    // Session IDs have at most 4096 UTF-8 bytes. Even worst-case JSON escaping
    // fits these bounds, including the endpoint and the sender's variant tag.
    const MAX_SENDER_BYTES: u32 = 32 * 1024;
    const MAX_ADDRESS_BYTES: u32 = 32 * 1024;
    // Remote wake answers carry a bounded complete facade result plus its wrapper.
    const MAX_ACCEPTANCE_BYTES: u32 = 1024 * 1024 + 1024;
    const PER_ITEM_SETTLEMENT: bool = true;
    fn max_acceptance_bytes_for(body: &Self::Body) -> u32 {
        match body {
            BoardDeliveryBody::Message(_) | BoardDeliveryBody::Notification(_) => 64,
            BoardDeliveryBody::Wake(_) => Self::MAX_ACCEPTANCE_BYTES,
        }
    }
    // A canonical ServiceId has 36 ASCII bytes; JSON adds two quotes.
    const MAX_STATUS_ROUTE_BYTES: u32 = 38;
    type StatusRoute = message_board::ServiceId;
    type Body = BoardDeliveryBody;
    type Sender = MailSender;
    type Address = message_board::SessionRef;
    type Acceptance = CarrierAcceptance;
    type LocalObservation = ();
    const MAX_LOCAL_OBSERVATION_BYTES: u32 = 4;
}

/// Cancels belong to the library's control envelope, never to an item body.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum BoardDeliveryBody {
    Message(MessageBody),
    Notification(NotificationBody),
    Wake(collaboration_mail_protocol::WakeBody),
}

impl BoardDeliveryBody {
    pub fn validate_sender(&self, sender: &MailSender) -> Result<(), MailValidationError> {
        match self {
            Self::Message(body) => body.validate_sender(sender),
            Self::Notification(body) => body.validate_sender(sender),
            Self::Wake(body) => body.validate_sender(sender),
        }
    }
}
