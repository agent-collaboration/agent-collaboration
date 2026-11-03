//! A Fleet receipt follows committed recipient custody and its native delivery admission.
use super::{
    AtomicStore, Inbox, LocalMailTask, MailDeliveryRuntime, RemoveMailRecord, ReportMailTask,
    RouterDeliveryTypes, StoreError,
};
use board_sync_protocol::BoardDeliveryTypes;
use collaboration_mail_protocol::CarrierAcceptance;
use fleet_sync_protocol::{DeliveryEnvelope, PushResult};
use runtime_primitives::{Clock, WakeScheduler};

impl MailDeliveryRuntime {
    pub async fn accept_pushed_mail(
        &self,
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
        let config = self.config;
        let relay = session.relay;
        let consumer = self.consumer.clone();
        let now = self.host.now();
        let answer = self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    Inbox::<_, RouterDeliveryTypes>::new(scope, config.owner, config.limits)
                        .accept_push::<LocalMailTask, ReportMailTask, RemoveMailRecord, _>(
                            item, relay, &*consumer, now,
                        )
                        .await
                        .map_err(fleet_sync_delivery::delivery_rollback)
                })
            })
            .await?;
        // Custody has already committed. A failed wake leaves the journal's
        // delivery obligation for recovery and cannot revoke this receipt.
        if let Err(error) = self.host.wake_at(now).await {
            tracing::warn!(%error, "pushed mail committed; runner wake failed");
        }
        Ok(answer)
    }
}

#[cfg(test)]
#[path = "pushed_mail_test.rs"]
mod tests;
