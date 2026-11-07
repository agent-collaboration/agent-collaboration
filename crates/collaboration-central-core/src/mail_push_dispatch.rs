//! Validate Push replies and preserve possible reach when a sent call's answer is lost.
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use durable_steps::{BoxFuture, CallFailure, CallId, CallRetryReason, StoreError};
use fleet_mail_delivery::PushCall;
use fleet_protocol::{ClientAnswer, DeliveryItemRef, DeliveryTypes, PushResult, ServerCallAnswer};
use futures_util::future::{Either, select};
use std::{future::Future, sync::Arc};

#[cfg(test)]
#[path = "mail_push_dispatch_test.rs"]
mod tests;

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
    match fleet_mail_delivery::classify_delivery_store_error(error) {
        fleet_mail_delivery::DeliveryTransactionFailure::BindingUnavailable { reason, .. } => {
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
        // Enqueue already succeeded; silence cannot prove that B took no
        // custody. Deduplicated calls retry AnswerLost without erasing reach.
        Either::Right(_) => Err(CallFailure::AnswerLost),
    }
}
