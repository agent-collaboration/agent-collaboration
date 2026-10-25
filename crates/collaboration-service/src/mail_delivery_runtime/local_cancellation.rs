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

const RELAY_REQUIRED: &str = "router.local_cancel_requires_relay";

#[derive(Clone, Copy)]
enum LocalCancelMode {
    Apply,
    RouteRelayedOriginal,
}

pub(super) enum RequestedLocalCancel {
    Applied(LocalCancelReference),
    RelayRequired,
}

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
        let (reference, has_effects) = self
            .admit_local_cancel(cancel, attribution, LocalCancelMode::Apply)
            .await
            .map_err(classify_delivery_store_error)?;
        self.wake_local_cancel(reference, has_effects).await
    }

    pub(super) async fn try_requested_local_cancel(
        &self,
        cancel: &DeliveryCancel<RouterDeliveryTypes>,
        attribution: &CancelCause,
    ) -> Result<RequestedLocalCancel, DeliveryTransactionFailure> {
        let local = DeliveryCancel::new(
            cancel.id(),
            cancel.sender().clone(),
            cancel.recipient().clone(),
            cancel.valid_until(),
            fleet_sync_protocol::CancelBody {
                target: cancel.target(),
            },
        )
        .map_err(|error| classify_delivery_store_error(StoreError::backend(error)))?;
        match self
            .admit_local_cancel(
                local,
                attribution.clone(),
                LocalCancelMode::RouteRelayedOriginal,
            )
            .await
        {
            Ok((reference, has_effects)) => self
                .wake_local_cancel(reference, has_effects)
                .await
                .map(RequestedLocalCancel::Applied),
            Err(StoreError::StepRollback { failure }) if matches!(failure.as_ref(), durable_steps::OperationFailure::Refused { code, .. } if code == RELAY_REQUIRED) =>
            {
                // Only the adapter's final error proves the whole attempted local
                // cancellation rolled back, including attribution and outcome writes.
                Ok(RequestedLocalCancel::RelayRequired)
            }
            Err(error) => Err(classify_delivery_store_error(error)),
        }
    }

    async fn admit_local_cancel(
        &self,
        cancel: DeliveryCancel<RouterDeliveryTypes>,
        attribution: CancelCause,
        mode: LocalCancelMode,
    ) -> Result<(LocalCancelReference, bool), StoreError> {
        let original = cancel.target();
        let proposed = local_cancel_mail_id(cancel.id())?;
        let target = local_cancel_mail_id(original)?;
        let config = self.config;
        let consumer = self.consumer.clone();
        let now = self.clock.now();
        #[cfg(test)]
        let admission_probe = self.cancel_admission_probe.clone();
        self.store
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
                    ensure_local_cancel_route(mode, &admitted.effects, original)?;
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
                    admit_cancel_effects(scope, &admitted.effects).await?;
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
    }

    async fn wake_local_cancel(
        &self,
        reference: LocalCancelReference,
        has_effects: bool,
    ) -> Result<LocalCancelReference, DeliveryTransactionFailure> {
        if has_effects {
            self.host.wake_at(self.host.now()).await.map_err(|error| {
                DeliveryTransactionFailure::OutcomeUnknown(StoreError::backend(error))
            })?;
        }
        Ok(reference)
    }
}

fn local_cancel_mail_id(item: ItemId) -> Result<MailItemId, StoreError> {
    item.as_uuid()
        .to_string()
        .try_into()
        .map_err(StoreError::backend)
}

fn ensure_local_cancel_route(
    mode: LocalCancelMode,
    effects: &fleet_sync_delivery::InboxEffects,
    original: ItemId,
) -> Result<(), StoreError> {
    if matches!(mode, LocalCancelMode::RouteRelayedOriginal)
        && effects.reports.iter().any(|report| report.item == original)
    {
        // The SDK owns the route. An original's report proves relayed custody;
        // the normal central control admission must precede its prevention.
        return Err(StoreError::StepRollback {
            failure: Box::new(durable_steps::OperationFailure::Refused {
                code: RELAY_REQUIRED.into(),
                detail: String::new(),
            }),
        });
    }
    Ok(())
}

async fn admit_cancel_effects(
    scope: &mut TursoScope<'_, MailHost>,
    effects: &fleet_sync_delivery::InboxEffects,
) -> Result<(), StoreError> {
    for attempt in &effects.attempts {
        scope
            .admit::<LocalMailTask>(
                &delivery_request_key(&(attempt.item, attempt.attempt))
                    .map_err(StoreError::backend)?,
                attempt,
            )
            .await
            .map_err(StoreError::backend)?;
    }
    for report in &effects.reports {
        scope
            .admit::<ReportMailTask>(
                &delivery_request_key(&(report.item, report.report_seq))
                    .map_err(StoreError::backend)?,
                report,
            )
            .await
            .map_err(StoreError::backend)?;
    }
    Ok(())
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
