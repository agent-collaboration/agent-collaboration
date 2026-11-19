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
    let clock = fixed_clock();
    let config = api_config(
        (test_identity()
            .with_runtime_clock(clock.clone())
            .with_automation_store(Arc::clone(&automation)))
        .build(),
        directory.path(),
    );
    let tcp = ServedApi::tcp(&config).await;
    let unix = ServedApi::unix(&config, &directory.path().join("api.sock"));
    let before = store_snapshot(&automation, &clock, directory.path()).await;
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

    // Assert: neither abandoned call left a wake, a receipt or an event behind.
    let listed = tcp.call_tool("wake_list", json!({"limit": 10})).await;
    assert_eq!(structured(&listed)["records"], json!([]), "{listed:?}");
    {
        use router_clock::RuntimeClock as _;
        let mut store = automation.lock().await;
        for (input, _) in &abandoned {
            let operation: collaboration_protocol::OperationId =
                serde_json::from_value(input["operationId"].clone()).expect("operation id");
            assert!(
                matches!(
                    store.read_operation(&operation).await,
                    Err(automation_storage::StorageError::OperationNotFound)
                ),
                "an abandoned call left its operation receipt"
            );
        }
        let events = store
            .read_event_history(&automation_storage::EventHistoryQuery {
                after: None,
                now_ms: clock.utc_now().timestamp_millis(),
                limit: 100,
            })
            .await
            .expect("automation events");
        let automation_storage::EventHistoryRead::Page(page) = events else {
            panic!("the event history expired on a still clock");
        };
        assert!(page.records.is_empty(), "an abandoned call left an event");
    }
    // The whole store, not just the submitted ids: no receipt under any id, and
    // no event appended, even one removed again.
    assert_eq!(
        store_snapshot(&automation, &clock, directory.path()).await,
        before,
        "an abandoned call changed the receipt inventory or the event high-water mark"
    );
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
    // The snapshot sees the retries' receipts and events, so its equality above can fail.
    assert_eq!(
        store_snapshot(&automation, &clock, directory.path()).await,
        StoreSnapshot {
            receipts: before.receipts + 2,
            event_high_water: before.event_high_water + 2,
        },
        "the retries did not each add one receipt and one event"
    );
    // The same event read sees the retries' events, so its empty answer above can fail.
    let events = automation
        .lock()
        .await
        .read_event_history(&automation_storage::EventHistoryQuery {
            after: None,
            now_ms: router_clock::RuntimeClock::utc_now(clock.as_ref()).timestamp_millis(),
            limit: 100,
        })
        .await
        .expect("automation events");
    let automation_storage::EventHistoryRead::Page(page) = events else {
        panic!("the event history expired on a still clock");
    };
    assert_eq!(page.records.len(), 2, "one created event per retried wake");
    tcp.stop().await;
    unix.stop().await;
}

/// The automation store's whole receipt inventory and its event high-water mark.
#[derive(Debug, PartialEq, Eq)]
struct StoreSnapshot {
    receipts: i64,
    event_high_water: i64,
}

async fn store_snapshot(
    automation: &Arc<Mutex<automation_storage::AutomationStore>>,
    clock: &Arc<router_clock::ManualClock>,
    directory: &std::path::Path,
) -> StoreSnapshot {
    use sqlx::{ConnectOptions as _, Connection as _};
    let events = automation
        .lock()
        .await
        .read_event_history(&automation_storage::EventHistoryQuery {
            after: None,
            now_ms: router_clock::RuntimeClock::utc_now(clock.as_ref()).timestamp_millis(),
            limit: 100,
        })
        .await
        .expect("automation events");
    let automation_storage::EventHistoryRead::Page(page) = events else {
        panic!("the event history expired on a still clock");
    };
    // No store method lists every receipt, so a read-only connection counts them.
    let mut reader = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(directory.join("automation.sqlite"))
        .read_only(true)
        .connect()
        .await
        .expect("read-only automation connection");
    let receipts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM operation_receipts")
        .fetch_one(&mut reader)
        .await
        .expect("receipt inventory");
    reader.close().await.expect("close the reader");
    StoreSnapshot {
        receipts,
        event_high_water: page.next.sequence,
    }
}
