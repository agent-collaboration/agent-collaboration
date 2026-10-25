//! Remote controls wait for both the group and each frozen association's actual ACK mark.
use super::*;
use board_sync_protocol::BoardDeliveryBody;
use fleet_sync_delivery::DeliveryTransactionFailure;
use fleet_sync_protocol::{DeliveryEnvelope, DeliveryRefusal};
use message_board::HandoffSeq;
use message_board_storage::{MailCancelGroup, MailHandoffItem};

impl PublisherInner {
    pub(super) async fn expand_remote_cancel_group(
        &self,
        project: &ProjectId,
        sequence: HandoffSeq,
        group: &MailCancelGroup,
    ) -> Result<(), BoardError> {
        let runtime = self
            .mail
            .upgrade()
            .ok_or_else(BoardError::board_unavailable)?;
        if !runtime
            .acknowledged_mail_changes(project)
            .await?
            .max_handoff_seq
            .is_some_and(|through| sequence.get() <= through)
        {
            return Ok(());
        }
        let store = self
            .admission
            .registry
            .existing_project(project)
            .await
            .map_err(|_| BoardError::board_unavailable())?
            .ok_or_else(BoardError::board_unavailable)?;
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
                return Ok(());
            }
            for handoff in page {
                if handoff.sequence >= sequence {
                    return Ok(());
                }
                after = handoff.sequence;
                let MailHandoffItem::Delivery(envelope) = handoff.item else {
                    continue;
                };
                let DeliveryEnvelope::Item(item) = &*envelope else {
                    continue;
                };
                let BoardDeliveryBody::Notification(body) = item.body() else {
                    continue;
                };
                if item.recipient().address.endpoint.service_id == self.admission.installation
                    || !super::local_cancellation::group_covers(group, &body.source)
                {
                    continue;
                }
                let target = envelope
                    .id()
                    .as_uuid()
                    .to_string()
                    .try_into()
                    .map_err(|_| BoardError::board_unavailable())?;
                let (epoch, child) = self
                    .freeze_cancel_expansion(project, &group.id, &target)
                    .await?;
                if !runtime
                    .acknowledged_mail_changes(project)
                    .await?
                    .max_expansion_seq
                    .is_some_and(|through| child.sequence.get() <= through)
                {
                    continue;
                }
                let DeliveryEnvelope::Cancel(cancel) = *child.expansion.envelope else {
                    return Err(BoardError::board_unavailable());
                };
                let proposed = cancel.id();
                let cancel = serde_json::from_value(
                    serde_json::to_value(cancel).map_err(|_| BoardError::board_unavailable())?,
                )
                .map_err(|_| BoardError::board_unavailable())?;
                match runtime.enqueue_project_cancel(cancel, epoch).await {
                    Ok(admission) => {
                        if let Some(existing) =
                            crate::mail_delivery_runtime::other_canonical_cancel(&admission)
                        {
                            tracing::debug!(child = %proposed.as_uuid(), canonical = %existing.item.as_uuid(),
                                "another canonical cancel does not establish this frozen child's custody");
                        }
                    }
                    Err(DeliveryTransactionFailure::Refused(
                        DeliveryRefusal::CancelAlreadyRequested { existing },
                    )) => {
                        tracing::debug!(child = %proposed.as_uuid(), canonical = %existing.item.as_uuid(), "frozen child refused by an earlier canonical cancel");
                    }
                    // Keep the association; a later status, project-activity or Welcome hint retries it.
                    Err(DeliveryTransactionFailure::Refused(DeliveryRefusal::MailboxFull)) => {
                        return Ok(());
                    }
                    Err(_) => return Err(BoardError::board_unavailable()),
                }
            }
        }
    }
}
