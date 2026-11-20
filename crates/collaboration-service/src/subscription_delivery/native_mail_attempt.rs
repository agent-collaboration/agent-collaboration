//! One carrier call and its existing route reconciliation, without a custody or push write.
use super::subscription_push::SubscriptionPushStore;
use collaboration_contracts::NativeDeliveryResult;
use collaboration_contracts::NativeRefusalCode;
use collaboration_contracts::layer_zero::DeliveryRequest;
use collaboration_contracts::{UnsentCause, UnstoredAttemptEvidenceSink};
use collaboration_protocol::{DeliveryOutcome, DeliveryRejectionReason, MessageDelivery, PushKind};
use message_board::BoardError;

impl SubscriptionPushStore {
    pub(super) async fn dispatch_mail_attempt(
        &self,
        kind: PushKind,
        request: DeliveryRequest,
    ) -> Result<NativeDeliveryResult, BoardError> {
        if kind != PushKind::DirectMessage {
            return Err(BoardError::board_unavailable());
        }
        let target = request.target.clone();
        let mode = request.mode;
        let allow_hold = mode != MessageDelivery::Steer
            && !(mode == MessageDelivery::Queue
                && !self.delivery.supports_delivery_mode(&target, mode))
            && matches!(
                request.precondition,
                collaboration_contracts::DeliveryPrecondition::Unpinned
            );
        let sink = UnstoredAttemptEvidenceSink;
        let mut delivery = tokio::select! {
            () = self.shutdown.cancelled() => return Err(BoardError::board_unavailable()),
            receipt = self.delivery.deliver(request, &sink) => receipt.unwrap_or_else(|_| NativeDeliveryResult::unknown()),
        };
        let no_route = matches!(&delivery.receipt().outcome,DeliveryOutcome::Rejected(rejection) if rejection.reason==DeliveryRejectionReason::NoRoute);
        if no_route && allow_hold {
            delivery = NativeDeliveryResult::not_submitted(
                "no route currently claims the session; direct message held",
                true,
                UnsentCause::Holdable {
                    otherwise: NativeRefusalCode::Rejection(DeliveryRejectionReason::NoRoute),
                },
            );
        }
        Ok(delivery)
    }
}
