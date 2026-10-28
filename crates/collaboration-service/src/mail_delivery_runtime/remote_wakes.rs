//! Remote wake requests use the same durable outbox, inbox and canonical reports as messages.
use super::{
    Arc, AtomicStore, CarrierAcceptance, Clock, DeliveryError, DeliveryItem, DeliveryReceipt,
    InstallationPeerResolutionError, ItemId, LocalDelivery, MailConsumer, MailDeliveryRuntime,
    MailSender, RouterDeliveryTypes, StoreError, SubmitMailTask, SyncTime, WakeScheduler,
};
use collaboration_mail_protocol::{
    RemoteWakeCreator, RemoteWakeMutationState, RemoteWakeRefusal, RemoteWakeResult, WakeBody,
};
use collaboration_protocol::{LocalMutationEvidence, LocalMutationState, WakeFailure};
use fleet_sync_delivery::{
    LocalOutcome, Outbox, OutboxAdmission, OutboxStatusState, delivery_rollback,
};
use fleet_sync_protocol::DeliveryStatus;
use serde::{Serialize, de::DeserializeOwned};

pub(crate) struct LocalWakeOwner {
    pub service: collaboration_protocol::UuidIdentity,
    pub store: Arc<tokio::sync::Mutex<automation_storage::AutomationStore>>,
    pub permits: Arc<tokio::sync::Semaphore>,
}
pub(crate) fn convert<T: DeserializeOwned>(value: impl Serialize) -> Result<T, ()> {
    serde_json::from_value(serde_json::to_value(value).map_err(|_| ())?).map_err(|_| ())
}
fn refused(reason: RemoteWakeRefusal, mutation: RemoteWakeMutationState) -> RemoteWakeResult {
    RemoteWakeResult::Refused { reason, mutation }
}
impl MailDeliveryRuntime {
    pub fn attach_wake_owner(
        &self,
        service: collaboration_protocol::UuidIdentity,
        store: Arc<tokio::sync::Mutex<automation_storage::AutomationStore>>,
        permits: Arc<tokio::sync::Semaphore>,
    ) -> Result<(), String> {
        let owner = LocalWakeOwner {
            service,
            store,
            permits,
        };
        if let Some(existing) = self.consumer.wakes.get() {
            return if existing.service == owner.service
                && Arc::ptr_eq(&existing.store, &owner.store)
                && Arc::ptr_eq(&existing.permits, &owner.permits)
            {
                Ok(())
            } else {
                Err("mail runtime already has a different physical wake owner".into())
            };
        }
        self.consumer
            .wakes
            .set(owner)
            .map_err(|_| "mail wake owner already attached".into())
    }

    pub(crate) async fn send_wake_mail(
        &self,
        prepared: automation_storage::RemoteWakePreparation,
    ) -> Result<RemoteWakeResult, RemoteWakeRefusal> {
        let (request, previously_prepared) = match prepared {
            automation_storage::RemoteWakePreparation::New(request) => (request, false),
            automation_storage::RemoteWakePreparation::Replayed {
                snapshot: Some(snapshot),
                ..
            } => {
                return convert(snapshot)
                    .map(RemoteWakeResult::Snapshot)
                    .map_err(|_| RemoteWakeRefusal::InvalidRecord);
            }
            automation_storage::RemoteWakePreparation::Replayed { request, .. } => (request, true),
        };
        let id = ItemId::new(
            uuid::Uuid::parse_str(request.operation_id.as_str())
                .map_err(|_| RemoteWakeRefusal::InvalidRecord)?,
        )
        .map_err(|_| RemoteWakeRefusal::InvalidRecord)?;
        let state = self
            .outbox_status(id)
            .await
            .map_err(|_| RemoteWakeRefusal::RemoteUnavailable)?;
        if self.clock.now().timestamp_millis() >= request.valid_until_ms {
            return expired_wake_answer(state, previously_prepared);
        }
        if previously_prepared && let Some(answer) = retained_wake_answer(state)? {
            return Ok(answer);
        }
        let session = self
            .session
            .as_ref()
            .ok_or(RemoteWakeRefusal::RemoteUnavailable)?;
        let service = String::from(request.target.endpoint.service_id.clone())
            .try_into()
            .map_err(|_| RemoteWakeRefusal::InvalidRecord)?;
        let peer = self
            .resolve_installation_peer(&service)
            .await
            .map_err(|error| match error {
                InstallationPeerResolutionError::RecipientUnknown => {
                    RemoteWakeRefusal::ResourceNotFound
                }
                InstallationPeerResolutionError::Unavailable => {
                    RemoteWakeRefusal::RemoteUnavailable
                }
                InstallationPeerResolutionError::Corrupt { .. } => RemoteWakeRefusal::InvalidRecord,
            })?;
        let sender = match &request.body.requested_by {
            RemoteWakeCreator::Session(sender) => MailSender::Session(sender.clone()),
            RemoteWakeCreator::Owner {}
            | RemoteWakeCreator::Router {}
            | RemoteWakeCreator::Unrecorded {} => MailSender::HumanUser {
                installation: self.service.clone(),
            },
        };
        if !sender.matches_submitting_installation(&self.service) {
            return Err(RemoteWakeRefusal::InvalidField {
                field: "requestedBy".into(),
                constraint: "Creator must belong to the submitting installation.".into(),
            });
        }
        request
            .body
            .validate_sender(&sender)
            .map_err(|_| RemoteWakeRefusal::InvalidRecord)?;
        let item = DeliveryItem::<RouterDeliveryTypes>::new(
            id,
            sender,
            fleet_sync_protocol::Recipient {
                peer,
                address: convert(request.target).map_err(|_| RemoteWakeRefusal::InvalidRecord)?,
            },
            Some(SyncTime::from_unix_millis(request.valid_until_ms)),
            board_sync_protocol::BoardDeliveryBody::Wake(request.body),
        )
        .map_err(|_| RemoteWakeRefusal::InvalidRecord)?;
        let deadline_ms = request.valid_until_ms;
        let relay = session.relay;
        let limits = self.config.limits;
        let clock = self.clock.clone();
        let retirement = fleet_sync_delivery::SubmitInput {
            item: id,
            digest: item.digest().clone(),
            relay,
            mailbox: peer,
        };
        let expired = self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    // Peer lookup or the store lock may have consumed the deadline.
                    // Inspect custody again atomically, without admitting expired work.
                    if clock.now().timestamp_millis() >= deadline_ms {
                        return Outbox::<_, RouterDeliveryTypes>::new(scope, limits)
                            .status(id)
                            .await
                            .map(|status| Some(status.state))
                            .map_err(delivery_rollback);
                    }
                    let (answer, _) = Outbox::<_, RouterDeliveryTypes>::new(scope, limits)
                        .enqueue_and_admit::<SubmitMailTask>(item, relay)
                        .await
                        .map_err(delivery_rollback)?;
                    super::terminal_retirement::admit_terminal_retirement(
                        scope, &answer, retirement,
                    )
                    .await?;
                    match answer {
                        OutboxAdmission::Queued { .. } | OutboxAdmission::Replayed { .. } => {
                            Ok(None)
                        }
                        OutboxAdmission::Refused { reason } => {
                            Err(delivery_rollback(DeliveryError::Refused { reason }))
                        }
                        OutboxAdmission::CancelAlreadyRequested { .. } => {
                            Err(StoreError::backend("wake item has a cancel admission"))
                        }
                    }
                })
            })
            .await
            .map_err(|_| RemoteWakeRefusal::RemoteUnavailable)?;
        if let Some(state) = expired {
            return expired_wake_answer(state, previously_prepared);
        }
        self.host
            .wake_at(self.host.now())
            .await
            .map_err(|_| RemoteWakeRefusal::RemoteUnavailable)?;
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(50));
        loop {
            interval.tick().await;
            let state = self
                .outbox_status(id)
                .await
                .map_err(|_| RemoteWakeRefusal::RemoteUnavailable)?;
            if let Some(answer) = retained_wake_answer(state)? {
                return Ok(answer);
            }
            if self.clock.now().timestamp_millis() >= deadline_ms {
                return Err(RemoteWakeRefusal::DeadlineExpired);
            }
        }
    }
}

fn retained_wake_answer(
    state: OutboxStatusState<CarrierAcceptance>,
) -> Result<Option<RemoteWakeResult>, RemoteWakeRefusal> {
    match state {
        OutboxStatusState::Known {
            status:
                DeliveryStatus::Delivered {
                    acceptance: CarrierAcceptance::Wake(result),
                },
            ..
        } => Ok(Some(*result)),
        OutboxStatusState::Known {
            status: DeliveryStatus::Delivered { .. },
            ..
        } => Err(RemoteWakeRefusal::InvalidRecord),
        OutboxStatusState::Known {
            status: DeliveryStatus::Expired,
            ..
        } => Err(RemoteWakeRefusal::DeadlineExpired),
        OutboxStatusState::Known {
            status:
                DeliveryStatus::Refused { .. } | DeliveryStatus::Uncertain | DeliveryStatus::Cancelled,
            ..
        } => Err(RemoteWakeRefusal::RemoteUnavailable),
        _ => Ok(None),
    }
}
fn expired_wake_answer(
    state: OutboxStatusState<CarrierAcceptance>,
    previously_prepared: bool,
) -> Result<RemoteWakeResult, RemoteWakeRefusal> {
    let had_custody = matches!(&state, OutboxStatusState::Known { .. });
    if let Some(answer) = retained_wake_answer(state)? {
        return Ok(answer);
    }
    Ok(refused(
        RemoteWakeRefusal::DeadlineExpired,
        if previously_prepared || had_custody {
            RemoteWakeMutationState::Unknown
        } else {
            RemoteWakeMutationState::None
        },
    ))
}

impl MailConsumer {
    pub(super) async fn deliver_wake(
        &self,
        item: &DeliveryItem<RouterDeliveryTypes>,
        body: &WakeBody,
    ) -> LocalDelivery<RouterDeliveryTypes> {
        let response = match self.wakes.get() {
            Some(owner) => owner.apply(item, body).await,
            None => refused(
                RemoteWakeRefusal::AutomationUnavailable,
                RemoteWakeMutationState::None,
            ),
        };
        let response = if serde_json::to_vec(&response).is_ok_and(|bytes| {
            bytes.len() <= collaboration_mail_protocol::MAX_REMOTE_WAKE_RESULT_BYTES
        }) {
            response
        } else {
            refused(
                RemoteWakeRefusal::Overloaded,
                RemoteWakeMutationState::Unknown,
            )
        };
        LocalDelivery {
            outcome: LocalOutcome::Delivered {
                acceptance: CarrierAcceptance::Wake(Box::new(response)),
            },
            // Wake admission is an operation answer, never a fabricated native push.
            observation: DeliveryReceipt {
                outcome: collaboration_protocol::DeliveryOutcome::NotSubmitted {
                    retryable: false,
                    reason: "owning wake operation answered".into(),
                },
                reachability: None,
                client: None,
            }
            .into(),
        }
    }
}
impl LocalWakeOwner {
    async fn apply(
        &self,
        item: &DeliveryItem<RouterDeliveryTypes>,
        body: &WakeBody,
    ) -> RemoteWakeResult {
        let fail = |reason| refused(reason, RemoteWakeMutationState::None);
        if body.validate_sender(item.sender()).is_err()
            || item.recipient().address.endpoint.service_id.as_str()
                != String::from(self.service.clone())
        {
            return fail(RemoteWakeRefusal::InvalidRecord);
        }
        let wakes =
            crate::collaboration_application::WakeOperations::new(&self.service, Some(&self.store))
                .with_wait_capacity(&self.permits);
        use collaboration_mail_protocol::RemoteWakeRequest as R;
        // Every control's frozen target must match the actual local wake before mutation.
        if let R::Show { wakeup_id }
        | R::Pause { wakeup_id, .. }
        | R::Resume { wakeup_id, .. }
        | R::Cancel { wakeup_id, .. }
        | R::Wait { wakeup_id, .. } = &body.request
        {
            let Ok(wakeup_id) = convert(wakeup_id) else {
                return fail(RemoteWakeRefusal::InvalidRecord);
            };
            match wakes
                .wake_show_local(collaboration_protocol::WakeShowRequest { wakeup_id })
                .await
            {
                Ok(snapshot)
                    if convert::<message_board::SessionRef>(
                        snapshot.definition.message.target.clone(),
                    )
                    .ok()
                    .as_ref()
                        == Some(&item.recipient().address) => {}
                Ok(_) => return fail(RemoteWakeRefusal::ResourceNotFound),
                Err(error) => return remote_failure(error),
            }
        }
        let answer: Result<RemoteWakeResult, WakeFailure> = async {
            match &body.request {
                R::Send {
                    operation_id,
                    replace,
                    message,
                    timing,
                    expiry,
                } => {
                    if message.target != item.recipient().address {
                        return Ok(fail(RemoteWakeRefusal::InvalidRecord));
                    }
                    let request = collaboration_protocol::WakeSendRequest {
                        operation_id: convert(operation_id).map_err(|_| invalid_wake())?,
                        requested_by: convert(&body.requested_by).map_err(|_| invalid_wake())?,
                        replace: *replace,
                        message: convert(message).map_err(|_| invalid_wake())?,
                        timing: convert(timing).map_err(|_| invalid_wake())?,
                        expiry: convert(expiry).map_err(|_| invalid_wake())?,
                    };
                    Ok(RemoteWakeResult::Snapshot(Box::new(
                        convert(wakes.wake_send_local(request).await?)
                            .map_err(|_| invalid_wake())?,
                    )))
                }
                R::Show { wakeup_id } => Ok(RemoteWakeResult::Snapshot(Box::new(
                    convert(
                        wakes
                            .wake_show_local(collaboration_protocol::WakeShowRequest {
                                wakeup_id: convert(wakeup_id).map_err(|_| invalid_wake())?,
                            })
                            .await?,
                    )
                    .map_err(|_| invalid_wake())?,
                ))),
                R::Pause {
                    operation_id,
                    wakeup_id,
                }
                | R::Resume {
                    operation_id,
                    wakeup_id,
                }
                | R::Cancel {
                    operation_id,
                    wakeup_id,
                } => {
                    let request = collaboration_protocol::WakeMutationRequest {
                        operation_id: convert(operation_id).map_err(|_| invalid_wake())?,
                        wakeup_id: convert(wakeup_id).map_err(|_| invalid_wake())?,
                    };
                    let result = match body.request {
                        R::Pause { .. } => wakes.wake_pause_local(request).await,
                        R::Resume { .. } => wakes.wake_resume_local(request).await,
                        _ => wakes.wake_cancel_local(request).await,
                    }?;
                    Ok(RemoteWakeResult::Mutation(Box::new(
                        convert(result).map_err(|_| invalid_wake())?,
                    )))
                }
                R::Wait {
                    wakeup_id,
                    after,
                    deadline,
                } => {
                    let remaining = (deadline.as_datetime() - chrono::Utc::now())
                        .to_std()
                        .map_err(|_| invalid_wake())?;
                    let wait = collaboration_protocol::WakeWaitRequest {
                        wakeup_id: convert(wakeup_id).map_err(|_| invalid_wake())?,
                        after: after.clone(),
                        timeout_seconds: Some(
                            u32::try_from(remaining.as_secs().max(1))
                                .map_err(|_| invalid_wake())?,
                        ),
                    };
                    match tokio::time::timeout(remaining, wakes.wake_wait_local(wait)).await {
                        Ok(Ok(answer)) => Ok(RemoteWakeResult::Wait(
                            convert(answer).map_err(|_| invalid_wake())?,
                        )),
                        Ok(Err(
                            crate::collaboration_application::WakeWaitFailure::InvalidField {
                                field,
                                constraint,
                            },
                        )) => Ok(fail(RemoteWakeRefusal::InvalidField {
                            field: field.into(),
                            constraint: constraint.into(),
                        })),
                        _ => Ok(fail(RemoteWakeRefusal::RemoteUnavailable)),
                    }
                }
            }
        }
        .await;
        answer.unwrap_or_else(remote_failure)
    }
}
fn invalid_wake() -> WakeFailure {
    crate::collaboration_application::wake_operations::wake_failure(
        Default::default(),
        collaboration_protocol::WakeFailureReason::InvalidRecord,
        LocalMutationState::Unknown,
    )
}
fn remote_failure(error: WakeFailure) -> RemoteWakeResult {
    let reason = convert(error.reason).unwrap_or(RemoteWakeRefusal::InvalidRecord);
    let LocalMutationEvidence::Local { mutation } = error.effects;
    refused(
        reason,
        convert(mutation).unwrap_or(RemoteWakeMutationState::Unknown),
    )
}
