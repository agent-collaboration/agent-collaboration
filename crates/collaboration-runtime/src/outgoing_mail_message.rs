//! The send boundary freezes the native message and label into the shared library item.
use board_sync_protocol::BoardDeliveryBody;
use collaboration_contracts::RouterDeliveryTypes;
use collaboration_mail_protocol::{MailSender, MessageBody};
use collaboration_protocol::{PushHeaderFacts, PushKind, PushOrigin, PushRecordDraft};
use fleet_sync_delivery::DeliveryError;
use fleet_sync_protocol::{DeliveryItem, ItemId, PeerId, Recipient};

pub fn outgoing_mail_message(
    draft: &PushRecordDraft,
    submitting_installation: &message_board::ServiceId,
    recipient_peer: PeerId,
) -> Result<DeliveryItem<RouterDeliveryTypes>, DeliveryError> {
    let invalid = || DeliveryError::Corrupt {
        field: "outgoing direct message",
    };
    draft.clone().into_pending().map_err(|_| invalid())?;
    if draft.kind != PushKind::DirectMessage {
        return Err(invalid());
    }
    let sender = match &draft.origin {
        PushOrigin::Session(sender) => {
            let sender: message_board::SessionRef =
                sender.clone().try_into().map_err(|_| invalid())?;
            if sender.endpoint.service_id != *submitting_installation {
                return Err(invalid());
            }
            MailSender::Session(sender)
        }
        PushOrigin::OwnerUnverified => MailSender::HumanUser {
            installation: submitting_installation.clone(),
        },
        PushOrigin::Router(_) => return Err(invalid()),
    };
    let PushHeaderFacts::DirectMessage {
        sender_display_name,
    } = &draft.header_facts
    else {
        return Err(invalid());
    };
    let body = MessageBody {
        text: draft
            .body
            .clone()
            .ok_or_else(invalid)?
            .try_into()
            .map_err(|_| invalid())?,
        delivery: draft.mode.ok_or_else(invalid)?.into(),
        guard: draft
            .guard
            .clone()
            .map(TryInto::try_into)
            .transpose()
            .map_err(|_| invalid())?,
        sender_display_name: sender_display_name.clone(),
        reply_to: draft
            .reply_to_push_id
            .clone()
            .map(TryInto::try_into)
            .transpose()
            .map_err(|_| DeliveryError::Corrupt {
                field: "outgoing reply item identity",
            })?,
    };
    DeliveryItem::new(
        ItemId::new(uuid::Uuid::parse_str(draft.push_id.as_str()).map_err(|_| invalid())?)
            .map_err(|_| invalid())?,
        sender,
        Recipient {
            peer: recipient_peer,
            address: draft.target.clone().try_into().map_err(|_| invalid())?,
        },
        None,
        BoardDeliveryBody::Message(body),
    )
    .map_err(|_| invalid())
}
