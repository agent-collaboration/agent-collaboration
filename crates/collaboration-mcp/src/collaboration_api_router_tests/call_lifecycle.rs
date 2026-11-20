//! Calls release their real waits when their client disconnects.
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
