//! A frozen project notification enters local custody without consulting a relay.
use super::{
    AtomicStore, DeliveryConsumer, DeliveryError, DeliveryItem, Inbox, LocalMailTask,
    MailDeliveryRuntime, MailSender, OperationState, RecipientOutcome, RouterDeliveryTypes,
    StoreError, SyncTime, WakeScheduler,
};
use durable_steps::JournalScope;
use fleet_sync_delivery::{
    DeliveryTransactionFailure, LocalItemAdmission, classify_delivery_store_error,
    delivery_request_key, delivery_rollback,
};
use interaction_storage::{insert_mail_push_in_scope, read_mail_push_in_scope};

impl MailDeliveryRuntime {
    pub(crate) async fn enqueue_local_project_item(
        &self,
        item: DeliveryItem<RouterDeliveryTypes>,
    ) -> Result<(), DeliveryTransactionFailure> {
        let runtime = self.clone();
        let settlement = {
            let settlements = self.settlement_drives.lock().map_err(|_| {
                DeliveryTransactionFailure::OutcomeUnknown(StoreError::backend(
                    "local project settlement gate",
                ))
            })?;
            if settlements.is_closed() {
                return Err(DeliveryTransactionFailure::OutcomeUnknown(
                    StoreError::backend("local project admission is closed"),
                ));
            }
            settlements.spawn(async move { runtime.enqueue_local_project_item_owned(item).await })
        };
        settlement.await.map_err(|error| {
            DeliveryTransactionFailure::OutcomeUnknown(StoreError::backend(error))
        })?
    }

    async fn enqueue_local_project_item_owned(
        &self,
        item: DeliveryItem<RouterDeliveryTypes>,
    ) -> Result<(), DeliveryTransactionFailure> {
        if !matches!(item.sender(), MailSender::Project { .. })
            || !matches!(
                item.body(),
                board_sync_protocol::BoardDeliveryBody::Notification(_)
            )
            || item.recipient().peer != self.config.owner
            || item.recipient().address.endpoint.service_id != self.service
        {
            return Err(DeliveryTransactionFailure::Corrupt {
                field: "local project notification".to_owned(),
            });
        }
        let draft = crate::received_mail_push(&item, self.clock.now()).map_err(|error| {
            DeliveryTransactionFailure::Corrupt {
                field: error.to_string(),
            }
        })?;
        let config = self.config;
        let now = SyncTime::from_unix_millis(self.clock.now().timestamp_millis());
        let consumer = self.consumer.clone();
        let admitted = self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    let previous =
                        Inbox::<_, RouterDeliveryTypes>::new(scope, config.owner, config.limits)
                            .local_outcome(item.id())
                            .await
                            .map_err(delivery_rollback)?;
                    let mut inserted_projection = false;
                    if crate::received_mail_push::is_poll_mail(&item)
                        && interaction_storage::read_pending_poll_in_scope(scope, item.id())
                            .await
                            .map_err(StoreError::backend)?
                            .is_none()
                    {
                        match previous {
                            Some(RecipientOutcome::Held) => {
                                return Err(delivery_rollback(DeliveryError::Corrupt {
                                    field: "held Poll policy projection missing",
                                }));
                            }
                            Some(_) => {}
                            None => {
                                // enqueue_local does not invoke on_received. Like
                                // local native pushes below, its policy projection
                                // must be prepared by this admission transaction.
                                consumer
                                    .on_received(scope, &item)
                                    .await
                                    .map_err(delivery_rollback)?;
                                inserted_projection = true;
                            }
                        }
                    }
                    if let Some(draft) = draft {
                        let current = read_mail_push_in_scope(scope, &draft.push_id)
                            .await
                            .map_err(StoreError::backend)?;
                        if current.is_none() {
                            match previous {
                                Some(RecipientOutcome::Held) => {
                                    return Err(delivery_rollback(DeliveryError::Corrupt {
                                        field: "held notification projection missing",
                                    }));
                                }
                                Some(_) => {} // A terminal replay must not recreate a pruned push.
                                None => {
                                    insert_mail_push_in_scope(scope, draft)
                                        .await
                                        .map_err(StoreError::backend)?;
                                    inserted_projection = true;
                                }
                            }
                        }
                    }
                    match Inbox::<_, RouterDeliveryTypes>::new(scope, config.owner, config.limits)
                        .enqueue_local(item, &*consumer, now)
                        .await
                        .map_err(delivery_rollback)?
                    {
                        LocalItemAdmission::Attempt(attempt) => {
                            scope
                                .admit::<LocalMailTask>(
                                    &delivery_request_key(&(attempt.item, attempt.attempt))
                                        .map_err(StoreError::backend)?,
                                    &attempt,
                                )
                                .await
                                .map_err(StoreError::backend)?;
                            Ok(Some(attempt))
                        }
                        LocalItemAdmission::Replayed if inserted_projection => {
                            // Replayed pending custody with no projection is corruption,
                            // not a new receive. Roll the tentative insertion back.
                            Err(delivery_rollback(DeliveryError::Corrupt {
                                field: "pending notification projection missing",
                            }))
                        }
                        LocalItemAdmission::Replayed | LocalItemAdmission::SettledOnAdmission => {
                            Ok(None)
                        }
                    }
                })
            })
            .await
            .map_err(classify_delivery_store_error)?;
        if let Some(attempt) = admitted {
            self.host.wake_at(now).await.map_err(|error| {
                DeliveryTransactionFailure::OutcomeUnknown(StoreError::backend(error))
            })?;
            let key = delivery_request_key(&(attempt.item, attempt.attempt)).map_err(|error| {
                DeliveryTransactionFailure::OutcomeUnknown(StoreError::backend(error))
            })?;
            let state = self
                .engine
                .start_and_settle::<LocalMailTask>(&key, attempt)
                .await
                .map_err(|error| {
                    DeliveryTransactionFailure::OutcomeUnknown(StoreError::backend(error))
                })?;
            if !matches!(state.state, OperationState::Completed { .. }) {
                return Err(DeliveryTransactionFailure::OutcomeUnknown(
                    StoreError::backend("local project delivery has not settled"),
                ));
            }
        }
        Ok(())
    }
}
