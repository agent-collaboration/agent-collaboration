//! Prepare application projections before local custody can record an outcome.
use super::{
    AtomicStore, Clock, Inbox, LocalMailTask, MailDeliveryRuntime, MailHost, OperationState,
    PushMessageSendResult, PushRecordDraft, RouterDeliveryTypes, StoreError, SyncTime, TursoScope,
    outgoing_mail_message,
};
#[cfg(test)]
use super::{CarrierAcceptance, DeliveryError, DeliveryItemRef, ItemId, RecipientOutcome};
use crate::push_record_delivery::PushDeliveryFailure;
use collaboration_protocol::{MailItemId, MessageSendResult};
use durable_steps::JournalScope;
use fleet_sync_delivery::{
    DeliveryAttempt, DeliveryTransactionFailure, LocalItemAdmission, classify_delivery_store_error,
    delivery_request_key, delivery_rollback,
};
use fleet_sync_protocol::DeliveryRefusal;
use interaction_storage::{
    insert_mail_push_in_scope, read_message_send_answer_in_scope, record_message_send_admission,
};
use runtime_primitives::WakeScheduler;

enum LocalAdmission {
    Attempt(DeliveryAttempt),
    Replayed(Option<PushMessageSendResult>),
    Settled(PushMessageSendResult),
}

impl MailDeliveryRuntime {
    #[cfg(test)]
    pub(crate) async fn local_custody_is_absent_for_test(
        &self,
        reference: DeliveryItemRef,
    ) -> Result<bool, StoreError> {
        let config = self.config;
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    // The public API's Absent result establishes no custody row exists.
                    // Refusals write nothing; an unexpected retirement rolls back so
                    // this probe cannot erase the evidence of a bad admission.
                    match Inbox::<_, RouterDeliveryTypes>::new(scope, config.owner, config.limits)
                        .retire_local(&reference)
                        .await
                        .map_err(StoreError::backend)?
                    {
                        fleet_sync_delivery::LocalRetirement::Absent => Ok(true),
                        fleet_sync_delivery::LocalRetirement::Refused(_) => Ok(false),
                        fleet_sync_delivery::LocalRetirement::Retired => Err(StoreError::backend(
                            "absence probe found unexpected terminal custody",
                        )),
                    }
                })
            })
            .await
    }

    #[cfg(test)]
    pub(crate) async fn local_outcome_for_test(
        &self,
        item: ItemId,
    ) -> Result<Option<RecipientOutcome<CarrierAcceptance>>, StoreError> {
        let config = self.config;
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    Inbox::<_, RouterDeliveryTypes>::new(scope, config.owner, config.limits)
                        .local_outcome(item)
                        .await
                        .map_err(StoreError::backend)
                })
            })
            .await
    }
    #[cfg(test)]
    pub(crate) async fn local_payload_is_compacted_for_test(
        &self,
        reference: DeliveryItemRef,
    ) -> Result<bool, StoreError> {
        let config = self.config;
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    let mut inbox =
                        Inbox::<_, RouterDeliveryTypes>::new(scope, config.owner, config.limits);
                    if !matches!(
                        inbox
                            .local_outcome(reference.item)
                            .await
                            .map_err(StoreError::backend)?,
                        Some(RecipientOutcome::Delivered { .. })
                    ) {
                        return Ok(false);
                    }
                    match inbox.resolve_delivery(&reference).await {
                        Err(DeliveryError::CustodyMissing) => Ok(true),
                        Ok(_) => Ok(false),
                        Err(error) => Err(StoreError::backend(error)),
                    }
                })
            })
            .await
    }

    pub(crate) async fn send_local(
        &self,
        draft: PushRecordDraft,
        target_identity: String,
    ) -> Result<PushMessageSendResult, PushDeliveryFailure> {
        let push = draft.push_id.clone();
        let runtime = self.clone();
        let settlement = {
            let settlements = self
                .settlement_drives
                .lock()
                .map_err(|_| PushDeliveryFailure::DeliveryUnavailable)?;
            if settlements.is_closed() {
                return Err(PushDeliveryFailure::DeliveryUnavailable);
            }
            settlements.spawn(async move { runtime.send_local_owned(draft, target_identity).await })
        };
        // The runtime owns this task. Dropping the RPC's handle cancels only its wait.
        settlement
            .await
            .map_err(|_| PushDeliveryFailure::DeliveryUnknown(push))?
    }

    async fn send_local_owned(
        &self,
        draft: PushRecordDraft,
        target_identity: String,
    ) -> Result<PushMessageSendResult, PushDeliveryFailure> {
        draft
            .clone()
            .into_pending()
            .map_err(PushDeliveryFailure::InvalidRecord)?;
        let push_id = draft.push_id.clone();
        let item_id: MailItemId = push_id
            .clone()
            .try_into()
            .map_err(|_| PushDeliveryFailure::StoreFailed)?;
        let item = outgoing_mail_message(&draft, &self.service, self.config.owner)
            .map_err(|_| PushDeliveryFailure::StoreFailed)?;
        let config = self.config;
        let now = self.clock.now();
        let consumer = self.consumer.clone();
        let admission = self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    let item_id = draft
                        .push_id
                        .clone()
                        .try_into()
                        .map_err(StoreError::backend)?;
                    // These writes are tentative until the library's identity/digest check
                    // and the rest of this same owner transaction commit.
                    prepare_send_projections(scope, &item_id, draft, &target_identity, now).await?;
                    let admission =
                        Inbox::<_, RouterDeliveryTypes>::new(scope, config.owner, config.limits)
                            .enqueue_local(
                                item,
                                &*consumer,
                                SyncTime::from_unix_millis(now.timestamp_millis()),
                            )
                            .await
                            .map_err(delivery_rollback)?;
                    match admission {
                        LocalItemAdmission::Attempt(input) => {
                            scope
                                .admit::<LocalMailTask>(
                                    &delivery_request_key(&(input.item, input.attempt))
                                        .map_err(StoreError::backend)?,
                                    &input,
                                )
                                .await
                                .map_err(StoreError::backend)?;
                            Ok(LocalAdmission::Attempt(input))
                        }
                        LocalItemAdmission::Replayed => Ok(LocalAdmission::Replayed(
                            read_local_answer(scope, &item_id).await?,
                        )),
                        LocalItemAdmission::SettledOnAdmission => {
                            // The one SDK hook already saw both projections. Never call it again.
                            let answer =
                                read_local_answer(scope, &item_id).await?.ok_or_else(|| {
                                    StoreError::backend(
                                        "settled local admission has no recorded answer",
                                    )
                                })?;
                            Ok(LocalAdmission::Settled(answer))
                        }
                    }
                })
            })
            .await
            .map_err(local_admission_failure)?;
        let input = match admission {
            LocalAdmission::Settled(result) | LocalAdmission::Replayed(Some(result)) => {
                return Ok(result);
            }
            LocalAdmission::Replayed(None) => {
                return Err(PushDeliveryFailure::DeliveryUnknown(push_id));
            }
            LocalAdmission::Attempt(input) => input,
        };
        // Only an actual attempt wakes or drives the native-delivery task.
        self.host
            .wake_at(self.host.now())
            .await
            .map_err(|_| PushDeliveryFailure::DeliveryUnknown(push_id.clone()))?;
        let key = delivery_request_key(&(input.item, input.attempt))
            .map_err(|_| PushDeliveryFailure::DeliveryUnknown(push_id.clone()))?;
        let status = self
            .engine
            .start_and_settle::<LocalMailTask>(&key, input)
            .await
            .map_err(|_| PushDeliveryFailure::DeliveryUnknown(push_id.clone()))?;
        match status.state {
            OperationState::Completed { .. } => {
                let answer = self
                    .owner
                    .message_send_answer(&item_id)
                    .await
                    .map_err(|_| PushDeliveryFailure::DeliveryUnknown(push_id.clone()))?
                    .and_then(|answer| answer.result);
                match answer {
                    Some(MessageSendResult::Local(result)) => Ok(result),
                    _ => Err(PushDeliveryFailure::DeliveryUnknown(push_id)),
                }
            }
            OperationState::Failed { reason } => {
                tracing::warn!(
                    push_id = push_id.as_str(),
                    ?reason,
                    "local mail operation failed; inspect its recorded outcome"
                );
                Err(PushDeliveryFailure::DeliveryUnknown(push_id))
            }
            OperationState::Running { .. } => Err(PushDeliveryFailure::DeliveryUnknown(push_id)),
        }
    }
}

async fn prepare_send_projections(
    scope: &mut TursoScope<'_, MailHost>,
    item: &MailItemId,
    draft: PushRecordDraft,
    target_identity: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), StoreError> {
    // The non-pruned answer marks an existing projection set; retain its first
    // label, answer and clock, and never recreate a pruned push on replay.
    // This does not establish same-content replay: only the SDK may do that below.
    if read_message_send_answer_in_scope(scope, item)
        .await
        .map_err(StoreError::backend)?
        .is_none()
    {
        insert_mail_push_in_scope(scope, draft)
            .await
            .map_err(StoreError::backend)?;
        record_message_send_admission(scope, item, target_identity, now)
            .await
            .map_err(StoreError::backend)?;
    }
    Ok(())
}

async fn read_local_answer(
    scope: &mut TursoScope<'_, MailHost>,
    item: &MailItemId,
) -> Result<Option<PushMessageSendResult>, StoreError> {
    let answer = read_message_send_answer_in_scope(scope, item)
        .await
        .map_err(StoreError::backend)?
        .ok_or_else(|| StoreError::backend("local custody has no send answer"))?;
    match answer.result {
        Some(MessageSendResult::Local(result)) => Ok(Some(result)),
        Some(MessageSendResult::Queued { .. }) => {
            Err(StoreError::backend("local custody has a remote answer"))
        }
        None => Ok(None),
    }
}

pub(super) fn local_admission_failure(error: StoreError) -> PushDeliveryFailure {
    match classify_delivery_store_error(error) {
        DeliveryTransactionFailure::Refused(reason) => map_refusal(reason),
        DeliveryTransactionFailure::Corrupt { .. }
        | DeliveryTransactionFailure::OutcomeUnknown(_) => PushDeliveryFailure::StoreFailed,
    }
}
fn map_refusal(reason: DeliveryRefusal) -> PushDeliveryFailure {
    use collaboration_mail_protocol::MailRefusal;
    match reason {
        DeliveryRefusal::ItemConflict { existing } => match existing.as_str().to_owned().try_into()
        {
            Ok(existing) => {
                PushDeliveryFailure::MailRefused(MailRefusal::ItemConflict { existing })
            }
            Err(_) => PushDeliveryFailure::StoreFailed,
        },
        DeliveryRefusal::MailboxFull => {
            PushDeliveryFailure::MailRefused(MailRefusal::MailboxFull {})
        }
        _ => PushDeliveryFailure::StoreFailed,
    }
}
