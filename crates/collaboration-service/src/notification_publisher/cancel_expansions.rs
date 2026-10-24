//! One project transaction freezes each group-target association and its SDK envelope.
use super::*;
use board_sync_protocol::BoardDeliveryTypes;
use fleet_sync_protocol::{CancelBody, DeliveryCancel, DeliveryEnvelope, ItemId};
use message_board::{EpochNumber, MailItemRef};
use message_board_storage::{MailCancelExpansion, MailHandoffItem, SequencedMailCancelExpansion};

impl PublisherInner {
    pub(super) async fn freeze_cancel_expansion(
        &self,
        project: &ProjectId,
        group: &MailItemRef,
        target: &MailItemRef,
    ) -> Result<(EpochNumber, SequencedMailCancelExpansion), BoardError> {
        let store = self
            .admission
            .registry
            .existing_project(project)
            .await
            .map_err(|_| BoardError::board_unavailable())?
            .ok_or_else(BoardError::board_unavailable)?;
        let mut write = store
            .begin_admission(self.admission.record_origin())
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        let epoch = write
            .admitted_epoch()
            .map_err(|_| BoardError::board_unavailable())?;
        if let Some(existing) = write
            .mail_cancel_expansion_for_target(group, target)
            .await
            .map_err(|_| BoardError::board_unavailable())?
        {
            write
                .rollback()
                .await
                .map_err(|_| BoardError::board_unavailable())?;
            return Ok((epoch, existing));
        }
        let handoff = write
            .mail_handoff(target)
            .await
            .map_err(|_| BoardError::board_unavailable())?
            .ok_or_else(BoardError::board_unavailable)?;
        let MailHandoffItem::Delivery(envelope) = handoff.item else {
            return Err(BoardError::board_unavailable());
        };
        let expansion = MailCancelExpansion {
            group_handoff: group.clone(),
            target_item: target.clone(),
            envelope: Box::new(DeliveryEnvelope::Cancel(
                DeliveryCancel::<BoardDeliveryTypes>::new(
                    ItemId::new(uuid::Uuid::now_v7())
                        .map_err(|_| BoardError::board_unavailable())?,
                    envelope.sender().clone(),
                    envelope.recipient().clone(),
                    None,
                    CancelBody {
                        target: envelope.id(),
                    },
                )
                .map_err(|_| BoardError::board_unavailable())?,
            )),
        };
        let sequence = write
            .append_mail_cancel_expansion(&expansion)
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        write
            .commit()
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        self.admission.registry.notify_activity(project);
        Ok((
            epoch,
            SequencedMailCancelExpansion {
                sequence,
                expansion,
            },
        ))
    }
}
