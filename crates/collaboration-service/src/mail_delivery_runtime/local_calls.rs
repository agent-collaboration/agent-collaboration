//! Resolve committed native calls through the owning custody scope.
use super::{
    AtomicStore, CallEnvelope, CallFailure, CallRouter, CallTarget, DeliverCall, DeliveryItemRef,
    Inbox, LoadPolicy, MailCallRouter, RouterDeliveryTypes, StoreError, SyncTime, Value, Weak,
    mark_mail_push_attempted_in_scope, read_mail_push_in_scope,
};

impl CallRouter for MailCallRouter {
    async fn dispatch(&self, envelope: CallEnvelope) -> Result<Value, CallFailure> {
        if envelope.kind != DeliverCall::<RouterDeliveryTypes>::KIND {
            return self.dispatch_remote(envelope).await;
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
                    if !collaboration_runtime::received_mail_push::is_poll_mail(item)
                        && !matches!(item.body(), board_sync_protocol::BoardDeliveryBody::Wake(_))
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
            Err(error) => return Err(self.binding_call_failure(error).await),
        };
        let item = resolved.item();
        let delivery = if let board_sync_protocol::BoardDeliveryBody::Wake(body) = item.body() {
            self.consumer.deliver_wake(item, body).await
        } else if collaboration_runtime::received_mail_push::is_poll_mail(item) {
            // The reader actor owns its input; retain the resolved custody value
            // here so its stored receipt bound still validates the returned answer.
            let poll_item = fleet_sync_protocol::DeliveryItem::new(
                item.id(),
                item.sender().clone(),
                item.recipient().clone(),
                item.valid_until(),
                item.body().clone(),
            )
            .map_err(|_| CallFailure::AnswerLost)?;
            let observed = self
                .consumer
                .reader_actors
                .attempt_poll_mail_once(Box::new(poll_item))
                .await;
            crate::mail_push_outcome::local_follow_outcome(
                observed.into_local_observation(),
                0,
                SyncTime::from_unix_millis(self.consumer.clock.now().timestamp_millis()),
            )
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
    pub(super) async fn binding_call_failure(&self, error: StoreError) -> CallFailure {
        match fleet_sync_delivery::classify_delivery_store_error(error) {
            fleet_sync_delivery::DeliveryTransactionFailure::BindingUnavailable {
                reason, ..
            } => {
                if reason == fleet_sync_protocol::SenderBindingFailure::Unknown {
                    let _ = super::installation_directory::refresh_sender_bindings(
                        self.binding.as_ref(),
                        self.owner.as_ref(),
                        self.session.as_ref(),
                    )
                    .await;
                }
                CallFailure::Retryable {
                    reason: durable_steps::CallRetryReason {
                        code: reason.code().into(),
                        detail: "sender binding is not established".into(),
                    },
                }
            }
            _ => CallFailure::AnswerLost,
        }
    }
}
