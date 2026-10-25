//! Frontier observations and acknowledged retirement use public SDK custody operations.
use super::*;
use board_sync_protocol::BoardDeliveryTypes;
use fleet_sync_delivery::LocalRetirement;
use fleet_sync_protocol::{
    DeliveryAnswer, DeliveryEnvelope, DeliveryRequest, ItemStatusState, OperationAnswer,
    OperationRequest,
};

impl MailDeliveryRuntime {
    pub(crate) async fn await_native_outcome_commit(&self, item: ItemId) -> Result<(), StoreError> {
        let push = item
            .as_uuid()
            .to_string()
            .try_into()
            .map_err(StoreError::backend)?;
        // This owner's read acquires the same mutex as the callback transaction.
        // It also works for Poll, where the read legitimately finds no push row.
        self.owner
            .mail_delivery_observation(&push)
            .await
            .map_err(StoreError::backend)?;
        Ok(())
    }
    pub(crate) async fn frozen_delivery_is_terminal(
        &self,
        envelope: &DeliveryEnvelope<BoardDeliveryTypes>,
    ) -> Result<bool, StoreError> {
        let DeliveryEnvelope::Item(item) = envelope else {
            return Err(StoreError::backend("frozen handoff is not an original"));
        };
        if item.recipient().address.endpoint.service_id == self.service {
            if item.recipient().peer != self.config.owner {
                return Err(StoreError::backend("local frozen handoff peer"));
            }
            let id = item.id();
            let config = self.config;
            return self.store.transaction(move |scope| Box::pin(async move {
                let outcome = Inbox::<_, RouterDeliveryTypes>::new(scope, config.owner, config.limits)
                    .local_outcome(id).await.map_err(StoreError::backend)?;
                Ok(matches!(outcome, Some(outcome) if !matches!(outcome, RecipientOutcome::Held)))
            })).await;
        }
        Ok(
            matches!(self.query_frozen_reference(item.recipient().peer, &DeliveryItemRef {
            item: item.id(), digest: item.digest().clone(),
        }).await?, ItemStatusState::Known { status, .. } if status.is_terminal()),
        )
    }

    /// The publisher calls this only below the project's acknowledged settled frontier.
    pub(crate) async fn retire_frozen_original(
        &self,
        envelope: &DeliveryEnvelope<BoardDeliveryTypes>,
    ) -> Result<bool, StoreError> {
        let DeliveryEnvelope::Item(item) = envelope else {
            return Err(StoreError::backend("retired handoff is not an original"));
        };
        let reference = DeliveryItemRef {
            item: item.id(),
            digest: item.digest().clone(),
        };
        if item.recipient().address.endpoint.service_id == self.service {
            if item.recipient().peer != self.config.owner {
                return Err(StoreError::backend("local retirement peer"));
            }
            let config = self.config;
            return self
                .store
                .transaction(move |scope| {
                    Box::pin(async move {
                        let retired = Inbox::<_, RouterDeliveryTypes>::new(
                            scope,
                            config.owner,
                            config.limits,
                        )
                        .retire_local(&reference)
                        .await
                        .map_err(StoreError::backend)?;
                        // The acknowledged frontier supplies eligibility, including on a
                        // replay after deletion committed but native progress did not.
                        Ok(matches!(
                            retired,
                            LocalRetirement::Retired | LocalRetirement::Absent
                        ))
                    })
                })
                .await;
        }
        self.retire_remote_reference(item.recipient().peer, &reference)
            .await
    }

    pub(crate) async fn retire_frozen_control(
        &self,
        envelope: &DeliveryEnvelope<BoardDeliveryTypes>,
        epoch: message_board::EpochNumber,
    ) -> Result<bool, StoreError> {
        let DeliveryEnvelope::Cancel(control) = envelope else {
            return Err(StoreError::backend("retired expansion is not a control"));
        };
        if control.recipient().address.endpoint.service_id == self.service {
            // The public local retirement contract intentionally retains controls.
            return Ok(control.recipient().peer == self.config.owner);
        }
        let Some(canonical) = self
            .remote_frozen_cancel_reference(control, RemoteCancelReplay::Eligible { epoch })
            .await?
        else {
            return Ok(false);
        };
        if canonical.item != control.id() {
            // An earlier automatic group's canonical control remains its own custody.
            return Ok(true);
        }
        self.retire_remote_reference(control.recipient().peer, &canonical)
            .await
    }

    async fn retire_remote_reference(
        &self,
        mailbox: PeerId,
        reference: &DeliveryItemRef,
    ) -> Result<bool, StoreError> {
        if !matches!(self.query_frozen_reference(mailbox, reference).await?,
            ItemStatusState::Known { status, .. } if status.is_terminal())
        {
            return Ok(false);
        }
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| StoreError::backend("retirement session missing"))?;
        let answer = session
            .requests
            .request(OperationRequest::Delivery(DeliveryRequest::Retire {
                call: super::remote_status::status_call("frozen-item-retire")?,
                mailbox,
                items: vec![reference.item],
            }))
            .await
            .map_err(StoreError::backend)?;
        let OperationAnswer::Delivery(DeliveryAnswer::Retire(answer)) = answer else {
            return Err(StoreError::backend("frozen retirement answer"));
        };
        let exact = |items: &[ItemId]| items == [reference.item];
        if exact(&answer.retired)
            && answer.unknown.is_empty()
            && answer.not_current_target.is_empty()
        {
            return Ok(true);
        }
        if answer.retired.is_empty()
            && ((exact(&answer.unknown) && answer.not_current_target.is_empty())
                || (exact(&answer.not_current_target) && answer.unknown.is_empty()))
        {
            return Ok(false);
        }
        Err(StoreError::backend("frozen retirement identities"))
    }
}
