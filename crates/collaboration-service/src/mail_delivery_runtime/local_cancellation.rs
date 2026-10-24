//! Local control attribution, SDK custody and every attempt commit together.
use super::*;
use collaboration_mail_protocol::{CancelCause, MailItemId};
use durable_steps::JournalScope;
use fleet_sync_delivery::{
    DeliveryTransactionFailure, classify_delivery_store_error, delivery_request_key,
    delivery_rollback,
};
use fleet_sync_protocol::DeliveryCancel;
use interaction_storage::{
    delete_tentative_local_cancel_in_scope, prepare_local_cancel_in_scope,
    read_local_cancel_in_scope,
};
use runtime_primitives::WakeScheduler;

#[derive(Clone, Debug)]
pub enum LocalCancelReference {
    Local {
        reference: DeliveryItemRef,
    },
    /// The SDK retained a remote or derived canonical control with no Local requester record.
    ExistingElsewhere {
        reference: DeliveryItemRef,
    },
}

impl MailDeliveryRuntime {
    /// Internal producer entry; caller-facing requested cancellation is a separate contract.
    pub async fn cancel_local(
        &self,
        cancel: DeliveryCancel<RouterDeliveryTypes>,
        attribution: CancelCause,
    ) -> Result<LocalCancelReference, DeliveryTransactionFailure> {
        let proposed: MailItemId =
            cancel
                .id()
                .as_uuid()
                .to_string()
                .try_into()
                .map_err(|error| {
                    DeliveryTransactionFailure::OutcomeUnknown(StoreError::backend(error))
                })?;
        let target: MailItemId =
            cancel
                .target()
                .as_uuid()
                .to_string()
                .try_into()
                .map_err(|error| {
                    DeliveryTransactionFailure::OutcomeUnknown(StoreError::backend(error))
                })?;
        let config = self.config;
        let consumer = self.consumer.clone();
        let now = self.clock.now();
        #[cfg(test)]
        let admission_probe = self.cancel_admission_probe.clone();
        let (reference, has_effects) = self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    // No outbox precedes this entry: its own attribution must exist before the hook.
                    let preparation =
                        prepare_local_cancel_in_scope(scope, &proposed, &target, &attribution, now)
                            .await
                            .map_err(StoreError::backend)?;
                    #[cfg(test)]
                    if let Some(probe) = admission_probe {
                        if read_local_cancel_in_scope(scope, &proposed)
                            .await
                            .map_err(StoreError::backend)?
                            .is_none()
                        {
                            return Err(StoreError::backend(
                                "cancel crash probe reached before attribution",
                            ));
                        }
                        pause_cancel_admission(&probe).await?;
                    }
                    let admitted =
                        Inbox::<_, RouterDeliveryTypes>::new(scope, config.owner, config.limits)
                            .enqueue_local_cancel(
                                cancel,
                                &*consumer,
                                SyncTime::from_unix_millis(now.timestamp_millis()),
                            )
                            .await
                            .map_err(delivery_rollback)?;
                    let canonical: MailItemId = admitted
                        .reference
                        .item
                        .as_uuid()
                        .to_string()
                        .try_into()
                        .map_err(StoreError::backend)?;
                    if canonical != proposed {
                        delete_tentative_local_cancel_in_scope(scope, &proposed, preparation)
                            .await
                            .map_err(StoreError::backend)?;
                    }
                    for attempt in &admitted.effects.attempts {
                        scope
                            .admit::<LocalMailTask>(
                                &delivery_request_key(&(attempt.item, attempt.attempt))
                                    .map_err(StoreError::backend)?,
                                attempt,
                            )
                            .await
                            .map_err(StoreError::backend)?;
                    }
                    // Reports remain in SDK custody for the durable report drive.
                    let has_effects = !admitted.effects.attempts.is_empty()
                        || !admitted.effects.reports.is_empty();
                    let reference = if read_local_cancel_in_scope(scope, &canonical)
                        .await
                        .map_err(StoreError::backend)?
                        .is_some()
                    {
                        LocalCancelReference::Local {
                            reference: admitted.reference,
                        }
                    } else {
                        LocalCancelReference::ExistingElsewhere {
                            reference: admitted.reference,
                        }
                    };
                    Ok((reference, has_effects))
                })
            })
            .await
            .map_err(classify_delivery_store_error)?;
        if has_effects {
            self.host.wake_at(self.host.now()).await.map_err(|error| {
                DeliveryTransactionFailure::OutcomeUnknown(StoreError::backend(error))
            })?;
        }
        Ok(reference)
    }
}

#[cfg(test)]
async fn pause_cancel_admission(socket: &std::path::Path) -> Result<(), StoreError> {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let mut connection = tokio::net::UnixStream::connect(socket)
        .await
        .map_err(StoreError::backend)?;
    connection
        .write_all(b"R")
        .await
        .map_err(StoreError::backend)?;
    // The parent kills the child here; returning without that kill is a failed probe.
    let mut release = [0_u8; 1];
    connection
        .read_exact(&mut release)
        .await
        .map_err(StoreError::backend)?;
    Err(StoreError::backend(
        "cancel crash probe unexpectedly released",
    ))
}
