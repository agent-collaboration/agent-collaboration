//! Calls release their real waits on disconnect and deadline.
use super::*;

#[tokio::test]
async fn a_client_that_disconnects_cancels_its_call_on_both_listeners() {
    // Arrange: a call that would hold its slot until the holder releases it.
    let directory = private_directory();
    let (identity, holder) =
        RequestSlotHolder::install(test_identity(), TEST_SERVICE_ID, directory.path())
            .await
            .expect("slot holder");
    let config = api_config(identity.build(), directory.path());
    let tcp = ServedApi::tcp(&config).await;
    let unix = ServedApi::unix(&config, &directory.path().join("api.sock"));

    for api in [&tcp, &unix] {
        // Act: the call is in flight, then its client goes away.
        api.abandon_tool_call(RequestSlotHolder::TOOL, holder.arguments())
            .await;

        // Assert: the handler was cancelled and released the Router, long before its bound.
        api.await_active_calls(0, Duration::from_secs(5)).await;
    }
    tcp.stop().await;
    unix.stop().await;
}

#[tokio::test]
async fn a_wake_wait_ends_at_its_bound_while_the_store_is_held() {
    // Arrange: a wake-up, then its store held by someone else past the waits' one-second bound.
    let directory = private_directory();
    let automation = automation_store(directory.path()).await;
    let config = api_config(
        (test_identity().with_automation_store(Arc::clone(&automation))).build(),
        directory.path(),
    );
    let api = ServedApi::unix(&config, &directory.path().join("api.sock"));
    let wake = api.call_tool("wake_send", wake_send_arguments()).await;
    let wakeup_id = structured(&wake)["definition"]["wakeupId"].clone();
    let cursor = structured(&wake)["latestEventCursor"].clone();
    let held = automation.lock().await;

    // Act: one wait starts from the current state and one resumes from the cursor; both need
    // the held store before they can observe anything.
    let ended = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(
            api.call_tool(
                "wake_wait_until_first_fire",
                json!({"wakeupId": wakeup_id, "timeoutSeconds": 1}),
            ),
            api.call_tool(
                "wake_wait_until_first_fire",
                json!({"wakeupId": wakeup_id, "after": cursor, "timeoutSeconds": 1}),
            ),
        )
    })
    .await;
    drop(held);

    // Assert: both ended at their bound while the store was still held. With no cursor yet
    // the start is unavailable; the resumed wait times out at the cursor it was given.
    let (starting, resuming) = ended.expect("the waits ended at their bound");
    assert_eq!(starting.is_error, Some(true), "{starting:?}");
    assert_eq!(structured(&starting)["kind"], "waitUnavailable");
    assert_eq!(structured(&resuming)["outcome"]["kind"], "timedOut");
    assert_eq!(structured(&resuming)["cursor"], cursor);
    api.stop().await;
}
