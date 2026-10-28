//! A committed public notification survives a pre-outbox write fault and joined Host crash.
use super::{
    fixture::{fields, header},
    messaging_fixture::MessagingCase,
    messaging_stopped_owner, records_reopen,
};
use board_sync_protocol::BoardDeliveryBody;
use codex_router_test_support::FixtureResult;
use fleet_sync_protocol::DeliveryEnvelope;
use message_board::{IntentState, NotifyIntent, PostId};
use message_board_storage::MailHandoffItem;
use serde_json::json;
use std::time::Duration;

pub async fn committed_handoff_before_outbox(scope: &mut MessagingCase<'_>) -> FixtureResult {
    let post: PostId = "019f0000-0000-7000-8000-000000092151"
        .to_owned()
        .try_into()?;
    let text = "B+ committed handoff survives the pre-outbox process restart";
    scope.sender.create().await?;
    let stopped = scope.stop_sender_for_readback().await?;
    messaging_stopped_owner::install_fault(
        &stopped,
        messaging_stopped_owner::MailFault::BeforeOutboxInsertion,
    )
    .await?;
    drop(stopped);
    scope.sender.reopen_host().await?;
    scope.sender.accept("thread_create", fields(header(&scope.sender.owner), json!({
        "projectId":scope.sender.project,"postId":post,
        "about":{"kind":"project","subject":{"line":"B+ pre-outbox recovery"}},
        "text":text,"references":[],"role":null,"watch":false,
        "addressees":[{"kind":"actor","recipient":{"kind":"session","session":scope.recipient_target()?}}]
    }))?).await?;
    wait_frozen_intent(scope, &post).await?;
    let stopped = scope
        .sender
        .host
        .take()
        .ok_or("sender Host is already stopped")?
        .crash_for_readback()
        .await?;
    let handoffs = records_reopen::stopped_values(
        scope.sender,
        "SELECT item AS value FROM mail_handoffs ORDER BY handoff_seq",
    )
    .await?;
    let [frozen] = handoffs.as_slice() else {
        return Err("expected one committed frozen handoff".into());
    };
    let MailHandoffItem::Delivery(envelope) = serde_json::from_str(frozen)? else {
        return Err("intent became a cancel group".into());
    };
    let DeliveryEnvelope::Item(item) = *envelope else {
        return Err("intent became a cancel".into());
    };
    let BoardDeliveryBody::Notification(body) = item.body() else {
        return Err("frozen handoff lost its notification body".into());
    };
    assert_eq!(body.excerpt.as_str(), text);
    assert_eq!(
        serde_json::to_value(item.recipient())?.get("address"),
        Some(&json!(scope.recipient_target()?))
    );
    let original = item.id().as_uuid().to_string();
    let frozen_digest = item.digest().as_str().to_owned();
    let rows = messaging_stopped_owner::delivery_rows(&stopped).await?;
    assert!(
        !rows.iter().any(|row| row.category == "outbox"),
        "outbox insertion was not prevented"
    );
    assert_eq!(scope.peer()?.count_after_barrier().await?, 0);
    messaging_stopped_owner::remove_faults(&stopped).await?;
    drop(stopped);
    // The old owning handoff, read before restart, is input to recovery's identity check.
    scope.sender.reopen_host().await?;
    scope
        .wait_sender_status(
            &original,
            &json!({"kind":"delivered","acceptance":"peerMessageWritten"}),
        )
        .await?;
    scope
        .peer()?
        .expect_marker(&original)
        .await
        .map_err(|error| format!("recovered {original} peer notification: {error}"))?;
    assert_eq!(
        scope
            .peer()?
            .count_matching_after_barrier(&original)
            .await?,
        1
    );
    let stopped = scope.recipient.stop_for_readback().await?;
    let received = messaging_stopped_owner::delivery_rows(&stopped).await?;
    assert_eq!(
        received
            .iter()
            .filter(|row| row.category == "inbox")
            .count(),
        1
    );
    let retained = messaging_stopped_owner::header(&received, "inbox", &original)?;
    assert_eq!(retained.get("digest"), Some(&json!(frozen_digest)));
    Ok(())
}

async fn wait_frozen_intent(scope: &MessagingCase<'_>, post: &PostId) -> FixtureResult {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            for record in scope.sender.feed_records("notifyIntent").await? {
                let intent: NotifyIntent = serde_json::from_value(record)?;
                if intent.cause == post.clone().into()
                    && matches!(intent.state(), IntentState::HandedOff { .. })
                {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|error| format!("post {post:?} did not reach HandedOff before crash: {error}"))?
}
