//! Lost custody knowledge cannot promise prevention before the actual recipient decides.
use super::{messaging_fixture::MessagingCase, messaging_stopped_owner};
use board_central_test_support::{DispatchCallSelector, ObjectAddress, RuntimeControl};
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};

pub async fn lost_custody_then_cancel(scope: &mut MessagingCase<'_>) -> FixtureResult {
    let original = "019f0000-0000-7000-8000-000000092181";
    let cancel = "019f0000-0000-7000-8000-000000092182";
    scope.recipient.stop_host().await?;
    let mailbox = scope.mailbox_address()?;
    // Hold the independent Sent publication as well as the retried submit.
    scope
        .hold_object_call(mailbox.clone(), scope.sender_connection()?, None)
        .await?;
    scope
        .hold_object_call(
            ObjectAddress::Registry,
            mailbox.clone(),
            Some(DispatchCallSelector::MailboxSubmit),
        )
        .await?;
    scope
        .send_cli(original, "B+ original with a genuinely lost custody answer")
        .await?;
    let held = scope
        .control(ObjectAddress::Registry, RuntimeControl::AwaitHeldDispatch)
        .await?;
    assert_submit_identity(&held, "/dispatch/value/request", original)?;
    scope
        .control(
            ObjectAddress::Registry,
            RuntimeControl::DropNextSelectedObjectReply {
                destination: mailbox.clone(),
                call: DispatchCallSelector::MailboxSubmit,
            },
        )
        .await?;
    scope
        .control(
            ObjectAddress::Registry,
            RuntimeControl::ResumeDispatchAndHoldNextSelected {
                destination: mailbox.clone(),
                call: DispatchCallSelector::MailboxSubmit,
            },
        )
        .await?;
    let lost = scope
        .control(
            ObjectAddress::Registry,
            RuntimeControl::AwaitDroppedObjectReply,
        )
        .await?;
    assert_eq!(lost.pointer("/objectReply/kind"), Some(&json!("dropped")));
    assert_submit_identity(&lost, "/objectReply/value/request", original)?;
    scope
        .control(mailbox.clone(), RuntimeControl::AwaitHeldDispatch)
        .await?;
    scope
        .wait_sender_status(original, &json!({"kind":"queued"}))
        .await?;
    assert_eq!(
        scope.cancel(original, cancel).await?.get("outcome"),
        Some(&json!({"kind":"pending"}))
    );
    assert_eq!(scope.peer()?.count_after_barrier().await?, 0);
    scope
        .control(ObjectAddress::Registry, RuntimeControl::AwaitHeldDispatch)
        .await?;
    scope.resume_object_calls(ObjectAddress::Registry).await?;
    scope.resume_object_calls(mailbox).await?;
    scope
        .wait_sender_status(original, &json!({"kind":"sent","seq":1}))
        .await?;
    finish_prevention(scope, original, cancel, None).await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
fn assert_submit_identity(observed: &Value, pointer: &str, item: &str) -> FixtureResult {
    let bytes: Vec<u8> = serde_json::from_value(
        observed
            .pointer(pointer)
            .ok_or("fault request absent")?
            .clone(),
    )?;
    let request: Value = serde_json::from_slice(&bytes)?;
    assert_eq!(request.get("kind"), Some(&json!("central.mailbox-submit")));
    assert_eq!(
        request.pointer("/request/item/value/id"),
        Some(&json!(item))
    );
    Ok(())
}

pub async fn finish_prevention(
    scope: &mut MessagingCase<'_>,
    original: &str,
    cancel: &str,
    rejected: Option<&str>,
) -> FixtureResult {
    assert_eq!(scope.peer()?.count_after_barrier().await?, 0);
    // No peer is available during recipient arbitration. Its actual SDK may hold
    // the original or process the control first; neither ordering may inject.
    scope.stop_peer().await?;
    scope.recipient.reopen_host().await?;
    scope.wait_prevented(original, cancel).await?;
    scope
        .wait_sender_status(original, &json!({"kind":"cancelled"}))
        .await?;
    let recipient = scope
        .recipient
        .host()?
        .mcp(
            "router_show",
            json!({
                "caller":scope.recipient_target()?,"reference":original
            }),
        )
        .await?;
    assert_eq!(
        recipient.pointer("/record/deliveryState"),
        Some(&json!("rejected"))
    );
    let stopped_recipient = scope.recipient.stop_for_readback().await?;
    let received = messaging_stopped_owner::delivery_rows(&stopped_recipient).await?;
    let header = messaging_stopped_owner::header(&received, "inbox", original)?;
    assert_eq!(header.pointer("/progress/kind"), Some(&json!("final")));
    assert_eq!(
        header.pointer("/progress/value/outcome"),
        Some(&json!({"kind":"prevented","value":{"cancel":cancel}}))
    );
    if let Some(rejected) = rejected {
        assert!(!received.iter().any(|row| row.identity == rejected));
    }
    drop(stopped_recipient);
    let before = settled_sender_headers(scope, original, cancel).await?;
    scope.sender.reopen_host().await?;
    assert_eq!(
        scope.cancel(original, cancel).await?.get("outcome"),
        Some(&json!({"kind":"prevented"}))
    );
    let after = settled_sender_headers(scope, original, cancel).await?;
    // Stored terminal headers are frozen input to the replay, including versions.
    assert_eq!(before, after);
    scope.start_peer().await?;
    scope.recipient.reopen_host().await?;
    assert_eq!(scope.peer()?.count_after_barrier().await?, 0);
    Ok(())
}

async fn settled_sender_headers(
    scope: &mut MessagingCase<'_>,
    original: &str,
    cancel: &str,
) -> FixtureResult<[Value; 2]> {
    let stopped = scope.stop_sender_for_readback().await?;
    let rows = messaging_stopped_owner::delivery_rows(&stopped).await?;
    let original = messaging_stopped_owner::header(&rows, "outbox", original)?;
    let cancel = messaging_stopped_owner::header(&rows, "outbox", cancel)?;
    for header in [&original, &cancel] {
        assert_eq!(
            header.pointer("/value/status"),
            Some(&json!({"kind":"cancelled"}))
        );
        assert!(
            header
                .pointer("/value/version")
                .and_then(Value::as_u64)
                .is_some_and(|version| version >= 2)
        );
    }
    assert_eq!(
        cancel.pointer("/value/cancel"),
        Some(&json!({"kind":"prevented"}))
    );
    Ok([original, cancel])
}
