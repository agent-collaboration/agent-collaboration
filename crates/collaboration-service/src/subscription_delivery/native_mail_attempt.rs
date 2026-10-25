//! One carrier call and its existing route reconciliation, without a custody or push write.
use super::subscription_push::SubscriptionPushStore;
use crate::{
    AttemptEvidenceSink, AttemptReconciliation, AttemptReconciliationContext, DeliveryFuture,
    LoadPolicy, NativeDeliveryResult, NativeRefusalCode, UnsentCause, layer_zero::DeliveryRequest,
};
use agent_automation::RouteEffectEvidence;
use collaboration_protocol::{
    CodexGeneration, DeliveryNextAction, DeliveryOutcome, DeliveryRejection,
    DeliveryRejectionReason, MessageDelivery, PushKind, SessionReachability, SessionRef,
};
use message_board::BoardError;
use tokio::sync::Mutex;

impl SubscriptionPushStore {
    pub(super) async fn dispatch_mail_attempt(
        &self,
        kind: PushKind,
        request: DeliveryRequest,
    ) -> Result<NativeDeliveryResult, BoardError> {
        if !matches!(
            kind,
            PushKind::DirectMessage | PushKind::BoardNotification | PushKind::SubscriptionActivity
        ) {
            return Err(BoardError::board_unavailable());
        }
        let target = request.target.clone();
        let mode = request.mode;
        let prepared = request.payload.clone();
        let allow_hold = mode != MessageDelivery::Steer
            && matches!(request.precondition, crate::DeliveryPrecondition::Unpinned);
        let sink = CapturedAttemptEvidence::default();
        let mut delivery = tokio::select! {
            () = self.shutdown.cancelled() => return Err(BoardError::board_unavailable()),
            receipt = self.delivery.deliver(request, &sink) => receipt.unwrap_or_else(|_| NativeDeliveryResult::unknown()),
        };
        if kind == PushKind::SubscriptionActivity
            && prepared.load_policy == LoadPolicy::LoadedOnly
            && delivery.receipt().reachability == Some(SessionReachability::ProviderAcp)
            && delivery.receipt().outcome == DeliveryOutcome::Queued
        {
            delivery = self.reconcile_queued(&target, &prepared, &sink).await;
        }
        if matches!(kind, PushKind::DirectMessage | PushKind::BoardNotification) {
            let no_route = matches!(&delivery.receipt().outcome,DeliveryOutcome::Rejected(rejection) if rejection.reason==DeliveryRejectionReason::NoRoute);
            if no_route && !self.delivery.supports_delivery_mode(&target, mode) {
                delivery = NativeDeliveryResult::rejected(
                    DeliveryRejection {
                        native_failure: None,
                        reason: DeliveryRejectionReason::QueueUnsupported,
                        next_action: DeliveryNextAction::CorrectRequest,
                        client_code: None,
                        detail: Some(
                            "Queue delivery isn't supported for Claude Code terminals".to_owned(),
                        ),
                        claims: None,
                    },
                    UnsentCause::Refused(NativeRefusalCode::Rejection(
                        DeliveryRejectionReason::QueueUnsupported,
                    )),
                );
            } else if no_route && allow_hold {
                delivery = NativeDeliveryResult::not_submitted(
                    "no route currently claims the session; direct message held",
                    true,
                    UnsentCause::Holdable {
                        otherwise: NativeRefusalCode::Rejection(DeliveryRejectionReason::NoRoute),
                    },
                );
            }
        }
        Ok(delivery)
    }

    async fn reconcile_queued(
        &self,
        target: &SessionRef,
        prepared: &crate::layer_zero::PreparedPush,
        sink: &CapturedAttemptEvidence,
    ) -> NativeDeliveryResult {
        let Some(recorded) = sink.recorded.lock().await.clone() else {
            return NativeDeliveryResult::unknown();
        };
        let provider_evidence = match &recorded {
            RouteEffectEvidence::ProviderAcp(evidence) => Some(evidence.clone()),
            _ => None,
        };
        let context = AttemptReconciliationContext {
            target: target.clone(),
            prepared_push_id: prepared.push_id.clone(),
            mode: MessageDelivery::Auto,
            recorded,
        };
        match self.delivery.reconcile_attempt(context).await {
            Ok(AttemptReconciliation::Accepted(receipt)) => {
                NativeDeliveryResult::reconciled_acceptance(*receipt)
                    .unwrap_or_else(NativeDeliveryResult::unknown)
            }
            Ok(AttemptReconciliation::KnownNotSubmitted) => NativeDeliveryResult::not_submitted(
                "queued input was not submitted",
                true,
                UnsentCause::Holdable {
                    otherwise: NativeRefusalCode::QueuedNotSubmitted,
                },
            )
            .with_reachability(SessionReachability::ProviderAcp),
            Ok(AttemptReconciliation::StillUnknown) | Err(_) => match provider_evidence {
                Some(evidence) => {
                    NativeDeliveryResult::unknown().with_provider_acp_evidence(evidence)
                }
                None => NativeDeliveryResult::unknown(),
            },
        }
    }
}

#[derive(Default)]
struct CapturedAttemptEvidence {
    recorded: Mutex<Option<RouteEffectEvidence<SessionRef, CodexGeneration>>>,
}
impl AttemptEvidenceSink for CapturedAttemptEvidence {
    fn record(
        &self,
        evidence: RouteEffectEvidence<SessionRef, CodexGeneration>,
    ) -> DeliveryFuture<'_, ()> {
        Box::pin(async move {
            *self.recorded.lock().await = Some(evidence);
            Ok(())
        })
    }
}
