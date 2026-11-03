//! Answer-kind and causal deadline proof; no positive receiving-store ACK is manufactured.
use super::*;
use fleet_sync_protocol::PushResult;
use std::future::{pending, ready};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A causally released deadline must turn a silent Project answer into retryable uncertainty without waiting on another alarm."
)]
async fn a_silent_project_answer_waits_for_its_causal_deadline_then_remains_retryable() -> TestResult
{
    let (release, expired) = futures_channel::oneshot::channel::<()>();
    let answer = bounded_project_answer(pending(), async move {
        let _ = expired.await;
    });
    tokio::pin!(answer);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(answer.as_mut().poll(&mut context).is_pending());
    release
        .send(())
        .map_err(|()| "deadline receiver was dropped")?;
    assert!(
        matches!(answer.await,Err(CallFailure::Retryable {reason}) if reason.code=="projectStatusDeadline")
    );
    Ok(())
}

#[tokio::test]
async fn a_push_answer_cannot_complete_a_project_status_call() {
    let answer = ClientAnswer::Crate(ServerCallAnswer::Pushed(PushResult::Refused {
        reason: DeliveryRefusal::NotRecipient,
    }));
    assert!(matches!(
        bounded_project_answer(ready(Ok(answer)), pending()).await,
        Err(CallFailure::AnswerLost)
    ));
}

#[tokio::test]
async fn a_correlated_negative_status_answer_keeps_its_refusal() {
    let answer = ClientAnswer::Crate(ServerCallAnswer::Published(
        StatusPublicationResult::Refused {
            reason: DeliveryRefusal::NotSubmitter,
        },
    ));
    assert!(matches!(
        bounded_project_answer(ready(Ok(answer)), pending()).await,
        Ok(StatusPublicationResult::Refused {
            reason: DeliveryRefusal::NotSubmitter
        })
    ));
}
