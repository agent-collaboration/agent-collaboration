//! Acknowledged project cleanup releases its frontier while custody keeps its 60-day record.
use super::{
    AtomicStore, Clock, DeliveryItemRef, Inbox, ItemId, MailDeliveryRuntime, PeerId,
    RecipientOutcome, RemoteCancelReplay, RouterDeliveryTypes, StoreError,
};
use board_sync_protocol::BoardDeliveryTypes;
use fleet_sync_protocol::{DeliveryEnvelope, ItemStatusState};

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
        if item
            .recipient()
            .address
            .session()
            .map_err(StoreError::backend)?
            .endpoint
            .service_id
            == self.service
        {
            if item.recipient().peer != self.config.owner {
                return Err(StoreError::backend("local frozen handoff peer"));
            }
            let id = item.id();
            let config = self.config;
            let binding = self.binding.clone();
            return self.store.transaction(move |scope| Box::pin(async move {
                let outcome = Inbox::<_, RouterDeliveryTypes, _>::new(scope, config.owner, config.limits, binding.as_ref())
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
        if self.host.now() >= fleet_sync_delivery::removal_deadline(item.id()) {
            return Ok(true);
        }
        let reference = DeliveryItemRef {
            item: item.id(),
            digest: item.digest().clone(),
        };
        if item
            .recipient()
            .address
            .session()
            .map_err(StoreError::backend)?
            .endpoint
            .service_id
            == self.service
        {
            if item.recipient().peer != self.config.owner {
                return Err(StoreError::backend("local retirement peer"));
            }
            return self.frozen_delivery_is_terminal(envelope).await;
        }
        self.remote_reference_is_terminal(item.recipient().peer, &reference)
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
        if control
            .recipient()
            .address
            .session()
            .map_err(StoreError::backend)?
            .endpoint
            .service_id
            == self.service
        {
            // The public local retirement contract intentionally retains controls.
            return Ok(control.recipient().peer == self.config.owner);
        }
        if self.host.now() >= fleet_sync_delivery::removal_deadline(control.id()) {
            return Ok(true);
        }
        let Some(canonical) = self
            .remote_frozen_cancel_reference(control, RemoteCancelReplay::Eligible { epoch })
            .await?
        else {
            return Ok(false);
        };
        if canonical.item != control.id() {
            // Decision 949: an earlier requested control is not this group's custody.
            return Ok(true);
        }
        self.remote_reference_is_terminal(control.recipient().peer, &canonical)
            .await
    }

    async fn remote_reference_is_terminal(
        &self,
        mailbox: PeerId,
        reference: &DeliveryItemRef,
    ) -> Result<bool, StoreError> {
        if !matches!(self.query_frozen_reference(mailbox, reference).await?,
            ItemStatusState::Known { status, .. } if status.is_terminal())
        {
            return Ok(false);
        }
        Ok(true)
    }
}
