//! Real sender admission refuses a full mailbox before custody and preserves its admitted item.
use super::messaging_fixture::MessagingCase;
use board_central_test_support::RuntimeControl;
use codex_router_test_support::FixtureResult;
use serde_json::json;

pub async fn full_mailbox(scope: &mut MessagingCase<'_>) -> FixtureResult {
    let original = "019f0000-0000-7000-8000-000000092101";
    let rejected = "019f0000-0000-7000-8000-000000092102";
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
    scope.recipient.reopen_host().await?;
    scope
        .peer()?
        .expect_marker("B+ retained payload consumes the one ordinary slot")
        .await?;
    assert_eq!(scope.peer()?.count_after_barrier().await?, 1);
    assert_eq!(
        scope
            .peer()?
            .count_matching_after_barrier("B+ refused payload")
            .await?,
        0
    );
    Ok(())
}
