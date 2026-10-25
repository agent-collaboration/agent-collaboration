//! Resolve frozen control consequences through the SDK, without retaining an outcome copy.
use super::*;
use board_sync_protocol::BoardDeliveryTypes;
use collaboration_mail_protocol::CancelCause;
use fleet_sync_delivery::{DeliveryTransactionFailure, Outbox, OutboxStatusState};
use fleet_sync_protocol::{
    CancelResult, DeliveryAnswer, DeliveryEnvelope, DeliveryRefusal, DeliveryRequest,
    ItemStatusState, OperationAnswer, OperationRequest,
};

/// Both the canonical control and its target must have settled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FrozenCancelSettlement {
    Pending,
    Prevented,
    SettledWithoutPrevention,
}

/// Observing an unacknowledged association cannot enqueue a remote control.
pub(crate) enum RemoteCancelReplay {
    QueryOnly,
    Eligible { epoch: message_board::EpochNumber },
}

impl MailDeliveryRuntime {
    pub(crate) async fn replay_frozen_cancel_and_observe(
        &self,
        target: &DeliveryEnvelope<BoardDeliveryTypes>,
        cancel: &DeliveryEnvelope<BoardDeliveryTypes>,
        replay: RemoteCancelReplay,
    ) -> Result<FrozenCancelSettlement, StoreError> {
        let (DeliveryEnvelope::Item(original), DeliveryEnvelope::Cancel(control)) =
            (target, cancel)
        else {
            return Err(StoreError::backend("frozen cancellation shape"));
        };
        if control.target() != original.id()
            || control.recipient() != original.recipient()
            || control.sender() != original.sender()
        {
            return Err(StoreError::backend("frozen cancellation association"));
        }
        let config = self.config;
        let item = original.id();
        if original.recipient().address.endpoint.service_id == self.service {
            if original.recipient().peer != config.owner {
                return Err(StoreError::backend("local frozen cancellation peer"));
            }
            // The SDK's target-keyed admission re-derives the canonical control
            // after restart. Its ID and digest are authoritative locally (947).
            let replay =
                serde_json::from_value(serde_json::to_value(control).map_err(StoreError::backend)?)
                    .map_err(StoreError::backend)?;
            let canonical = match self
                .cancel_local(replay, CancelCause::BoardDerived {})
                .await
                .map_err(StoreError::backend)?
            {
                LocalCancelReference::Local { reference }
                | LocalCancelReference::ExistingElsewhere { reference } => reference,
            };
            return self.store.transaction(move |scope| Box::pin(async move {
                let mut inbox = Inbox::<_, RouterDeliveryTypes>::new(scope, config.owner, config.limits);
                let target = inbox.local_outcome(item).await.map_err(StoreError::backend)?;
                let control = inbox.local_outcome(canonical.item).await.map_err(StoreError::backend)?;
                if !local_terminal(&target) || !local_terminal(&control) {
                    return Ok(FrozenCancelSettlement::Pending);
                }
                Ok(if matches!(target, Some(RecipientOutcome::Prevented { cancel }) if cancel == canonical.item)
                    && matches!(control, Some(RecipientOutcome::Prevented { cancel }) if cancel == canonical.item) {
                    FrozenCancelSettlement::Prevented
                } else {
                    FrozenCancelSettlement::SettledWithoutPrevention
                })
            })).await;
        }
        let Some(reference) = self.remote_frozen_cancel_reference(control, replay).await? else {
            return Ok(FrozenCancelSettlement::Pending);
        };
        let canonical = self
            .query_frozen_reference(original.recipient().peer, &reference)
            .await?;
        let ItemStatusState::Known { status, cancel, .. } = canonical else {
            return Ok(FrozenCancelSettlement::Pending);
        };
        if !status.is_terminal() || matches!(cancel, Some(CancelResult::Pending)) {
            return Ok(FrozenCancelSettlement::Pending);
        }
        let target = self
            .query_frozen_reference(
                original.recipient().peer,
                &DeliveryItemRef {
                    item,
                    digest: original.digest().clone(),
                },
            )
            .await?;
        if !matches!(target, ItemStatusState::Known { status, .. } if status.is_terminal()) {
            return Ok(FrozenCancelSettlement::Pending);
        }
        Ok(if matches!(cancel, Some(CancelResult::Prevented)) {
            FrozenCancelSettlement::Prevented
        } else {
            FrozenCancelSettlement::SettledWithoutPrevention
        })
    }

    pub(super) async fn remote_frozen_cancel_reference(
        &self,
        control: &fleet_sync_protocol::DeliveryCancel<BoardDeliveryTypes>,
        replay: RemoteCancelReplay,
    ) -> Result<Option<DeliveryItemRef>, StoreError> {
        let config = self.config;
        let child = control.id();
        let state = self
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    Outbox::<_, RouterDeliveryTypes>::new(scope, config.limits)
                        .status(child)
                        .await
                        .map(|status| status.state)
                        .map_err(StoreError::backend)
                })
            })
            .await?;
        // The publisher replays eligible admission before frontier observation.
        // A relay refusal keeps the proposed row and epoch, but names another
        // canonical control; never take that refusal as the group's terminality.
        let observed = match state {
            OutboxStatusState::Known {
                status:
                    fleet_sync_protocol::DeliveryStatus::Refused {
                        reason: DeliveryRefusal::CancelAlreadyRequested { existing },
                    },
                ..
            } => existing,
            _ => DeliveryItemRef {
                item: child,
                digest: control.digest().clone(),
            },
        };
        let reference = match replay {
            RemoteCancelReplay::QueryOnly => observed,
            RemoteCancelReplay::Eligible { epoch } => {
                let cancel = serde_json::from_value(
                    serde_json::to_value(control).map_err(StoreError::backend)?,
                )
                .map_err(StoreError::backend)?;
                match self.enqueue_project_cancel(cancel, epoch).await {
                    Ok(admission) => other_canonical_cancel(&admission)
                        .cloned()
                        .unwrap_or(observed),
                    Err(DeliveryTransactionFailure::Refused(
                        DeliveryRefusal::CancelAlreadyRequested { existing },
                    )) => existing,
                    Err(DeliveryTransactionFailure::Refused(DeliveryRefusal::MailboxFull)) => {
                        return Ok(None);
                    }
                    Err(error) => return Err(StoreError::backend(error)),
                }
            }
        };
        Ok(Some(reference))
    }

    pub(super) async fn query_frozen_reference(
        &self,
        mailbox: PeerId,
        reference: &DeliveryItemRef,
    ) -> Result<ItemStatusState<CarrierAcceptance>, StoreError> {
        let Some(session) = &self.session else {
            return Ok(ItemStatusState::Unknown);
        };
        // No native transaction or project snapshot spans the public Query.
        let answer = session
            .requests
            .request(OperationRequest::Delivery(DeliveryRequest::Query {
                call: super::remote_status::status_call("frozen-item-query")?,
                mailbox,
                items: vec![reference.item],
            }))
            .await
            .map_err(StoreError::backend)?;
        let OperationAnswer::Delivery(DeliveryAnswer::Query(mut answer)) = answer else {
            return Err(StoreError::backend("frozen item query answer"));
        };
        if answer.statuses.len() != 1 {
            return Err(StoreError::backend("frozen item query cardinality"));
        }
        let status = answer
            .statuses
            .pop()
            .ok_or_else(|| StoreError::backend("frozen item query cardinality"))?;
        if status.item != reference.item {
            return Err(StoreError::backend("frozen item query identity"));
        }
        if matches!(&status.state, ItemStatusState::Known { digest, .. } if digest != &reference.digest)
        {
            return Err(StoreError::backend("frozen item query digest"));
        }
        Ok(status.state)
    }
}

fn local_terminal(outcome: &Option<RecipientOutcome<CarrierAcceptance>>) -> bool {
    matches!(outcome, Some(outcome) if !matches!(outcome, RecipientOutcome::Held))
}
