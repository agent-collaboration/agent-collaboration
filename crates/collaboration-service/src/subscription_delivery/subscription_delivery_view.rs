//! Join frozen membership allocations to native observations for public views only.
use super::*;
use board_sync_protocol::BoardDeliveryBody;
use collaboration_mail_protocol::NotificationSource;
use collaboration_protocol::{DeliveryOutcome, PushDeliveryState};
use fleet_sync_protocol::DeliveryEnvelope;
use message_board_storage::MailHandoffItem;

struct FrozenMemberAllocation {
    sequence: HandoffSeq,
    item: fleet_sync_protocol::ItemId,
    recipient: collaboration_protocol::SessionRef,
    mode: SubscriptionMode,
}

#[derive(Clone, Debug, Default)]
pub struct SubscriptionDeliveryView {
    pub allocated_pending: u64,
    pub held_since: Option<DateTime<Utc>>,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub last_outcome: Option<SubscriptionDeliveryOutcome>,
}

impl SubscriptionWindowStore {
    pub(crate) async fn observe_subscription_deliveries(
        &self,
        snapshot: &mut ReaderSubscriptionSnapshot,
    ) -> Result<(), BoardError> {
        for row in &mut snapshot.records {
            row.delivery = self.delivery_view(&row.membership).await?;
        }
        for mut candidate in std::mem::take(&mut snapshot.resolved_view_candidates) {
            candidate.delivery = self.delivery_view(&candidate.membership).await?;
            if candidate.delivery.allocated_pending == 0 {
                continue;
            }
            let record = &candidate.record;
            candidate.record = ThreadSubscriptionRecord::new(ThreadSubscriptionRecordProps {
                reader: record.reader().clone(),
                scope: record.scope().clone(),
                policy: record.policy().clone(),
                state: SubscriptionState::Draining,
                renewed_at: record.renewed_at(),
                expires_at: record.expires_at(),
                ended_at: None,
                generation: record.generation(),
                last_outcome: record.last_outcome().cloned(),
                roots: record.roots().to_vec(),
            })
            .map_err(unavailable)?;
            snapshot.records.push(candidate);
        }
        Ok(())
    }
    pub(super) async fn delivery_view(
        &self,
        membership: &SubscriptionMembership,
    ) -> Result<SubscriptionDeliveryView, BoardError> {
        let project = membership
            .envelope
            .scope
            .project
            .as_ref()
            .ok_or_else(BoardError::board_unavailable)?;
        let store = self
            .registry
            .existing_project(project)
            .await
            .map_err(unavailable)?
            .ok_or_else(BoardError::board_unavailable)?;
        let mut read = store.snapshot().await.map_err(unavailable)?;
        let mut after = HandoffSeq::ZERO;
        let mut allocations = Vec::new();
        loop {
            let page = read
                .mail_handoff_page(after, 100)
                .await
                .map_err(unavailable)?;
            if page.is_empty() {
                break;
            }
            for handoff in page {
                after = handoff.sequence;
                let MailHandoffItem::Delivery(envelope) = handoff.item else {
                    continue;
                };
                let DeliveryEnvelope::Item(item) = *envelope else {
                    continue;
                };
                let BoardDeliveryBody::Notification(body) = item.body() else {
                    continue;
                };
                if let NotificationSource::FollowBatch {
                    membership: id,
                    activated_at,
                    policy,
                    ..
                } = &body.source
                    && id.as_str() == membership.envelope.id.as_str()
                    && *activated_at == membership.activated_at()
                {
                    allocations.push(FrozenMemberAllocation {
                        sequence: handoff.sequence,
                        item: item.id(),
                        recipient: item
                            .recipient()
                            .address
                            .clone()
                            .try_into()
                            .map_err(unavailable)?,
                        mode: policy.mode(),
                    });
                }
            }
        }
        read.finish().await.map_err(unavailable)?;
        let mut view = SubscriptionDeliveryView::default();
        let mut latest = None;
        for allocation in allocations {
            let sequence = allocation.sequence;
            let push = allocation
                .item
                .as_uuid()
                .to_string()
                .try_into()
                .map_err(unavailable)?;
            let poll_pending = if allocation.mode == SubscriptionMode::Poll {
                self.interactions
                    .pending_poll_item(allocation.item)
                    .await
                    .map_err(unavailable)?
            } else {
                None
            };
            if let Some(pending) = &poll_pending {
                if pending.membership != membership.envelope.id
                    || pending.recipient != allocation.recipient
                {
                    return Err(BoardError::board_unavailable());
                }
                view.allocated_pending = view
                    .allocated_pending
                    .checked_add(1)
                    .ok_or_else(BoardError::board_unavailable)?;
            }
            let Some(observed) = self
                .interactions
                .mail_delivery_observation(&push)
                .await
                .map_err(unavailable)?
            else {
                continue;
            };
            if poll_pending.is_none()
                && matches!(
                    observed.state,
                    PushDeliveryState::Pending
                        | PushDeliveryState::Attempted
                        | PushDeliveryState::Held
                )
            {
                view.allocated_pending = view
                    .allocated_pending
                    .checked_add(1)
                    .ok_or_else(BoardError::board_unavailable)?;
            }
            if matches!(
                observed.state,
                PushDeliveryState::Held | PushDeliveryState::Pending
            ) {
                view.held_since = view.held_since.into_iter().chain(observed.held_since).min();
            }
            if observed.state == PushDeliveryState::Held {
                view.next_retry_at = view
                    .next_retry_at
                    .into_iter()
                    .chain(observed.scheduled_retry_at)
                    .min();
            }
            if let (Some(at), Some(receipt)) = (observed.recorded_at, observed.last_outcome)
                && latest.is_none_or(|previous| (at, sequence) > previous)
            {
                latest = Some((at, sequence));
                view.last_outcome = Some(match receipt.outcome {
                    DeliveryOutcome::NotSubmitted { reason, retryable } => {
                        SubscriptionDeliveryOutcome::NotSubmitted { reason, retryable }
                    }
                    DeliveryOutcome::Unknown => {
                        let mut evidence = serde_json::to_value(receipt).map_err(unavailable)?;
                        if let Some(route) = observed.route_evidence {
                            evidence
                                .as_object_mut()
                                .ok_or_else(BoardError::board_unavailable)?
                                .insert(
                                    "routeEvidence".to_owned(),
                                    serde_json::to_value(
                                        agent_automation::RouteEffectEvidence::ProviderAcp(route),
                                    )
                                    .map_err(unavailable)?,
                                );
                        }
                        SubscriptionDeliveryOutcome::Unknown { evidence }
                    }
                    DeliveryOutcome::Rejected(_) => SubscriptionDeliveryOutcome::Rejected {
                        evidence: serde_json::to_value(receipt).map_err(unavailable)?,
                    },
                    _ => SubscriptionDeliveryOutcome::Accepted,
                });
            }
        }
        Ok(view)
    }
}
