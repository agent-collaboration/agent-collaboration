//! A production remote-wake item expires at its sender and never creates recipient work.
use super::{messaging_fixture::MessagingCase, messaging_stopped_owner};
use codex_router_test_support::FixtureResult;
use runtime_primitives::SyncTime;
use serde_json::{Value, json};

pub async fn sender_sees_expiry(scope: &mut MessagingCase<'_>) -> FixtureResult {
    let operation = "019f0000-0000-7000-8000-000000092091";
    let text = "B+ remote wake expires before its recipient returns";
    scope.recipient.stop_host().await?;
    let now = chrono::Utc::now();
    let due = now + chrono::Duration::seconds(1);
    let deadline = now + chrono::Duration::seconds(5);
    let request = json!({"operationId":operation,"requestedBy":{"kind":"owner"},"replace":false,
        "message":{"target":scope.recipient_target()?,"content":{"kind":"humanUser","text":text},
            "delivery":"auto","generationGuard":null},
        "timing":{"kind":"at","at":due.to_rfc3339_opts(chrono::SecondsFormat::Nanos,true)},
        "expiry":{"kind":"at","at":deadline.to_rfc3339_opts(chrono::SecondsFormat::Nanos,true)}});
    let expired = scope.sender.host()?.mcp_reply("wake_send", request).await?;
    assert!(expired.is_error, "{value}", value = expired.value);
    assert_eq!(expired.value.get("kind"), Some(&json!("deadlineExpired")));
    assert_eq!(expired.value.get("operationId"), Some(&json!(operation)));
    assert_eq!(
        scope
            .sender
            .host()?
            .mcp("wake_list", json!({"limit":10,"cursor":null}))
            .await?,
        json!({"records":[],"nextCursor":null})
    );
    let stopped = scope.stop_sender_for_readback().await?;
    let rows = messaging_stopped_owner::delivery_rows(&stopped).await?;
    let header = messaging_stopped_owner::header(&rows, "outbox", operation)?;
    assert_eq!(header.pointer("/value/id"), Some(&json!(operation)));
    assert_eq!(
        header.pointer("/value/validUntil"),
        Some(&json!(SyncTime::from_unix_millis(
            deadline.timestamp_millis()
        )))
    );
    assert_eq!(header.pointer("/value/submitAttempted"), Some(&json!(true)));
    let version = header
        .pointer("/value/version")
        .and_then(Value::as_u64)
        .ok_or("sender status version absent")?;
    // The caller deadline can precede the independent Expired publication. Both
    // legal observations must prove earlier real Mailbox custody, not local expiry.
    match header.pointer("/value/status/kind").and_then(Value::as_str) {
        Some("sent") => assert_eq!(version, 1),
        Some("expired") => assert!(version >= 2),
        _ => return Err(format!("wake never had pre-deadline Mailbox custody: {header}").into()),
    }
    drop(stopped);
    scope.recipient.reopen_host().await?;
    assert_eq!(
        scope
            .recipient
            .host()?
            .mcp("wake_list", json!({"limit":10,"cursor":null}))
            .await?,
        json!({"records":[],"nextCursor":null})
    );
    assert_eq!(scope.peer()?.count_after_barrier().await?, 0);
    let stopped = scope.recipient.stop_for_readback().await?;
    let received = messaging_stopped_owner::delivery_rows(&stopped).await?;
    assert!(!received.iter().any(|row| row.category == "inbox"));
    Ok(())
}
