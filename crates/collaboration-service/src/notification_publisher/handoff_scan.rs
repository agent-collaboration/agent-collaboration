//! Every frozen project notification is eligible independently of a blocked earlier handoff.
use super::{BoardError, IntentHandling, ProjectId, PublisherInner, publication_epoch};
use message_board::EpochNumber;
use message_board_storage::{MailHandoff, MailHandoffItem};

impl PublisherInner {
    pub(super) async fn publish_project_handoffs(
        &self,
        project: &ProjectId,
    ) -> Result<(), BoardError> {
        let Some(store) = self
            .admission
            .registry
            .existing_project(project)
            .await
            .map_err(|_| BoardError::board_unavailable())?
        else {
            return Err(BoardError::board_unavailable());
        };
        let mut read = store
            .snapshot()
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        let mut after = read
            .mail_settled_through()
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        read.finish()
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        loop {
            if self.shutdown.is_cancelled() {
                break;
            }
            let epoch = publication_epoch(&store).await?;
            let mut read = store
                .snapshot()
                .await
                .map_err(|_| BoardError::board_unavailable())?;

            let page = read
                .mail_handoff_page(after, 100)
                .await
                .map_err(|_| BoardError::board_unavailable())?;
            read.finish()
                .await
                .map_err(|_| BoardError::board_unavailable())?;
            if page.is_empty() {
                break;
            }
            for handoff in page {
                after = handoff.sequence;
                match &handoff.item {
                    MailHandoffItem::Delivery(_) => {
                        self.publish_delivery_handoff(project, epoch, handoff)
                            .await?;
                    }
                    MailHandoffItem::CancelGroup(group) => {
                        self.expand_local_cancel_group(project, handoff.sequence, group)
                            .await?;
                        self.expand_remote_cancel_group(project, handoff.sequence, group)
                            .await?;
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) async fn publish_delivery_handoff(
        &self,
        project: &ProjectId,
        epoch: EpochNumber,
        handoff: MailHandoff,
    ) -> Result<IntentHandling, BoardError> {
        let Some(runtime) = self.mail.upgrade() else {
            return Ok(IntentHandling::Pending);
        };
        let MailHandoffItem::Delivery(envelope) = handoff.item else {
            return Err(BoardError::board_unavailable());
        };
        let fleet_sync_protocol::DeliveryEnvelope::Item(item) = *envelope else {
            return Err(BoardError::board_unavailable());
        };
        let local = item.recipient().address.endpoint.service_id == self.admission.installation;
        if !local
            && !runtime
                .acknowledged_mail_changes(project)
                .await?
                .max_handoff_seq
                .is_some_and(|through| handoff.sequence.get() <= through)
        {
            return Ok(IntentHandling::Pending);
        }
        // Decoding changes only the marker, validating and preserving the SDK digest.
        let item = serde_json::from_value(
            serde_json::to_value(item).map_err(|_| BoardError::board_unavailable())?,
        )
        .map_err(|_| BoardError::board_unavailable())?;
        if local {
            runtime
                .enqueue_local_project_item(item)
                .await
                .map_err(|_| BoardError::board_unavailable())?;
        } else {
            match runtime.enqueue_project_item(item, epoch).await {
                Ok(_) => {}
                Err(fleet_sync_delivery::DeliveryTransactionFailure::Refused(
                    fleet_sync_protocol::DeliveryRefusal::MailboxFull,
                )) => return Ok(IntentHandling::Pending),
                Err(_) => return Err(BoardError::board_unavailable()),
            }
        }
        Ok(IntentHandling::Consumed)
    }
}
