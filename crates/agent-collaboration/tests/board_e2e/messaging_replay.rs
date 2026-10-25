//! Public identity conflicts and a real lost mailbox reply preserve one recipient effect.
use super::{messaging_fixture::MessagingCase, messaging_stopped_owner};
use board_central_test_support::{DispatchCallSelector, ObjectAddress, RuntimeControl};
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};

const DELIVERED: &str = "peerMessageWritten";

pub async fn changed_content_conflicts(scope: &mut MessagingCase<'_>) -> FixtureResult {
    let item = "019f0000-0000-7000-8000-000000092011";
    let text = "B+ original remote content survives a conflicting caller";
    let first = scope.send_cli(item, text).await?;
    scope
        .wait_sender_status(item, &json!({"kind":"delivered","acceptance":DELIVERED}))
        .await?;
    scope.peer()?.expect_marker(text).await?;
    let replay = scope
        .sender
        .host()?
        .mcp("message_send", scope.direct_request(item, text)?)
        .await?;
    assert_eq!(first.pointer("/result/record"), Some(&replay));
    let conflict = scope
        .sender
        .host()?
        .mcp_reply(
            "message_send",
            scope.direct_request(item, "B+ conflicting content must not replace the original")?,
        )
        .await?;
    assert!(conflict.is_error, "{value}", value = conflict.value);
    assert_eq!(conflict.value.get("kind"), Some(&json!("rejected")));
    assert_eq!(
        conflict.value.get("serviceKind"),
        Some(&json!("itemConflict"))
    );
    assert_eq!(conflict.value.get("effect"), Some(&json!("none")));
    let received = scope
        .recipient
        .host()?
        .mcp(
            "router_show",
            json!({
                "caller":scope.recipient_target()?,"reference":item
            }),
        )
        .await?;
    assert_eq!(received.pointer("/record/pushId"), Some(&json!(item)));
    assert_eq!(received.pointer("/record/body"), Some(&json!(text)));
    assert_eq!(
        received.pointer("/record/deliveryState"),
        Some(&json!("delivered"))
    );
    assert_eq!(scope.peer()?.count_after_barrier().await?, 1);
    assert_one_recipient_item(scope, item).await
}

pub async fn mailbox_retry_keeps_one_effect(scope: &mut MessagingCase<'_>) -> FixtureResult {
    let item = "019f0000-0000-7000-8000-000000092012";
    let text = "B+ one remote effect despite lost mailbox custody answer";
    scope
        .control(
            ObjectAddress::Registry,
            RuntimeControl::DropNextSelectedObjectReply {
                destination: scope.mailbox_address()?,
                call: DispatchCallSelector::MailboxSubmit,
            },
        )
        .await?;
    scope.send_cli(item, text).await?;
    let lost = scope
        .control(
            ObjectAddress::Registry,
            RuntimeControl::AwaitDroppedObjectReply,
        )
        .await?;
    assert_eq!(lost.pointer("/objectReply/kind"), Some(&json!("dropped")));
    let bytes: Vec<u8> = serde_json::from_value(
        lost.pointer("/objectReply/value/request")
            .ok_or("dropped request absent")?
            .clone(),
    )?;
    let submitted: Value = serde_json::from_slice(&bytes)?;
    assert_eq!(
        submitted.get("kind"),
        Some(&json!("central.mailbox-submit"))
    );
    assert_eq!(
        submitted.pointer("/request/item/value/id"),
        Some(&json!(item))
    );
    scope
        .wait_sender_status(item, &json!({"kind":"delivered","acceptance":DELIVERED}))
        .await?;
    scope.peer()?.expect_marker(text).await?;
    scope
        .sender
        .host()?
        .mcp("message_send", scope.direct_request(item, text)?)
        .await?;
    assert_eq!(scope.peer()?.count_after_barrier().await?, 1);
    assert_one_recipient_item(scope, item).await
}

async fn assert_one_recipient_item(scope: &mut MessagingCase<'_>, item: &str) -> FixtureResult {
    let stopped = scope.recipient.stop_for_readback().await?;
    let rows = messaging_stopped_owner::delivery_rows(&stopped).await?;
    let received = rows
        .iter()
        .filter(|row| row.category == "inbox")
        .collect::<Vec<_>>();
    assert_eq!(received.len(), 1);
    assert_eq!(
        received.first().ok_or("recipient item absent")?.identity,
        item
    );
    Ok(())
}
