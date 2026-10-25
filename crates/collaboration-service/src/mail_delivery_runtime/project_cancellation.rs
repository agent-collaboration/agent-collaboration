//! Frozen project controls share the original's atomic SDK custody and epoch boundary.
use super::*;
use fleet_sync_delivery::{
    DeliveryTransactionFailure, Outbox, OutboxAdmission, classify_delivery_store_error,
    delivery_rollback,
};
use fleet_sync_protocol::{DeliveryCancel, DeliveryRefusal, DeliveryStatus};
use message_board::EpochNumber;

pub(crate) fn other_canonical_cancel(
    admission: &OutboxAdmission<CarrierAcceptance>,
) -> Option<&DeliveryItemRef> {
    match admission {
        OutboxAdmission::Replayed {
            status:
                DeliveryStatus::Refused {
                    reason: DeliveryRefusal::CancelAlreadyRequested { existing },
                },
            ..
        }
        | OutboxAdmission::CancelAlreadyRequested {
            status:
                DeliveryStatus::Refused {
                    reason: DeliveryRefusal::CancelAlreadyRequested { existing },
                },
            ..
        }
        | OutboxAdmission::Refused {
            reason: DeliveryRefusal::CancelAlreadyRequested { existing },
        } => Some(existing),
        OutboxAdmission::CancelAlreadyRequested { existing, .. } => Some(existing),
        _ => None,
    }
}

impl MailDeliveryRuntime {
    pub(crate) async fn enqueue_project_cancel(
        &self,
        cancel: DeliveryCancel<RouterDeliveryTypes>,
        epoch: EpochNumber,
    ) -> Result<OutboxAdmission<CarrierAcceptance>, DeliveryTransactionFailure> {
        let MailSender::Project { project } = cancel.sender() else {
            return Err(DeliveryTransactionFailure::Corrupt {
                field: "project cancel sender".into(),
            });
        };
        let relay = self
            .session
            .as_ref()
            .ok_or_else(|| {
                DeliveryTransactionFailure::OutcomeUnknown(StoreError::backend(
                    "project cancel requires its configured relay",
                ))
            })?
            .relay;
        let project = project.clone();
        let id = cancel
            .id()
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
        let probe = self.project_admission_probe.clone();
        let (admission, has_effects) = self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    let (admission, operation) =
                        Outbox::<_, RouterDeliveryTypes>::new(scope, config.limits)
                            .enqueue_cancel_and_admit::<SubmitMailTask, _>(
                                cancel, relay, &*consumer,
                            )
                            .await
                            .map_err(delivery_rollback)?;
                    #[cfg(test)]
                    if let Some(socket) = probe {
                        super::project_admission::pause_project_admission(&socket).await?;
                    }
                    super::project_admission::record_project_custody_epoch(
                        scope, &admission, &id, &project, epoch, now,
                    )
                    .await?;
                    Ok((admission, operation.is_some()))
                })
            })
            .await
            .map_err(classify_delivery_store_error)?;
        if has_effects {
            self.host.wake_at(self.host.now()).await.map_err(|error| {
                DeliveryTransactionFailure::OutcomeUnknown(StoreError::backend(error))
            })?;
        }
        Ok(admission)
    }
}
