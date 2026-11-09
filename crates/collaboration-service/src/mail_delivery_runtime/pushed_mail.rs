//! A Fleet receipt follows committed recipient custody and its native delivery admission.
use super::{
    AtomicStore, Inbox, LocalMailTask, MailDeliveryRuntime, RemoveMailRecord, ReportMailTask,
    RouterDeliveryTypes, StoreError,
};
use board_sync_protocol::BoardDeliveryTypes;
use collaboration_mail_protocol::CarrierAcceptance;
use fleet_protocol::{DeliveryEnvelope, PushResult};
use runtime_primitives::{Clock, WakeScheduler};

impl MailDeliveryRuntime {
    #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
    pub async fn runtime_test_pushed_item(
        &self,
        reference: fleet_protocol::DeliveryItemRef,
    ) -> Result<fleet_protocol::DeliveryItem<BoardDeliveryTypes>, StoreError> {
        let config = self.config;
        let item = self
            .store
            .transaction({
                let binding = self.binding.clone();
                move |scope| {
                    Box::pin(async move {
                        Inbox::<_, RouterDeliveryTypes, _>::new(
                            scope,
                            config.owner,
                            config.limits,
                            binding.as_ref(),
                        )
                        .resolve_delivery(&reference)
                        .await
                        .map_err(StoreError::backend)
                    })
                }
            })
            .await?;
        serde_json::from_value(serde_json::to_value(item).map_err(StoreError::backend)?)
            .map_err(StoreError::backend)
    }

    pub async fn accept_pushed_mail(
        &self,
        submitter: fleet_protocol::Submitter,
        item: DeliveryEnvelope<BoardDeliveryTypes>,
    ) -> Result<PushResult<CarrierAcceptance>, StoreError> {
        let session = self.session.as_ref().ok_or_else(|| {
            StoreError::backend("Fleet Push requires an attached machine session")
        })?;
        // RouterDeliveryTypes changes only local observation. Decode through the
        // same validated envelope contract instead of duplicating its fields.
        let item: DeliveryEnvelope<RouterDeliveryTypes> =
            serde_json::from_value(serde_json::to_value(item).map_err(StoreError::backend)?)
                .map_err(StoreError::backend)?;
        let sender_service = match item.sender() {
            super::MailSender::Session(session) => Some(session.endpoint.service_id.clone()),
            super::MailSender::HumanUser { installation } => Some(installation.clone()),
            super::MailSender::Project { .. } => None,
        };
        let wire = serde_json::to_vec(&item).map_err(StoreError::backend)?;
        let config = self.config;
        let relay = session.relay;
        loop {
            let consumer = self.consumer.clone();
            let binding = self.binding.clone();
            let pushed = serde_json::from_slice(&wire).map_err(StoreError::backend)?;
            let now = self.host.now();
            let answer = self
                .store
                .transaction(move |scope| {
                    Box::pin(async move {
                        Inbox::<_, RouterDeliveryTypes, _>::new(
                            scope,
                            config.owner,
                            config.limits,
                            binding.as_ref(),
                        )
                        .accept_push::<LocalMailTask, ReportMailTask, RemoveMailRecord, _>(
                            submitter,
                            pushed,
                            relay,
                            consumer.as_ref(),
                            now,
                        )
                        .await
                        .map_err(fleet_mail_delivery::delivery_rollback)
                    })
                })
                .await?;
            if matches!(
                answer,
                PushResult::BindingUnavailable {
                    reason: fleet_protocol::SenderBindingFailure::Unknown
                }
            ) && let Some(service) = &sender_service
                && self.resolve_installation_peer(service).await.is_ok()
            {
                continue;
            }
            // Only committed custody admits work; the network receipt follows it.
            if matches!(
                answer,
                PushResult::Accepted { .. } | PushResult::Replayed { .. }
            ) && let Err(error) = self.host.wake_at(now).await
            {
                tracing::warn!(%error, "pushed mail committed; runner wake failed");
            }
            return Ok(answer);
        }
    }
}

#[cfg(test)]
#[path = "pushed_mail_test.rs"]
mod tests;
