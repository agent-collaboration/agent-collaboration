//! Calls release their real native waits on disconnect and deadline.
use super::*;

#[tokio::test]
async fn a_client_that_disconnects_cancels_its_call_on_both_listeners() {
    // Arrange
    let directory = private_directory();
    let config = api_config(wake_application(directory.path()).await, directory.path());
    let tcp = ServedApi::tcp(&config).await;
    let unix = ServedApi::unix(&config, &directory.path().join("api.sock"));
    let wake = tcp.call_tool("wake_send", wake_send_arguments()).await;
    let wakeup_id = structured(&wake)["definition"]["wakeupId"].clone();
    let long_wait = json!({"wakeupId": wakeup_id, "timeoutSeconds": 1500});

    for api in [&tcp, &unix] {
        // Act: the call is in flight, then its client goes away.
        api.abandon_tool_call("wake_wait_until_first_fire", long_wait.clone())
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

#[tokio::test]
async fn a_client_that_disconnects_from_a_mutation_waiting_on_the_store_leaves_no_mutation() {
    // Arrange: a plain mutation belongs to its caller's future; its store is held.
    let directory = private_directory();
    let automation = automation_store(directory.path()).await;
    let config = api_config(
        (test_identity().with_automation_store(Arc::clone(&automation))).build(),
        directory.path(),
    );
    let tcp = ServedApi::tcp(&config).await;
    let unix = ServedApi::unix(&config, &directory.path().join("api.sock"));
    let held = automation.lock().await;

    // Act: each listener's client sends a wake, then goes away while it waits on the store.
    let mut abandoned = Vec::new();
    for (index, api) in [&tcp, &unix].into_iter().enumerate() {
        // One wake per target, so each listener's wake names its own session.
        let mut input = wake_send_arguments();
        input["message"]["target"]["sessionId"] = json!(format!("abandoned-{index}"));
        api.abandon_tool_call("wake_send", input.clone()).await;
        api.await_active_calls(0, Duration::from_secs(5)).await;
        abandoned.push((input, api));
    }
    drop(held);

    // Assert: neither abandoned call left a wake behind.
    let listed = tcp.call_tool("wake_list", json!({"limit": 10})).await;
    assert_eq!(structured(&listed)["records"], json!([]), "{listed:?}");
    // A valid retry is a sensitivity control: neither an invalid input nor a
    // disabled mutation path may make the no-effect check pass.
    for (input, api) in abandoned {
        let reply = api.call_tool("wake_send", input).await;
        assert_eq!(reply.is_error, Some(false), "{reply:?}");
    }
    let listed = tcp.call_tool("wake_list", json!({"limit": 10})).await;
    assert_eq!(
        structured(&listed)["records"].as_array().map(Vec::len),
        Some(2),
        "{listed:?}"
    );
    tcp.stop().await;
    unix.stop().await;
}
