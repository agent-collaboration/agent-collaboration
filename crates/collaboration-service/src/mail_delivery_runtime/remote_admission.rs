//! Remote direct sends commit SDK custody and their replay answer together.
use super::{
    AtomicStore, Clock, DeliveryError, InstallationPeerResolutionError, MailDeliveryRuntime,
    PushRecordDraft, RouterDeliveryTypes, StoreError, SubmitMailTask, WakeScheduler,
    outgoing_mail_message,
};
use crate::push_record_delivery::PushDeliveryFailure;
use collaboration_protocol::{MailItemId, MessageSendResult, QueuedMessageSend};
use fleet_sync_delivery::{Outbox, OutboxAdmission, delivery_rollback};
use interaction_storage::{
    read_message_send_answer_in_scope, record_message_send_admission, record_message_send_outcome,
};

impl MailDeliveryRuntime {
    pub(crate) async fn send_remote(
        &self,
        draft: PushRecordDraft,
        target_identity: String,
    ) -> Result<MessageSendResult, PushDeliveryFailure> {
        let runtime = self.clone();
        let task = {
            let settlements = self
                .settlement_drives
                .lock()
                .map_err(|_| PushDeliveryFailure::DeliveryUnavailable)?;
            if settlements.is_closed() {
                return Err(PushDeliveryFailure::DeliveryUnavailable);
            }
            settlements
                .spawn(async move { runtime.send_remote_owned(draft, target_identity).await })
        };
        task.await.map_err(|_| PushDeliveryFailure::StoreFailed)?
    }

    async fn send_remote_owned(
        &self,
        draft: PushRecordDraft,
        target_identity: String,
    ) -> Result<MessageSendResult, PushDeliveryFailure> {
        // Configuration owns the relay even while the prepared session is offline.
        // Never admit mail with an invented relay on an unenrolled installation.
        let relay = self
            .session
            .as_ref()
            .ok_or(PushDeliveryFailure::DeliveryUnavailable)?
            .relay;
        let service = String::from(draft.target.endpoint.service_id.clone())
            .try_into()
            .map_err(|_| PushDeliveryFailure::StoreFailed)?;
        let peer = self
            .resolve_installation_peer(&service)
            .await
            .map_err(|error| match error {
                InstallationPeerResolutionError::RecipientUnknown => {
                    PushDeliveryFailure::MailRefused(
                        collaboration_mail_protocol::MailRefusal::RecipientUnknown {},
                    )
                }
                InstallationPeerResolutionError::Unavailable => {
                    PushDeliveryFailure::DeliveryUnavailable
                }
                InstallationPeerResolutionError::Corrupt { .. } => PushDeliveryFailure::StoreFailed,
            })?;
        let item = outgoing_mail_message(&draft, &self.service, peer)
            .map_err(|_| PushDeliveryFailure::StoreFailed)?;
        let item_id: MailItemId = draft
            .push_id
            .clone()
            .try_into()
            .map_err(|_| PushDeliveryFailure::StoreFailed)?;
        let read_context = collaboration_protocol::MailReadContext::Direct {
            origin: draft.origin,
            recipient: draft.target.clone(),
        };
        let target = draft.target;
        let limits = self.config.limits;
        let now = self.clock.now();
        let retirement = fleet_sync_delivery::SubmitInput {
            item: item.id(),
            digest: item.digest().clone(),
            relay,
            mailbox: peer,
        };
        let answer = self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    let (admission, _) = Outbox::<_, RouterDeliveryTypes>::new(scope, limits)
                        .enqueue_and_admit::<SubmitMailTask>(item, relay)
                        .await
                        .map_err(delivery_rollback)?;
                    super::terminal_retirement::admit_terminal_retirement(
                        scope, &admission, retirement,
                    )
                    .await?;
                    match admission {
                        OutboxAdmission::Queued { .. } => {
                            interaction_storage::record_mail_read_context_in_scope(
                                scope,
                                &item_id,
                                &read_context,
                                now,
                            )
                            .await
                            .map_err(StoreError::backend)?;
                            let answer = MessageSendResult::Queued {
                                queued: QueuedMessageSend {
                                    item: item_id.clone(),
                                    target,
                                    target_identity,
                                },
                            };
                            let MessageSendResult::Queued { queued } = &answer else {
                                return Err(StoreError::backend(
                                    "remote admission produced a local answer",
                                ));
                            };
                            record_message_send_admission(
                                scope,
                                &item_id,
                                &queued.target_identity,
                                now,
                            )
                            .await
                            .map_err(StoreError::backend)?;
                            record_message_send_outcome(scope, &item_id, &answer)
                                .await
                                .map_err(StoreError::backend)?;
                            Ok(answer)
                        }
                        OutboxAdmission::Replayed { .. } => {
                            interaction_storage::record_mail_read_context_in_scope(
                                scope,
                                &item_id,
                                &read_context,
                                now,
                            )
                            .await
                            .map_err(StoreError::backend)?;
                            let answer = read_message_send_answer_in_scope(scope, &item_id)
                                .await
                                .map_err(StoreError::backend)?
                                .and_then(|row| row.result);
                            match answer {
                                Some(answer @ MessageSendResult::Queued { .. }) => Ok(answer),
                                _ => {
                                    Err(StoreError::backend("remote custody has no queued answer"))
                                }
                            }
                        }
                        OutboxAdmission::Refused { reason } => {
                            Err(delivery_rollback(DeliveryError::Refused { reason }))
                        }
                        OutboxAdmission::CancelAlreadyRequested { .. } => {
                            Err(delivery_rollback(DeliveryError::Corrupt {
                                field: "direct original returned cancel admission",
                            }))
                        }
                    }
                })
            })
            .await
            .map_err(super::local_admission::local_admission_failure)?;
        // A queued answer promises committed custody, not a live central call.
        // A failed wake leaves the durable operation for the runtime's recovery.
        if let Err(error) = self.host.wake_at(self.host.now()).await {
            tracing::warn!(%error, "remote mail committed; runtime wake failed");
        }
        Ok(answer)
    }
}
