use crate::{
    ArchiveSeen, BoardAnnotation, BoardDeliveryTypes, BoardPayload, BoardReadCursor, BoardReadPage,
    BoardReadScope, FleetRequestEnvelope, ForwardOutcome, ForwardedPost, PositionedBoardNotice,
    VerifiedForwardedPost,
};
use collaboration_mail_protocol::{MailSender, MailValidationError};
use fleet_sync_protocol::{ConsumerProtocol, DeliveryEnvelope, Epoch, PeerId};
use serde::{Deserialize, Serialize};

pub struct BoardConsumerProtocol;

impl ConsumerProtocol for BoardConsumerProtocol {
    type Delivery = BoardDeliveryTypes;
    type Request = BoardConsumerRequest;
    type Answer = BoardConsumerAnswer;
    type ServerRequest = BoardServerRequest;
    type ClientAnswer = BoardClientAnswer;
    type Notice = PositionedBoardNotice;
    type Annotation = BoardAnnotation;
}

#[derive(Serialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BoardConsumerRequest {
    RenewMachineToken(fleet_identity_protocol::RenewMachineToken),
    ForwardedPost {
        project: message_board::ProjectId,
        post: ForwardedPost,
    },
    MailSubmit {
        item: Box<DeliveryEnvelope<BoardDeliveryTypes>>,
        origin_epoch: Option<Epoch>,
    },
    Fleet(FleetRequestEnvelope),
    Read {
        scope: BoardReadScope,
        cursor: Option<BoardReadCursor>,
    },
    ArchiveSeen(ArchiveSeen),
}

// Keep unchecked decoding private: the public wire shape is fixed by PD4, but
// an origin epoch is meaningful only for project mail.
#[derive(Deserialize)]
#[serde(
    remote = "BoardConsumerRequest",
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum BoardConsumerRequestFields {
    RenewMachineToken(fleet_identity_protocol::RenewMachineToken),
    ForwardedPost {
        project: message_board::ProjectId,
        post: ForwardedPost,
    },
    MailSubmit {
        item: Box<DeliveryEnvelope<BoardDeliveryTypes>>,
        origin_epoch: Option<Epoch>,
    },
    Fleet(FleetRequestEnvelope),
    Read {
        scope: BoardReadScope,
        cursor: Option<BoardReadCursor>,
    },
    ArchiveSeen(ArchiveSeen),
}

impl BoardConsumerRequest {
    /// The installation and peer must come from the same registry enrollment row.
    /// Wire decoding alone cannot verify that external binding.
    pub fn mail_submit(
        item: DeliveryEnvelope<BoardDeliveryTypes>,
        origin_epoch: Option<Epoch>,
        recipient_installation: &message_board::ServiceId,
        recipient_peer: PeerId,
    ) -> Result<Self, MailValidationError> {
        validate_mail_submit(&item, origin_epoch)?;
        if item.recipient().address.endpoint.service_id != *recipient_installation {
            return Err(MailValidationError {
                field: "recipient.address.endpoint.serviceId",
            });
        }
        if item.recipient().peer != recipient_peer {
            return Err(MailValidationError {
                field: "recipient.peer",
            });
        }
        Ok(Self::MailSubmit {
            item: Box::new(item),
            origin_epoch,
        })
    }
}

fn validate_mail_submit(
    item: &DeliveryEnvelope<BoardDeliveryTypes>,
    origin_epoch: Option<Epoch>,
) -> Result<(), MailValidationError> {
    if let DeliveryEnvelope::Item(item) = item {
        item.body().validate_sender(item.sender())?;
    }
    match (item.sender(), origin_epoch) {
        (MailSender::Project { .. }, Some(_))
        | (MailSender::Session(_) | MailSender::HumanUser { .. }, None) => Ok(()),
        (MailSender::Project { .. }, None)
        | (MailSender::Session(_) | MailSender::HumanUser { .. }, Some(_)) => {
            Err(MailValidationError {
                field: "originEpoch",
            })
        }
    }
}

impl<'de> Deserialize<'de> for BoardConsumerRequest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let request = BoardConsumerRequestFields::deserialize(deserializer)?;
        if let Self::MailSubmit { item, origin_epoch } = &request {
            validate_mail_submit(item, *origin_epoch).map_err(serde::de::Error::custom)?;
        }
        Ok(request)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BoardConsumerAnswer {
    RenewMachineToken(fleet_identity_protocol::MachineTokenRenewal),
    ForwardedPost(ForwardOutcome),
    MailSubmit(MailSubmitAnswer),
    Fleet {
        request_id: message_board::RequestId,
        decision: BoardPayload,
    },
    Read(BoardReadPage),
    ArchiveSeen(BoardPayload),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MailSubmitAnswer {
    Submitted(
        fleet_sync_protocol::SubmitResult<
            <BoardDeliveryTypes as fleet_sync_protocol::DeliveryTypes>::Acceptance,
        >,
    ),
    MailboxUnavailable {
        reason: fleet_sync_protocol::DeliveryRefusal,
    },
    NotWriter,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum BoardServerRequest {
    ForwardedPost(VerifiedForwardedPost),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum BoardClientAnswer {
    ForwardedPost(BoardPayload),
}
