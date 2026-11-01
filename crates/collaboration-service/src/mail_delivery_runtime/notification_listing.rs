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
    ) -> Result<Option<PushRecord>, StoreError> {
        let id = listing.push_id.clone();
        let config = self.config;
        let reserved_at = self.clock.now();
        let now = SyncTime::from_unix_millis(reserved_at.timestamp_millis());
        let listing = listing.clone();
        let machine = self
            .service
            .as_str()
            .to_owned()
            .try_into()
            .map_err(StoreError::backend)?;
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    let members =
                        interaction_storage::prepare_notification_listing_in_scope(scope, &id)
                            .await
                            .map_err(StoreError::backend)?;
                    let mut retained = Vec::new();
                    for member in members {
                        let Some(reference) = member.reference else {
                            if member.record.delivery_state
                                == collaboration_protocol::PushDeliveryState::Held
                            {
                                retained.push(member.record);
                            }
                            continue;
                        };
                        let admitted = Inbox::<_, RouterDeliveryTypes>::new(
                            scope,
                            config.owner,
                            config.limits,
                        )
                        .release_held::<LocalMailTask>(reference.item, now)
                        .await
                        .map_err(StoreError::backend)?;
                        let Some(admitted) = admitted else {
                            continue;
                        };
                        let operation = scope
                            .operation(admitted.id())
                            .await?
                            .ok_or(StoreError::MissingOperation { id: admitted.id() })?;
                        let OperationExecution::Running { input, .. } = operation.execution else {
                            continue;
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
                            } if reserved == reference => retained.push(member.record),
                            DeliveryCheck::Deliver { .. } => {
                                return Err(StoreError::backend(
                                    "notification reservation reference differs",
                                ));
                            }
                            _ => {}
                        }
                    }
                    let draft = if retained.is_empty() {
                        None
                    } else {
                        Some(
                            crate::notification_listing_content::draft_for_members(
                                machine,
                                &listing.target,
                                id.clone(),
                                &retained,
                                listing.created_at,
                            )
                            .map_err(StoreError::backend)?,
                        )
                    };
                    let retained_ids = retained
                        .iter()
                        .map(|record| record.push_id.clone())
                        .collect::<Vec<_>>();
                    interaction_storage::refreeze_notification_listing_in_scope(
                        scope,
                        &id,
                        draft,
                        &retained_ids,
                        reserved_at,
                    )
                    .await
                    .map_err(StoreError::backend)
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
