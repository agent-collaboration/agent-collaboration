//! The Push protocol boundary validates replies and makes deadline expiry retryable.
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use durable_steps::{BoxFuture, CallFailure, CallId, CallRetryReason, StoreError};
use fleet_sync_delivery::PushCall;
use fleet_sync_protocol::{
    ClientAnswer, DeliveryItemRef, DeliveryTypes, PushResult, ServerCallAnswer,
};
use futures_util::future::{Either, select};
use std::{future::Future, sync::Arc};

type PushAnswer = PushResult<<BoardDeliveryTypes as DeliveryTypes>::Acceptance>;
pub(crate) type PushDispatcher = Arc<
    dyn Fn(
            PushCall<BoardDeliveryTypes>,
            DeliveryItemRef,
            CallId,
        ) -> BoxFuture<'static, Result<PushAnswer, CallFailure>>
        + Send
        + Sync,
>;

pub(crate) fn store_failure(error: StoreError) -> CallFailure {
    match fleet_sync_delivery::classify_delivery_store_error(error) {
        fleet_sync_delivery::DeliveryTransactionFailure::BindingUnavailable { reason, .. } => {
            CallFailure::Retryable {
                reason: CallRetryReason {
                    code: reason.code().into(),
                    detail: "Fleet sender binding is unavailable".into(),
                },
            }
        }
        _ => CallFailure::AnswerLost,
    }
}

pub(crate) async fn bounded_answer(
    answer: impl Future<Output = Result<ClientAnswer<BoardConsumerProtocol>, CallFailure>> + Send,
    deadline: impl Future<Output = ()> + Send,
) -> Result<PushAnswer, CallFailure> {
    match select(Box::pin(answer), Box::pin(deadline)).await {
        Either::Left((Ok(ClientAnswer::Crate(ServerCallAnswer::Pushed(answer))), _)) => {
            PushCall::<BoardDeliveryTypes>::validated_answer(answer)
        }
        Either::Left((Err(error), _)) => Err(error),
        Either::Left((Ok(_), _)) => Err(CallFailure::AnswerLost),
        Either::Right(_) => Err(CallFailure::Retryable {
            reason: CallRetryReason {
                code: "pushDeadline".into(),
                detail: "recipient Push answer deadline elapsed".into(),
            },
        }),
    }
}
