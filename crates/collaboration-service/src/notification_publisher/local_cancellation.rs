//! The publisher freezes local child controls; the native owner only asks before injection.
use super::{BoardError, NotificationPublisher, ProjectId, PublisherInner};
use board_sync_protocol::BoardDeliveryBody;
use collaboration_mail_protocol::{CancelCause, NotificationSource};
use fleet_sync_protocol::{DeliveryEnvelope, DeliveryItemRef};
use message_board::{HandoffSeq, MailItemRef};
use message_board_storage::{MailCancelGroup, MailHandoffItem};

impl NotificationPublisher {
    /// True means a real covering control is durable, so the caller must not inject.
    pub(crate) async fn before_local_follow_injection(
        &self,
        project: &ProjectId,
        reference: &DeliveryItemRef,
        target: &collaboration_protocol::SessionRef,
    ) -> Result<bool, BoardError> {
        let target: message_board::SessionRef = target
            .clone()
            .try_into()
            .map_err(|_| BoardError::board_unavailable())?;
        // A remote inbox need not hold the sender's project. Only a surviving
        // local frozen handoff can name a same-machine board-derived group.
        let Some(store) = self
            .inner
            .admission
            .registry
            .existing_project(project)
            .await
            .map_err(|_| BoardError::board_unavailable())?
        else {
            return Ok(false);
        };
        let id: MailItemRef = reference
            .item
            .as_uuid()
            .to_string()
            .try_into()
            .map_err(|_| BoardError::board_unavailable())?;
        let mut read = store
            .snapshot()
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        let Some(handoff) = read
            .mail_handoff(&id)
            .await
            .map_err(|_| BoardError::board_unavailable())?
        else {
            read.finish()
                .await
                .map_err(|_| BoardError::board_unavailable())?;
            return Ok(false);
        };
        let MailHandoffItem::Delivery(envelope) = handoff.item else {
            return Err(BoardError::board_unavailable());
        };
        if envelope.digest() != &reference.digest
            || envelope.recipient().address != target
            || target.endpoint.service_id != self.inner.admission.installation
        {
            return Err(BoardError::board_unavailable());
        }
        let DeliveryEnvelope::Item(item) = &*envelope else {
            return Err(BoardError::board_unavailable());
        };
        let BoardDeliveryBody::Notification(notification) = item.body() else {
            return Err(BoardError::board_unavailable());
        };
        let mut after = handoff.sequence;
        let mut covering = None;
        loop {
            let page = read
                .mail_handoff_page(after, 100)
                .await
                .map_err(|_| BoardError::board_unavailable())?;
            if page.is_empty() {
                break;
            }
            for candidate in page {
                after = candidate.sequence;
                if let MailHandoffItem::CancelGroup(group) = candidate.item
                    && group_covers(&group, &notification.source)
                {
                    covering = Some(group.id);
                    break;
                }
            }
            if covering.is_some() {
                break;
            }
        }
        read.finish()
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        let Some(group) = covering else {
            return Ok(false);
        };
        self.inner
            .persist_local_cancel(project, &group, &id)
            .await?;
        Ok(true)
    }
}

impl PublisherInner {
    pub(super) async fn expand_local_cancel_group(
        &self,
        project: &ProjectId,
        sequence: HandoffSeq,
        group: &MailCancelGroup,
    ) -> Result<(), BoardError> {
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
                break;
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
                let BoardDeliveryBody::Notification(notification) = item.body() else {
                    continue;
                };
                if item.recipient().address.endpoint.service_id == self.admission.installation
                    && group_covers(group, &notification.source)
                {
                    let id = item
                        .id()
                        .as_uuid()
                        .to_string()
                        .try_into()
                        .map_err(|_| BoardError::board_unavailable())?;
                    self.persist_local_cancel(project, &group.id, &id).await?;
                }
            }
        }
        Ok(())
    }

    async fn persist_local_cancel(
        &self,
        project: &ProjectId,
        group: &MailItemRef,
        target: &MailItemRef,
    ) -> Result<(), BoardError> {
        let runtime = self
            .mail
            .upgrade()
            .ok_or_else(BoardError::board_unavailable)?;
        let (_, child) = self.freeze_cancel_expansion(project, group, target).await?;
        if child
            .expansion
            .envelope
            .recipient()
            .address
            .endpoint
            .service_id
            != self.admission.installation
        {
            return Err(BoardError::board_unavailable());
        }
        let DeliveryEnvelope::Cancel(cancel) = *child.expansion.envelope else {
            return Err(BoardError::board_unavailable());
        };
        let cancel = serde_json::from_value(
            serde_json::to_value(cancel).map_err(|_| BoardError::board_unavailable())?,
        )
        .map_err(|_| BoardError::board_unavailable())?;
        runtime
            .cancel_local(cancel, CancelCause::BoardDerived {})
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        Ok(())
    }
}

pub(super) fn group_covers(group: &MailCancelGroup, source: &NotificationSource) -> bool {
    matches!(source, NotificationSource::FollowBatch { membership, activated_at, generation, .. }
        if membership == &group.membership && activated_at == &group.activated_at && generation == &group.generation)
}
