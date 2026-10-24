//! Requested controls reuse the existing attribution projection and SDK custody.
use super::*;
use collaboration_mail_protocol::{CancelCause, MailItemId};
use collaboration_protocol::{RequestedCancelOutcome, RequestedCancelResult};
use fleet_sync_delivery::{
    DeliveryTransactionFailure, Outbox, classify_delivery_store_error, delivery_rollback,
};
use fleet_sync_protocol::{
    CancelResult, DeliveryCancel, DeliveryRefusal, DeliveryStatus, TooLateAt,
};
use interaction_storage::{
    delete_tentative_local_cancel_in_scope, prepare_local_cancel_in_scope,
    read_local_cancel_in_scope, record_local_cancel_outcome_in_scope,
};

fn mail_id(item: ItemId) -> Result<MailItemId, StoreError> {
    item.as_uuid()
        .to_string()
        .try_into()
        .map_err(StoreError::backend)
}

impl MailDeliveryRuntime {
    pub(crate) async fn requested_cancel(
        &self,
        cancel: DeliveryCancel<RouterDeliveryTypes>,
        attribution: CancelCause,
        epoch: Option<message_board::EpochNumber>,
    ) -> Result<RequestedCancelResult, DeliveryTransactionFailure> {
        let proposed = cancel.id();
        let target = cancel.target();
        let admitted = if cancel.recipient().address.endpoint.service_id == self.service {
            match self.try_requested_local_cancel(&cancel, &attribution).await {
                Ok(super::local_cancellation::RequestedLocalCancel::Applied(reference)) => {
                    Ok(reference)
                }
                Ok(super::local_cancellation::RequestedLocalCancel::RelayRequired) => {
                    self.admit_remote_requested_cancel(cancel, attribution, epoch)
                        .await
                }
                Err(error) => Err(error),
            }
        } else {
            self.admit_remote_requested_cancel(cancel, attribution, epoch)
                .await
        };
        let reference = match admitted {
            Ok(
                LocalCancelReference::Local { reference }
                | LocalCancelReference::ExistingElsewhere { reference },
            ) => reference,
            Err(DeliveryTransactionFailure::Refused(DeliveryRefusal::CancelSenderMismatch)) => {
                return Ok(RequestedCancelResult {
                    cancel: mail_id(proposed).map_err(classify_delivery_store_error)?,
                    target: mail_id(target).map_err(classify_delivery_store_error)?,
                    outcome: RequestedCancelOutcome::Refused {
                        reason: collaboration_mail_protocol::MailRefusal::CancelSenderMismatch {
                            target: mail_id(target).map_err(classify_delivery_store_error)?,
                        },
                    },
                });
            }
            Err(error) => return Err(error),
        };
        self.requested_cancel_result(reference.item, target)
            .await
            .map_err(classify_delivery_store_error)
    }

    async fn admit_remote_requested_cancel(
        &self,
        cancel: DeliveryCancel<RouterDeliveryTypes>,
        attribution: CancelCause,
        epoch: Option<message_board::EpochNumber>,
    ) -> Result<LocalCancelReference, DeliveryTransactionFailure> {
        let relay = self
            .session
            .as_ref()
            .ok_or_else(|| {
                DeliveryTransactionFailure::OutcomeUnknown(StoreError::backend(
                    "remote cancellation is not configured",
                ))
            })?
            .relay;
        let proposed = mail_id(cancel.id()).map_err(classify_delivery_store_error)?;
        let target = mail_id(cancel.target()).map_err(classify_delivery_store_error)?;
        let reference = DeliveryItemRef {
            item: cancel.id(),
            digest: cancel.digest().clone(),
        };
        let project = match cancel.sender() {
            MailSender::Project { project } => Some(project.clone()),
            _ => None,
        };
        let retirement = fleet_sync_delivery::SubmitInput {
            item: cancel.id(),
            digest: cancel.digest().clone(),
            relay,
            mailbox: cancel.recipient().peer,
        };
        if project.is_some() != epoch.is_some() {
            return Err(DeliveryTransactionFailure::Corrupt {
                field: "requested cancel epoch".into(),
            });
        }
        let config = self.config;
        let consumer = self.consumer.clone();
        let now = self.clock.now();
        let admitted = self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    let preparation =
                        prepare_local_cancel_in_scope(scope, &proposed, &target, &attribution, now)
                            .await
                            .map_err(StoreError::backend)?;
                    let (admission, _) =
                        Outbox::<_, RouterDeliveryTypes>::new(scope, config.limits)
                            .enqueue_cancel_and_admit::<SubmitMailTask, _>(
                                cancel,
                                relay,
                                consumer.as_ref(),
                            )
                            .await
                            .map_err(delivery_rollback)?;
                    let canonical = if let Some(existing) =
                        super::project_cancellation::other_canonical_cancel(&admission)
                    {
                        existing.clone()
                    } else {
                        match &admission {
                            fleet_sync_delivery::OutboxAdmission::Refused { reason } => {
                                return Err(delivery_rollback(DeliveryError::Refused {
                                    reason: reason.clone(),
                                }));
                            }
                            _ => reference,
                        }
                    };
                    if project.is_none() {
                        super::terminal_retirement::admit_terminal_retirement(
                            scope, &admission, retirement,
                        )
                        .await?;
                    }
                    if canonical.item.as_uuid().to_string() != proposed.as_str() {
                        delete_tentative_local_cancel_in_scope(scope, &proposed, preparation)
                            .await
                            .map_err(StoreError::backend)?;
                        return Ok(LocalCancelReference::ExistingElsewhere {
                            reference: canonical,
                        });
                    }
                    if let (Some(project), Some(epoch)) = (project, epoch) {
                        super::project_admission::record_project_custody_epoch(
                            scope, &admission, &proposed, &project, epoch, now,
                        )
                        .await?;
                    }
                    project_requested_outbox_outcome(scope, canonical.item, config.limits).await?;
                    Ok(LocalCancelReference::Local {
                        reference: canonical,
                    })
                })
            })
            .await
            .map_err(classify_delivery_store_error)?;
        if let Err(error) = self.host.wake_at(self.host.now()).await {
            tracing::warn!(%error,"requested cancel committed; runtime wake failed");
        }
        Ok(admitted)
    }

    pub(crate) async fn requested_cancel_result(
        &self,
        cancel: ItemId,
        target: ItemId,
    ) -> Result<RequestedCancelResult, StoreError> {
        let id = mail_id(cancel)?;
        let target_id = mail_id(target)?;
        let saved = self
            .owner
            .local_cancel_delivery(&id)
            .await
            .map_err(StoreError::backend)?;
        if let Some(saved) = &saved
            && saved.target_item != target_id
        {
            return Err(StoreError::backend("requested cancel target differs"));
        }
        let outcome = match saved.and_then(|saved| saved.outcome) {
            Some(outcome) => requested_outcome(outcome, &target_id)?,
            None => {
                let config = self.config;
                let outcome = self
                    .store
                    .transaction(move |scope| {
                        Box::pin(async move {
                            let local = Inbox::<_, RouterDeliveryTypes>::new(
                                scope,
                                config.owner,
                                config.limits,
                            )
                            .local_outcome(cancel)
                            .await
                            .map_err(delivery_rollback)?;
                            if local.is_some() {
                                return Ok(local);
                            }
                            let status =
                                Outbox::<_, RouterDeliveryTypes>::new(scope, config.limits)
                                    .status(cancel)
                                    .await
                                    .map_err(delivery_rollback)?;
                            Ok(outbox_cancel_outcome(status.state, cancel))
                        })
                    })
                    .await?;
                match outcome {
                    Some(outcome) => requested_outcome(outcome, &target_id)?,
                    None => RequestedCancelOutcome::Pending,
                }
            }
        };
        Ok(RequestedCancelResult {
            cancel: id,
            target: target_id,
            outcome,
        })
    }
}

pub(super) async fn project_requested_outbox_outcome(
    scope: &mut TursoScope<'_, MailHost>,
    item: ItemId,
    limits: DeliveryLimits,
) -> Result<(), StoreError> {
    let id = mail_id(item)?;
    let Some(record) = read_local_cancel_in_scope(scope, &id)
        .await
        .map_err(StoreError::backend)?
    else {
        return Ok(());
    };
    let status = Outbox::<_, RouterDeliveryTypes>::new(scope, limits)
        .status(item)
        .await
        .map_err(delivery_rollback)?;
    let outcome = outbox_cancel_outcome(status.state, item);
    if let Some(outcome) = outcome {
        record_local_cancel_outcome_in_scope(scope, &id, &record.target_item, &outcome)
            .await
            .map_err(StoreError::backend)?;
    }
    Ok(())
}

fn outbox_cancel_outcome(
    state: fleet_sync_delivery::OutboxStatusState<CarrierAcceptance>,
    item: ItemId,
) -> Option<RecipientOutcome<CarrierAcceptance>> {
    let fleet_sync_delivery::OutboxStatusState::Known { status, cancel, .. } = state else {
        return None;
    };
    match cancel {
        Some(CancelResult::Prevented) => Some(RecipientOutcome::Prevented { cancel: item }),
        Some(CancelResult::TooLate { at }) => Some(RecipientOutcome::TooLate { cancel: item, at }),
        _ => match status {
            DeliveryStatus::Refused { reason } => Some(RecipientOutcome::Refused { reason }),
            _ => None,
        },
    }
}

fn requested_outcome(
    outcome: RecipientOutcome<CarrierAcceptance>,
    target: &MailItemId,
) -> Result<RequestedCancelOutcome, StoreError> {
    use collaboration_mail_protocol::MailRefusal;
    Ok(match outcome {
        RecipientOutcome::Prevented { .. } => RequestedCancelOutcome::Prevented,
        RecipientOutcome::TooLate { at, .. } => RequestedCancelOutcome::TooLate {
            at: match at {
                TooLateAt::Delivered => collaboration_mail_protocol::TooLateAt::Delivered,
                TooLateAt::Uncertain => collaboration_mail_protocol::TooLateAt::Uncertain,
            },
        },
        RecipientOutcome::Refused { reason } => RequestedCancelOutcome::Refused {
            reason: match reason {
                DeliveryRefusal::CancelSenderMismatch => MailRefusal::CancelSenderMismatch {
                    target: target.clone(),
                },
                DeliveryRefusal::MailboxFull => MailRefusal::MailboxFull {},
                DeliveryRefusal::RecipientUnknown => MailRefusal::RecipientUnknown {},
                DeliveryRefusal::RecipientRevoked => MailRefusal::RecipientRevoked {},
                DeliveryRefusal::DroppedByPolicy => MailRefusal::DroppedByPolicy {},
                DeliveryRefusal::ItemConflict { existing } => MailRefusal::ItemConflict {
                    existing: existing
                        .as_str()
                        .to_owned()
                        .try_into()
                        .map_err(StoreError::backend)?,
                },
                _ => {
                    return Err(StoreError::backend(
                        "requested cancellation has an internal refusal; inspect SDK status",
                    ));
                }
            },
        },
        RecipientOutcome::Held => RequestedCancelOutcome::Pending,
        _ => {
            return Err(StoreError::backend(
                "requested cancellation has an invalid item outcome",
            ));
        }
    })
}
