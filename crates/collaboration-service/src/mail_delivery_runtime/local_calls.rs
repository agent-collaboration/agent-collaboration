//! Resolve committed native calls through the owning custody scope.
use super::{
    AtomicStore, CallEnvelope, CallFailure, CallRouter, CallTarget, DeliverCall, DeliveryItemRef,
    Inbox, LoadPolicy, MailCallRouter, RouterDeliveryTypes, StoreError, Value, Weak,
    mark_mail_push_attempted_in_scope, read_mail_push_in_scope,
};

impl CallRouter for MailCallRouter {
    async fn dispatch(&self, envelope: CallEnvelope) -> Result<Value, CallFailure> {
        if envelope.kind != DeliverCall::<RouterDeliveryTypes>::KIND {
            return Err(CallFailure::AnswerLost);
        }
        let _: DeliverCall<RouterDeliveryTypes> =
            serde_json::from_value(envelope.target).map_err(|_| CallFailure::AnswerLost)?;
        let reference: DeliveryItemRef =
            serde_json::from_value(envelope.request).map_err(|_| CallFailure::AnswerLost)?;
        let store = self
            .store
            .get()
            .and_then(Weak::upgrade)
            .ok_or(CallFailure::AnswerLost)?;
        let config = self.config;
        let result = store
            .transaction({ let binding = self.binding.clone(); move |scope| {
                Box::pin(async move {
                    let mut inbox =
                        Inbox::<_, RouterDeliveryTypes, _>::new(scope, config.owner, config.limits, binding.as_ref());
                    let resolved = inbox
                        .resolve_local_delivery(&reference)
                        .await
                        .map_err(fleet_sync_delivery::delivery_rollback)?;
                    let item = resolved.item();
                    // SDK custody retains Held for later attempt identities, even
                    // after readiness makes the application projection Pending.
                    let first_attempt = inbox
                        .local_outcome(item.id())
                        .await
                        .map_err(StoreError::backend)?
                        .is_none();
                    let push_id = item
                        .id()
                        .as_uuid()
                        .to_string()
                        .try_into()
                        .map_err(StoreError::backend)?;
                    let mut direct_load_policy = LoadPolicy::LoadedOnly;
                    if matches!(item.body(), board_sync_protocol::BoardDeliveryBody::Message(_))
                    {
                        let current = read_mail_push_in_scope(scope, &push_id)
                            .await
                            .map_err(StoreError::backend)?
                            .ok_or_else(|| StoreError::backend("mail push is missing"))?;
                        let draft = collaboration_runtime::received_mail_push(item, current.created_at)
                            .map_err(StoreError::backend)?
                            .ok_or_else(|| StoreError::backend("native mail has no push"))?;
                        // The envelope freezes mode/guard. Only the pre-attempt state
                        // authorizes this call to load; no value survives a replay.
                        direct_load_policy = if first_attempt {
                            collaboration_runtime::mail_push_preparation::direct_message_load_policy(
                                draft.mode,
                                draft.guard.as_ref(),
                                current.delivery_state,
                            )
                        } else {
                            LoadPolicy::LoadedOnly
                        };
                        mark_mail_push_attempted_in_scope(scope, &push_id)
                            .await
                            .map_err(StoreError::backend)?;
                    }
                    Ok((resolved, direct_load_policy))
                })
            } })
            .await;
        let (resolved, direct_load_policy) = match result {
            Ok(resolved) => resolved,
            Err(error) => return Err(Self::binding_call_failure(error)),
        };
        let item = resolved.item();
        let delivery = if let board_sync_protocol::BoardDeliveryBody::Wake(body) = item.body() {
            self.consumer.deliver_wake(item, body)
        } else if matches!(
            item.body(),
            board_sync_protocol::BoardDeliveryBody::Notification(_)
        ) {
            super::LocalDelivery {
                outcome: fleet_sync_delivery::LocalOutcome::Dropped,
                observation: super::DeliveryReceipt {
                    outcome: collaboration_protocol::DeliveryOutcome::NotSubmitted {
                        retryable: false,
                        reason: "Board notification delivery is not available in this build"
                            .to_owned(),
                    },
                    reachability: None,
                    client: None,
                }
                .into(),
            }
        } else {
            self.consumer
                .deliver_with_load_policy(item, envelope.call, direct_load_policy)
                .await
        };
        serde_json::to_value(resolved.validated_answer(delivery))
            .map_err(|_| CallFailure::AnswerLost)
    }
}

impl MailCallRouter {
    fn binding_call_failure(error: StoreError) -> CallFailure {
        match fleet_sync_delivery::classify_delivery_store_error(error) {
            fleet_sync_delivery::DeliveryTransactionFailure::BindingUnavailable {
                reason, ..
            } => CallFailure::Retryable {
                reason: durable_steps::CallRetryReason {
                    code: reason.code().into(),
                    detail: "sender binding is not established".into(),
                },
            },
            _ => CallFailure::AnswerLost,
        }
    }
}
