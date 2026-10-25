//! Real sender admission observes the full Mailbox while reserved cancellation still settles.
use super::{messaging_cancel::finish_prevention, messaging_fixture::MessagingCase};
use board_central_test_support::RuntimeControl;
use codex_router_test_support::FixtureResult;
use serde_json::json;

pub async fn full_mailbox(scope: &mut MessagingCase<'_>) -> FixtureResult {
    let original = "019f0000-0000-7000-8000-000000092101";
    let rejected = "019f0000-0000-7000-8000-000000092102";
    let cancel = "019f0000-0000-7000-8000-000000092103";
    scope.recipient.stop_host().await?;
    scope
        .control(
            scope.mailbox_address()?,
            RuntimeControl::SmallMailboxCapacity,
        )
        .await?;
    scope
        .send_cli(
            original,
            "B+ retained payload consumes the one ordinary slot",
        )
        .await?;
    scope
        .wait_sender_status(original, &json!({"kind":"sent","seq":1}))
        .await?;
    scope
        .send_cli(
            rejected,
            "B+ refused payload must never enter recipient custody",
        )
        .await?;
    scope
        .wait_sender_status(
            rejected,
            &json!({"kind":"refused","reason":{"kind":"mailboxFull"}}),
        )
        .await?;
    assert_eq!(
        scope.cancel(original, cancel).await?.get("outcome"),
        Some(&json!({"kind":"pending"}))
    );
    finish_prevention(scope, original, cancel, Some(rejected)).await
}
