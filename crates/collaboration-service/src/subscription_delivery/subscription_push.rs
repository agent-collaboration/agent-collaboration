//! Store-first subscription records and their prepared Layer-0 delivery.
use collaboration_contracts::MachineIdentity;
use collaboration_contracts::SessionMessageDelivery;
use collaboration_contracts::layer_zero::PreparedPush;
use collaboration_protocol::{DeliveryOutcome, DeliveryReceipt, SessionRef};
use interaction_storage::InteractionStore;
use message_board::{BoardError, Identity};
use router_clock::RuntimeClock;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub(super) struct SubscriptionPushStore {
    pub(super) store: Arc<InteractionStore>,
    pub(super) delivery: Arc<dyn SessionMessageDelivery>,
    pub(super) machine: MachineIdentity,
    pub(super) clock: Arc<dyn RuntimeClock>,
    pub(super) shutdown: CancellationToken,
    #[cfg(test)]
    observations: tokio::sync::broadcast::Sender<super::subscription_service::OwnerObservation>,
}

pub(super) struct SubscriptionPushStoreProps {
    pub store: Arc<InteractionStore>,
    pub delivery: Arc<dyn SessionMessageDelivery>,
    pub machine: MachineIdentity,
    pub clock: Arc<dyn RuntimeClock>,
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

    pub(super) async fn mark_attempted(&self, prepared: &PreparedPush) -> Result<(), BoardError> {
        self.store
            .mark_push_attempted(&prepared.push_id)
            .await
            .map_err(|_| BoardError::board_unavailable())?;
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
                        .settle_push_record(
                            &prepared.push_id,
                            receipt.clone(),
                            self.clock.utc_now(),
                        )
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
