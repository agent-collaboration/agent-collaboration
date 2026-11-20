//! Host custody of delivery evidence requested by an exact owner placement.

use acp_client_runtime::session_owner_task::OwnerCompletion;
use agent_automation::{RouteEffectEvidence, SubmissionEffect};
use collaboration_contracts::AttemptEvidenceSink;
use collaboration_protocol::{CodexGeneration, SessionRef};
use provider_session_machines::session_owner::{
    DeliveryPlacement, DeliveryPlacementKind, EffectOutcome, OwnerEvent,
};
use router_clock::RuntimeClock;
use std::{sync::Arc, time::Duration};
use tokio::sync::oneshot;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

pub(crate) type ProviderDeliveryEvidence = RouteEffectEvidence<SessionRef, CodexGeneration>;

pub(crate) struct DeliveryEvidenceOffer {
    pub(crate) placement: DeliveryPlacement,
    pub(crate) recorded: oneshot::Sender<EffectOutcome>,
}

pub(crate) struct OwnerDeliveryEvidence {
    intake: Option<oneshot::Sender<DeliveryEvidenceOffer>>,
    retained: Option<Arc<dyn AttemptEvidenceSink>>,
    effect: ProviderDeliveryEvidence,
    clock: Arc<dyn RuntimeClock>,
    cancelled: CancellationToken,
}

impl OwnerDeliveryEvidence {
    pub(crate) fn new(
        intake: oneshot::Sender<DeliveryEvidenceOffer>,
        retained: Option<Arc<dyn AttemptEvidenceSink>>,
        effect: ProviderDeliveryEvidence,
        clock: Arc<dyn RuntimeClock>,
    ) -> Self {
        Self {
            intake: Some(intake),
            retained,
            effect,
            clock,
            cancelled: CancellationToken::new(),
        }
    }

    pub(crate) fn offer(
        &mut self,
        placement: DeliveryPlacement,
        completion: OwnerCompletion,
        tasks: &TaskTracker,
    ) {
        let intake = self.intake.take();
        let retained = self.retained.clone();
        let effect = placement_evidence(self.effect.clone(), placement.kind);
        let cancelled = self.cancelled.clone();
        let clock = self.clock.clone();
        tasks.spawn(async move {
            let record = async {
                if let Some(intake) = intake {
                    let (recorded, result) = oneshot::channel();
                    if intake.send(DeliveryEvidenceOffer { placement: placement.clone(), recorded }).is_ok() {
                        return result.await.unwrap_or(EffectOutcome::Failed);
                    }
                } else if let Some(writer) = retained
                    && writer.record(effect).await.is_ok()
                {
                    return EffectOutcome::Applied;
                }
                EffectOutcome::Failed
            };
            let outcome = tokio::select! {
                biased;
                () = cancelled.cancelled() => return,
                outcome = router_clock::timeout(clock.as_ref(), Duration::from_secs(10), record) => {
                    outcome.unwrap_or(EffectOutcome::Unknown)
                }
            };
            completion.send(OwnerEvent::DeliveryEvidence { placement, outcome });
        });
    }
}

impl Drop for OwnerDeliveryEvidence {
    fn drop(&mut self) {
        self.cancelled.cancel();
    }
}

pub(crate) fn placement_evidence(
    mut effect: ProviderDeliveryEvidence,
    kind: DeliveryPlacementKind,
) -> ProviderDeliveryEvidence {
    if let RouteEffectEvidence::ProviderAcp(evidence) = &mut effect {
        evidence.submission = match kind {
            DeliveryPlacementKind::Queue => SubmissionEffect::RouterQueued,
            DeliveryPlacementKind::Start | DeliveryPlacementKind::Steer => {
                SubmissionEffect::Dispatching
            }
        };
    }
    effect
}
