#![expect(clippy::large_futures, reason = "lint pass follows the crate split")]
//! The real Host resolves a pending Local cancel before admitting its original.
#[path = "support/local_mail_cancellation_fixture.rs"]
mod fixture;
use collaboration_client::{CollaborationClient, MessageSendRequest, PublicMessageContent};
use collaboration_protocol::{
    DeliveryOutcome, MessageDelivery, MessageSendResult, PushDeliveryState, PushRecordShowParams,
};
use fixture::{PendingLocalCancelFixture, TestResult};
use fleet_sync_protocol::RecipientOutcome;

#[tokio::test]
async fn cancel_before_original_is_settled_by_the_host_without_native_input_or_central()
-> TestResult {
    // A real pending Local control is fixture-arranged through public Inbox before Host startup.
    let fixture = PendingLocalCancelFixture::prepare().await?;
    let request = MessageSendRequest {
        item_id: Some(fixture.item.clone()),
        target: fixture.target.clone(),
        message: PublicMessageContent::HumanUser {
            text: "This cancelled original must never reach the peer"
                .to_owned()
                .try_into()?,
        },
        delivery: MessageDelivery::Auto,
        generation_guard: None,
    };
    let runtime = fixture.start_host().await?;
    let first: TestResult<_> = async {
        let client =
            CollaborationClient::connect(fixture.root(), "local-cancel-original", "1").await?;
        let sent = client
            .send_message(request.clone())
            .await
            .map_err(|error| format!("served original admission: {error:?}"))?;
        let MessageSendResult::Local(sent) = sent else {
            return Err("local original returned remote custody".into());
        };
        if sent.push_id.as_str() != fixture.item.as_str()
            || sent.target != fixture.target
            || sent.delivery_state != PushDeliveryState::Rejected
            || !matches!(
                &sent.receipt.outcome,
                DeliveryOutcome::NotSubmitted {
                    retryable: false,
                    ..
                }
            )
        {
            return Err(format!(
                "pending Local cancellation did not prevent the served original: {sent:?}"
            )
            .into());
        }
        fixture.require_no_native_input()?;
        let shown = client
            .router_show(PushRecordShowParams {
                caller: fixture.target.clone(),
                reference: sent.link.clone(),
            })
            .await?;
        let collaboration_client::protocol::MessageShowResult::Local(shown) = shown else {
            return Err("native proof expected a stored local push".into());
        };
        if shown.record.delivery_state != PushDeliveryState::Rejected
            || serde_json::to_value(shown.record.last_outcome.as_ref())?
                != serde_json::to_value(Some(&sent.receipt))?
        {
            return Err("served show omitted the committed prevented result".into());
        }
        Ok(serde_json::to_value(MessageSendResult::Local(sent))?)
    }
    .await;
    runtime.shutdown().await?;
    let first = first?;
    let cold = fixture.read_after_shutdown().await?;
    if !matches!(cold.outcome,Some(RecipientOutcome::Prevented {cancel}) if cancel == fixture.cancel)
        || cold.push.delivery_state != PushDeliveryState::Rejected
        || serde_json::to_value(
            cold.answer
                .result
                .as_ref()
                .ok_or("cold answer is pending")?,
        )? != first
    {
        return Err("cold native owner did not retain the canonical prevented result".into());
    }
    fixture.require_no_native_input()?;

    let runtime = fixture.start_host().await?;
    let replay: TestResult<_> = async {
        let client =
            CollaborationClient::connect(fixture.root(), "local-cancel-replay", "1").await?;
        let result = client
            .send_message(request)
            .await
            .map_err(|error| format!("served prevented replay: {error:?}"))?;
        fixture.require_no_native_input()?;
        Ok(serde_json::to_value(result)?)
    }
    .await;
    runtime.shutdown().await?;
    if replay? != first {
        return Err("same-ID replay changed its first Local answer".into());
    }
    let reopened = fixture.read_after_shutdown().await?;
    if reopened.answer.recorded_at != cold.answer.recorded_at
        || reopened.answer.target_identity != cold.answer.target_identity
        || reopened.push.settled_at != cold.push.settled_at
    {
        return Err("replay changed the first label, answer clock or settlement".into());
    }
    fixture.require_no_native_input()?;
    Ok(())
}
