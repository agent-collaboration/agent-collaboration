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
            BoardDeliveryBody::CreateSession(_) => 64 * 1024,
            BoardDeliveryBody::RemoteCreateResult(_) => 64,
        }
    }
    // A canonical ServiceId has 36 ASCII bytes; JSON adds two quotes.
    const MAX_STATUS_ROUTE_BYTES: u32 = 38;
    type StatusRoute = message_board::ServiceId;
    type Body = BoardDeliveryBody;
    type Sender = MailSender;
    type Address = DeliveryAddress;
    type Acceptance = CarrierAcceptance;
    type LocalObservation = ();
    const MAX_LOCAL_OBSERVATION_BYTES: u32 = 4;
}

/// Cancels belong to the library's control envelope, never to an item body.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum BoardDeliveryBody {
    CreateSession(collaboration_mail_protocol::CreateSessionBody),
    RemoteCreateResult(collaboration_mail_protocol::RemoteCreateResultBody),
    Message(MessageBody),
    Notification(NotificationBody),
    Wake(collaboration_mail_protocol::WakeBody),
}

impl BoardDeliveryBody {
    pub fn validate_sender(&self, sender: &MailSender) -> Result<(), MailValidationError> {
        match self {
            Self::CreateSession(_) | Self::RemoteCreateResult(_) => {
                matches!(sender, MailSender::Fleet)
                    .then_some(())
                    .ok_or(MailValidationError { field: "sender" })
            }
            Self::Message(body) => body.validate_sender(sender),
            Self::Notification(body) => body.validate_sender(sender),
            Self::Wake(body) => body.validate_sender(sender),
        }
    }
}

/// The session variant preserves D1's bare address representation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum DeliveryAddress {
    Session(message_board::SessionRef),
    Machine {
        machine: fleet_sync_protocol::PeerId,
    },
}

impl DeliveryAddress {
    pub fn session(&self) -> Result<&message_board::SessionRef, MailValidationError> {
        match self {
            Self::Session(session) => Ok(session),
            Self::Machine { .. } => Err(MailValidationError {
                field: "recipient.address",
            }),
        }
    }
    pub fn into_session(self) -> Result<message_board::SessionRef, MailValidationError> {
        match self {
            Self::Session(session) => Ok(session),
            Self::Machine { .. } => Err(MailValidationError {
                field: "recipient.address",
            }),
        }
    }
}

impl From<message_board::SessionRef> for DeliveryAddress {
    fn from(session: message_board::SessionRef) -> Self {
        Self::Session(session)
    }
}

/// Shape authorization only; enrollment and endpoint ownership need registry evidence.
pub fn validate_delivery_address(
    sender: &MailSender,
    body: &BoardDeliveryBody,
    address: &DeliveryAddress,
) -> Result<(), MailValidationError> {
    body.validate_sender(sender)?;
    let allowed = matches!(
        (sender, body, address),
        (
            MailSender::Session(_) | MailSender::HumanUser { .. } | MailSender::Project { .. },
            BoardDeliveryBody::Message(_)
                | BoardDeliveryBody::Notification(_)
                | BoardDeliveryBody::Wake(_),
            DeliveryAddress::Session(_)
        ) | (
            MailSender::Fleet,
            BoardDeliveryBody::CreateSession(_),
            DeliveryAddress::Machine { .. }
        ) | (
            MailSender::Fleet,
            BoardDeliveryBody::RemoteCreateResult(_),
            DeliveryAddress::Session(_)
        )
    );
    allowed.then_some(()).ok_or(MailValidationError {
        field: "recipient.address",
    })
}

#[cfg(test)]
#[path = "board_delivery_test.rs"]
mod tests;
