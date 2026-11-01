//! The SDK's public attempt transitions reserve a listing's members before its native effect.
use super::{
    AtomicStore, Inbox, LocalMailTask, MailDeliveryRuntime, RouterDeliveryTypes, StoreError,
    SyncTime,
};
use collaboration_protocol::PushRecord;
use durable_steps::storage::{JournalStorage, OperationExecution};
use fleet_sync_delivery::{DeliveryAttempt, DeliveryCheck};
use fleet_sync_protocol::{DeliveryItem, DeliveryItemRef};

impl MailDeliveryRuntime {
    pub(crate) async fn prepare_notification_for_listing(
        &self,
        reference: DeliveryItemRef,
    ) -> Result<
        (
            DeliveryItem<RouterDeliveryTypes>,
            collaboration_runtime::MailPushPreparation,
        ),
        StoreError,
    > {
        let config = self.config;
        let item = self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    Inbox::<_, RouterDeliveryTypes>::new(scope, config.owner, config.limits)
                        .resolve_delivery(&reference)
                        .await
                        .map_err(StoreError::backend)
                })
            })
            .await?;
        let preparation = self
            .consumer
            .carrier
            .prepare_notification_for_listing(&item)
            .await
            .map_err(StoreError::backend)?;
        Ok((item, preparation))
    }
    pub(crate) async fn reserve_notification_listing(
        &self,
        listing: &PushRecord,
    ) -> Result<(), StoreError> {
        let id = listing.push_id.clone();
        let config = self.config;
        let now = SyncTime::from_unix_millis(self.clock.now().timestamp_millis());
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    let members =
                        interaction_storage::prepare_notification_listing_in_scope(scope, &id)
                            .await
                            .map_err(StoreError::backend)?;
                    for member in members {
                        let Some(reference) = member.reference else {
                            continue;
                        };
                        let admitted = Inbox::<_, RouterDeliveryTypes>::new(
                            scope,
                            config.owner,
                            config.limits,
                        )
                        .release_held::<LocalMailTask>(reference.item, now)
                        .await
                        .map_err(StoreError::backend)?
                        .ok_or_else(|| {
                            StoreError::backend("notification member is no longer held")
                        })?;
                        let operation = scope
                            .operation(admitted.id())
                            .await?
                            .ok_or(StoreError::MissingOperation { id: admitted.id() })?;
                        let OperationExecution::Running { input, .. } = operation.execution else {
                            return Err(StoreError::backend(
                                "notification reservation is already terminal",
                            ));
                        };
                        let attempt: DeliveryAttempt =
                            serde_json::from_slice(&input).map_err(StoreError::backend)?;
                        if attempt.item != reference.item {
                            return Err(StoreError::backend(
                                "notification reservation identity differs",
                            ));
                        }
                        match Inbox::<_, RouterDeliveryTypes>::new(
                            scope,
                            config.owner,
                            config.limits,
                        )
                        .check_delivery(attempt, now)
                        .await
                        .map_err(StoreError::backend)?
                        {
                            DeliveryCheck::Deliver {
                                reference: reserved,
                            } if reserved == reference => {}
                            _ => {
                                return Err(StoreError::backend(
                                    "notification member is not eligible for native delivery",
                                ));
                            }
                        }
                    }
                    Ok(())
                })
            })
            .await
    }
    pub(crate) async fn wake_notification_callbacks(&self) -> Result<(), StoreError> {
        self.host
            .wake_at(SyncTime::from_unix_millis(
                self.clock.now().timestamp_millis(),
            ))
            .await
            .map_err(StoreError::backend)
    }
}
use runtime_primitives::WakeScheduler;
