//! Live publications and Welcome catch-up commit before central is acknowledged.
use super::*;
use fleet_sync_delivery::{Outbox, delivery_rollback};
use fleet_sync_protocol::{
    DeliveryAnswer, DeliveryRequest, ItemId, OperationAnswer, OperationRequest, StatusAckResult,
    StatusPublication,
};
use runtime_primitives::{IdSource, OperationId, StepName, UuidV7Ids};
use std::collections::BTreeSet;

pub(super) fn status_call(step: &str) -> Result<CallId, StoreError> {
    Ok(CallId {
        operation: OperationId::new(UuidV7Ids.new_id()).map_err(StoreError::backend)?,
        step: StepName::new(step).map_err(StoreError::backend)?,
    })
}

impl MailDeliveryRuntime {
    pub(crate) async fn outbox_status(
        &self,
        item: ItemId,
    ) -> Result<fleet_sync_delivery::OutboxStatusState<CarrierAcceptance>, StoreError> {
        let limits = self.config.limits;
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    Outbox::<_, RouterDeliveryTypes>::new(scope, limits)
                        .status(item)
                        .await
                        .map(|status| status.state)
                        .map_err(delivery_rollback)
                })
            })
            .await
    }
    pub async fn catch_up_status(&self) -> Result<(), StoreError> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| StoreError::backend("remote mail session is not attached"))?;
        let limits = self.config.limits;
        loop {
            let after = self
                .store
                .transaction(move |scope| {
                    Box::pin(async move {
                        Outbox::<_, RouterDeliveryTypes>::new(scope, limits)
                            .status_cursor()
                            .await
                            .map_err(delivery_rollback)
                    })
                })
                .await?;
            // Network I/O never holds the native owner's transaction.
            let batch = match session
                .requests
                .request(OperationRequest::Delivery(DeliveryRequest::StatusFetch {
                    call: status_call("mail-status-fetch")?,
                    after,
                    limit: 100,
                }))
                .await
                .map_err(StoreError::backend)?
            {
                OperationAnswer::Delivery(DeliveryAnswer::StatusFetch(batch)) => batch,
                _ => return Err(StoreError::backend("mail status fetch was not answered")),
            };
            let empty = batch.events.is_empty();
            let through = batch.through;
            let items: Vec<_> = batch.events.iter().map(|event| event.item).collect();
            self.store
                .transaction(move |scope| {
                    Box::pin(async move {
                        Outbox::<_, RouterDeliveryTypes>::new(scope, limits)
                            .apply_status_batch(after, batch)
                            .await
                            .map_err(delivery_rollback)?;
                        Ok(())
                    })
                })
                .await?;
            self.prompt_project_coverage(&items).await;
            // Empty pages also retry the acknowledgement of this committed prefix.
            match session
                .requests
                .request(OperationRequest::Delivery(DeliveryRequest::StatusAck {
                    call: status_call("mail-status-ack")?,
                    through,
                }))
                .await
                .map_err(StoreError::backend)?
            {
                OperationAnswer::Delivery(DeliveryAnswer::StatusAck(
                    StatusAckResult::Acknowledged { .. },
                )) => {}
                _ => {
                    return Err(StoreError::backend(
                        "mail status prefix was not acknowledged",
                    ));
                }
            }
            if empty {
                return Ok(());
            }
        }
    }

    pub async fn publish_status(
        &self,
        publication: StatusPublication<CarrierAcceptance>,
    ) -> Result<(), StoreError> {
        let limits = self.config.limits;
        let item = publication.item;
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    // This applies one canonical item only. The SDK catch-up cursor
                    // belongs to apply_status_batch and is never advanced by a live hint.
                    Outbox::<_, RouterDeliveryTypes>::new(scope, limits)
                        .publish_status(publication)
                        .await
                        .map_err(delivery_rollback)?;
                    Ok(())
                })
            })
            .await?;
        self.prompt_project_coverage(&[item]).await;
        Ok(())
    }

    async fn prompt_project_coverage(&self, items: &[ItemId]) {
        let mut projects = BTreeSet::new();
        for item in items {
            let Ok(item) = item.as_uuid().to_string().try_into() else {
                tracing::warn!("committed mail status has an invalid item reference");
                continue;
            };
            match self.owner.mail_submission_epoch(&item).await {
                Ok(Some(epoch)) => {
                    projects.insert(epoch.project);
                }
                Ok(None) => {} // Direct mail has no project submission projection.
                Err(error) => {
                    tracing::warn!(%error, "committed mail status project hint delayed");
                }
            }
        }
        self.reader_actors
            .prompt_follow_coverage(&projects.into_iter().collect::<Vec<_>>())
            .await;
    }
}
