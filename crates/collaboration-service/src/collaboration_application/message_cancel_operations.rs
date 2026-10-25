//! A sender requests one SDK control using its Router's frozen routing projection.
use super::*;
use collaboration_mail_protocol::{CancelCause, MailSender};
use collaboration_protocol::{MessageCancelRequest, MessageCancelSender, RequestedCancelResult};
use fleet_sync_protocol::{CancelBody, DeliveryCancel, ItemId, Recipient};

impl MessageOperations<'_> {
    pub async fn message_cancel(
        &self,
        request: MessageCancelRequest,
    ) -> Result<RequestedCancelResult, MessageFailure> {
        let stage = MessageFailureStage::Discovery;
        let store = self.push_store(stage)?;
        let mail = self.identity.mail_delivery.as_ref().ok_or_else(|| {
            MessageFailure::discovery(
                MessageFailureKind::Unavailable,
                "Message cancellation is unavailable",
            )
        })?;
        let sender = match request.sender {
            MessageCancelSender::Agent { sender } => {
                if sender.endpoint.service_id != self.identity.service_id {
                    return Err(MessageFailure::discovery(
                        MessageFailureKind::WrongService,
                        "Cancel sender belongs to another Router",
                    ));
                }
                MailSender::Session(sender.try_into().map_err(|_| {
                    MessageFailure::discovery(
                        MessageFailureKind::InvalidField,
                        "Invalid cancel sender",
                    )
                })?)
            }
            MessageCancelSender::HumanUser => MailSender::HumanUser {
                installation: String::from(self.identity.service_id.clone())
                    .try_into()
                    .map_err(|_| {
                        MessageFailure::discovery(
                            MessageFailureKind::InvalidField,
                            "Invalid installation",
                        )
                    })?,
            },
        };
        let target = request.target.clone();
        let queued = store.message_send_answer(&target).await.map_err(|_| {
            MessageFailure::discovery(
                MessageFailureKind::Unavailable,
                "Sender routing could not be read",
            )
        })?;
        let recipient = if let Some(MessageSendResult::Queued { queued }) =
            queued.and_then(|saved| saved.result)
        {
            let service = String::from(queued.target.endpoint.service_id.clone())
                .try_into()
                .map_err(|_| {
                    MessageFailure::discovery(
                        MessageFailureKind::InvalidField,
                        "Invalid recipient service",
                    )
                })?;
            let peer = mail
                .resolve_installation_peer(&service)
                .await
                .map_err(|_| {
                    MessageFailure::discovery(
                        MessageFailureKind::Unavailable,
                        "Recipient enrollment is unavailable",
                    )
                })?;
            Recipient {
                peer,
                address: queued.target.try_into().map_err(|_| {
                    MessageFailure::discovery(MessageFailureKind::InvalidField, "Invalid recipient")
                })?,
            }
        } else {
            let push: PushId = target
                .clone()
                .try_into()
                .map_err(|_| MessageFailure::not_found(stage))?;
            let original = store
                .get_push_record(&push)
                .await
                .map_err(|_| {
                    MessageFailure::discovery(
                        MessageFailureKind::Unavailable,
                        "Message routing could not be read",
                    )
                })?
                .ok_or_else(|| MessageFailure::not_found(stage))?;
            if original.kind != PushKind::DirectMessage {
                return Err(MessageFailure::discovery(
                    MessageFailureKind::NotDirectMessage,
                    "Use project notification cancellation for a notification",
                ));
            }
            Recipient {
                peer: self.identity.installation_peer.ok_or_else(|| {
                    MessageFailure::discovery(
                        MessageFailureKind::Unavailable,
                        "Installation identity is unavailable",
                    )
                })?,
                address: original.target.try_into().map_err(|_| {
                    MessageFailure::discovery(MessageFailureKind::InvalidField, "Invalid recipient")
                })?,
            }
        };
        let id = request
            .cancel_id
            .unwrap_or_else(collaboration_protocol::MailItemId::generate);
        let parse = |id: &collaboration_protocol::MailItemId| -> Result<ItemId, MessageFailure> {
            ItemId::new(uuid::Uuid::parse_str(id.as_str()).map_err(|_| {
                MessageFailure::discovery(MessageFailureKind::InvalidShape, "Invalid item identity")
            })?)
            .map_err(|_| {
                MessageFailure::discovery(MessageFailureKind::InvalidShape, "Invalid item identity")
            })
        };
        let cancel = DeliveryCancel::new(
            parse(&id)?,
            sender,
            recipient,
            None,
            CancelBody {
                target: parse(&target)?,
            },
        )
        .map_err(|_| {
            MessageFailure::discovery(MessageFailureKind::InvalidField, "Invalid requested cancel")
        })?;
        mail.requested_cancel(cancel, CancelCause::BySender {}, None)
            .await
            .map_err(|error| match error {
                fleet_sync_delivery::DeliveryTransactionFailure::Refused(
                    fleet_sync_protocol::DeliveryRefusal::ItemConflict { existing },
                ) => {
                    let mut failure = MessageFailure::discovery(
                        MessageFailureKind::ItemConflict,
                        "Cancel identity belongs to different content",
                    );
                    let Ok(existing) = existing.as_str().to_owned().try_into() else {
                        return MessageFailure::discovery(
                            MessageFailureKind::Unavailable,
                            "Stored cancel conflict digest is invalid",
                        );
                    };
                    failure.existing = Some(existing);
                    failure
                }
                fleet_sync_delivery::DeliveryTransactionFailure::Refused(
                    fleet_sync_protocol::DeliveryRefusal::MailboxFull,
                ) => MessageFailure::discovery(
                    MessageFailureKind::MailboxFull,
                    "Requested cancellation mailbox is full",
                ),
                fleet_sync_delivery::DeliveryTransactionFailure::OutcomeUnknown(_) => {
                    MessageFailure::discovery(
                        MessageFailureKind::OutcomeUnknown,
                        "Cancellation outcome is unknown; inspect the cancel ID before retrying",
                    )
                }
                _ => MessageFailure::discovery(
                    MessageFailureKind::Unavailable,
                    "Requested cancellation is unavailable",
                ),
            })
    }
}
