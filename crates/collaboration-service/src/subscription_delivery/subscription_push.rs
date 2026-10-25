//! Store-first subscription records and their prepared Layer-0 delivery.
use super::SubscriptionClock;
use crate::{
    DeliveryPrecondition, LoadPolicy, MachineIdentity, SessionMessageDelivery,
    layer_zero::{DeliveryRequest, PreparedPush},
};
use agent_automation::AttemptId;
use collaboration_protocol::{
    DeliveryCorrelationId, DeliveryOutcome, DeliveryReceipt, MachineId, MessageDelivery,
    MessageText, PushActivityRange, PushActivitySnapshot, PushHeaderFacts, PushId, PushKind,
    PushLineInput, PushOrigin, PushRecordDraft, RouterLink, RouterOriginRef, SessionRef,
    render_push_line,
};
use interaction_storage::InteractionStore;
use message_board::{
    BoardError, Identity, SubscriptionBatch, SubscriptionGeneration, SubscriptionScope,
};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub(super) struct SubscriptionPushStore {
    pub(super) store: Arc<InteractionStore>,
    pub(super) delivery: Arc<dyn SessionMessageDelivery>,
    pub(super) machine: MachineIdentity,
    pub(super) clock: Arc<dyn SubscriptionClock>,
    pub(super) shutdown: CancellationToken,
    #[cfg(test)]
    observations: tokio::sync::broadcast::Sender<super::subscription_service::OwnerObservation>,
}

pub(super) struct SubscriptionPushStoreProps {
    pub store: Arc<InteractionStore>,
    pub delivery: Arc<dyn SessionMessageDelivery>,
    pub machine: MachineIdentity,
    pub clock: Arc<dyn SubscriptionClock>,
    pub shutdown: CancellationToken,
    #[cfg(test)]
    pub observations: tokio::sync::broadcast::Sender<super::subscription_service::OwnerObservation>,
}

impl SubscriptionPushStore {
    pub(super) fn new(props: SubscriptionPushStoreProps) -> Self {
        let SubscriptionPushStoreProps {
            store,
            delivery,
            machine,
            clock,
            shutdown,
            #[cfg(test)]
            observations,
        } = props;
        Self {
            store,
            delivery,
            machine,
            clock,
            shutdown,
            #[cfg(test)]
            observations,
        }
    }

    pub(super) async fn activity(
        &self,
        target: &SessionRef,
        batch: &SubscriptionBatch,
        load_policy: LoadPolicy,
    ) -> Result<PreparedPush, BoardError> {
        let root_count =
            u8::try_from(batch.roots.len()).map_err(|_| BoardError::board_unavailable())?;
        let message_count = batch
            .roots
            .iter()
            .try_fold(0_u64, |count, root| count.checked_add(root.post_count))
            .ok_or_else(BoardError::board_unavailable)?;
        let header_facts = PushHeaderFacts::SubscriptionActivity {
            root_count,
            message_count,
            held_since: batch.held_since.map(|held| held.to_rfc3339()),
            thread_resolved: batch.draining,
        };
        let activity = PushActivitySnapshot {
            ranges: batch
                .roots
                .iter()
                .map(|root| PushActivityRange {
                    thread_id: root.thread_id.clone(),
                    project_id: root.project_id.clone(),
                    from: root.from,
                    through: root.through,
                })
                .collect(),
            held: batch.held,
            draining: batch.draining,
        };
        let origin_router_ref = RouterOriginRef::SubscriptionActivity {
            target: target.clone(),
            batch_id: batch.batch_id.clone(),
        };
        let draft = self.draft(
            target,
            PushKind::SubscriptionActivity,
            origin_router_ref,
            header_facts,
            None,
            Some(activity),
        )?;
        self.store_prepared(draft, load_policy).await
    }

    pub(super) async fn expiry(
        &self,
        target: &SessionRef,
        scope: &SubscriptionScope,
        subscription_generation: SubscriptionGeneration,
    ) -> Result<PreparedPush, BoardError> {
        let (kind, id) = scope.kind_and_id();
        let argument = match scope {
            SubscriptionScope::Thread { .. } => "--thread-id",
            SubscriptionScope::Project { .. } => "--project-id",
        };
        let draft = self.draft(
            target,
            PushKind::SubscriptionExpiry,
            RouterOriginRef::SubscriptionExpiry {
                target: target.clone(),
                scope: scope.clone(),
                subscription_generation,
            },
            PushHeaderFacts::SubscriptionExpiry {
                scope: format!("{kind} {id}"),
            },
            Some(format!(
                "Renew with: agent-collaboration thread subscribe {argument} {id}"
            )),
            None,
        )?;
        self.store_prepared(draft, LoadPolicy::LoadedOnly).await
    }

    fn draft(
        &self,
        target: &SessionRef,
        kind: PushKind,
        origin_router_ref: RouterOriginRef,
        header_facts: PushHeaderFacts,
        body: Option<String>,
        activity: Option<PushActivitySnapshot>,
    ) -> Result<PushRecordDraft, BoardError> {
        Ok(PushRecordDraft {
            push_id: PushId::try_from(uuid::Uuid::now_v7().to_string())
                .map_err(|_| BoardError::board_unavailable())?,
            kind,
            origin: PushOrigin::Router(kind),
            origin_router_ref: Some(
                origin_router_ref
                    .canonical_string()
                    .map_err(|_| BoardError::board_unavailable())?,
            ),
            target: target.clone(),
            mode: None,
            guard: None,
            reply_to_push_id: None,
            header_facts,
            body,
            activity,
            created_at: self.clock.now(),
        })
    }

    async fn store_prepared(
        &self,
        draft: PushRecordDraft,
        load_policy: LoadPolicy,
    ) -> Result<PreparedPush, BoardError> {
        let record = draft
            .clone()
            .into_pending()
            .map_err(|_| BoardError::board_unavailable())?;
        let line = render_push_line(&PushLineInput {
            link: RouterLink::new(
                MachineId::from(self.machine.service_id().clone()),
                record.push_id.clone(),
            ),
            machine_label: self.machine.machine_label().clone(),
            origin: record.origin,
            header_facts: record.header_facts,
            // Expiry stores the renew command for show; subscription lines never preview it.
            body: None,
        })
        .map_err(|_| BoardError::board_unavailable())?;
        let line = MessageText::try_from(line).map_err(|_| BoardError::board_unavailable())?;
        self.store
            .insert_push_record(draft)
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        Ok(PreparedPush {
            push_id: record.push_id,
            line,
            load_policy,
        })
    }

    pub(super) async fn mark_attempted(&self, prepared: &PreparedPush) -> Result<(), BoardError> {
        self.store
            .mark_push_attempted(&prepared.push_id)
            .await
            .map_err(|_| BoardError::board_unavailable())?;
        Ok(())
    }

    pub(super) async fn deliver(
        &self,
        target: &SessionRef,
        prepared: PreparedPush,
    ) -> Result<(), BoardError> {
        self.mark_attempted(&prepared).await?;
        let correlation = DeliveryCorrelationId::try_from(prepared.push_id.as_str().to_owned())
            .map_err(|_| BoardError::board_unavailable())?;
        let request = DeliveryRequest {
            payload: prepared.clone(),
            target: target.clone(),
            mode: MessageDelivery::Auto,
            precondition: DeliveryPrecondition::Unpinned,
            correlation,
            attempt: AttemptId::generate(),
        };
        let attempted = self
            .dispatch_mail_attempt(PushKind::SubscriptionActivity, request)
            .await?;
        let receipt = attempted.into_receipt();
        self.record_receipt(&prepared, &receipt, true).await?;
        Ok(())
    }

    /// After an effect, retry only its evidence write. The owning actor stays serialized.
    pub(super) async fn record_receipt(
        &self,
        prepared: &PreparedPush,
        receipt: &DeliveryReceipt,
        allow_hold: bool,
    ) -> Result<(), BoardError> {
        let mut delay = std::time::Duration::from_secs(1);
        loop {
            let result = {
                let store = &self.store;
                if allow_hold
                    && matches!(
                        receipt.outcome,
                        DeliveryOutcome::NotSubmitted {
                            retryable: true,
                            ..
                        }
                    )
                {
                    store
                        .hold_push_record(&prepared.push_id, receipt.clone())
                        .await
                } else {
                    store
                        .settle_push_record(&prepared.push_id, receipt.clone(), self.clock.now())
                        .await
                }
            };
            if result.is_ok() {
                return Ok(());
            }
            #[cfg(test)]
            let _ = self
                .observations
                .send(super::subscription_service::OwnerObservation::PushSettlementRetry);
            tracing::warn!(
                push_id = prepared.push_id.as_str(),
                "retrying stored push settlement without redelivery"
            );
            let deadline = self
                .clock
                .monotonic_now()
                .checked_add(delay)
                .ok_or_else(BoardError::board_unavailable)?;
            tokio::select! { () = self.shutdown.cancelled() => return Err(BoardError::board_unavailable()), () = self.clock.sleep_until(deadline) => {} }
            delay = delay
                .saturating_mul(2)
                .min(std::time::Duration::from_secs(30));
        }
    }
}

pub(super) fn target_session(reader: &Identity) -> Result<SessionRef, BoardError> {
    let Identity::Session { session } = reader else {
        return Err(BoardError::invalid_field(
            "reader",
            "deliver-mode subscription requires a session Reader",
        ));
    };
    serde_json::from_value(
        serde_json::to_value(session).map_err(|_| BoardError::board_unavailable())?,
    )
    .map_err(|_| BoardError::invalid_field("reader", "must contain a valid SessionRef"))
}
