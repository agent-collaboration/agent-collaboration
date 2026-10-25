//! A real Unix peer write whose journal receipt cannot commit remains uncertain.
use super::{messaging_fixture::MessagingCase, messaging_stopped_owner};
use codex_router_test_support::FixtureResult;
use serde_json::json;

pub async fn carrier_receipt_is_lost(scope: &mut MessagingCase<'_>) -> FixtureResult {
    let item = "019f0000-0000-7000-8000-000000092171";
    let text = "B+ actual carrier write loses its journal receipt";
    let stopped = scope.recipient.stop_for_readback().await?;
    messaging_stopped_owner::install_fault(
        &stopped,
        messaging_stopped_owner::MailFault::BeforeCarrierReceiptPersistence,
    )
    .await?;
    drop(stopped);
    scope.recipient.reopen_host().await?;
    scope.send_cli(item, text).await?;
    scope.peer()?.expect_marker(text).await?;
    scope
        .wait_sender_status(item, &json!({"kind":"uncertain"}))
        .await?;
    assert_eq!(scope.peer()?.count_after_barrier().await?, 1);
    let target = scope.recipient_target()?;
    let received = scope
        .recipient
        .host()?
        .mcp("router_show", json!({"caller":target,"reference":item}))
        .await?;
    assert_eq!(received.pointer("/record/pushId"), Some(&json!(item)));
    assert_eq!(received.pointer("/record/body"), Some(&json!(text)));
    assert_eq!(
        received.pointer("/record/deliveryState"),
        Some(&json!("outcome-unknown"))
    );
    let stopped = scope.recipient.stop_for_readback().await?;
    let rows = messaging_stopped_owner::delivery_rows(&stopped).await?;
    let header = messaging_stopped_owner::header(&rows, "inbox", item)?;
    assert_eq!(header.pointer("/progress/kind"), Some(&json!("final")));
    assert_eq!(
        header.pointer("/progress/value/outcome"),
        Some(&json!({"kind":"uncertain"}))
    );
    messaging_stopped_owner::remove_faults(&stopped).await?;
    drop(stopped);
    scope.recipient.reopen_host().await?;
    scope.send_cli(item, text).await?;
    scope
        .wait_sender_status(item, &json!({"kind":"uncertain"}))
        .await?;
    assert_eq!(scope.peer()?.count_after_barrier().await?, 1);
    Ok(())
}
