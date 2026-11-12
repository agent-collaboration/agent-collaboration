//! Calls release their real native waits on disconnect.
use super::*;
use std::{sync::Arc, time::Duration};

#[tokio::test]
async fn a_client_that_disconnects_cancels_its_call_on_both_listeners() {
    // Arrange: a caught-up journal read that would otherwise wait thirty seconds for records.
    let directory = private_directory();
    let journal = lifecycle_observation::ObservationJournal::open(
        &directory.path().join("session-registry.sqlite"),
        "00000000-0000-4000-8000-000000000003"
            .to_owned()
            .try_into()
            .expect("journal identity"),
    )
    .await
    .expect("lifecycle journal");
    let journal = Arc::new(lifecycle_observation::LifecycleStore::new(journal));
    let endpoint = json!({"serviceId": TEST_SERVICE_ID, "endpointId": "codex-local"});
    let description = serde_json::from_value(json!({
        "endpoint":endpoint,"label":"Codex fixture",
        "availability":{"state":"available","observedAt":"2026-10-08T00:00:00Z"},
        "channels":[{"kind":"nativeCodex","transport":"unixWebSocket","path":"codex-native.sock",
            "schemaDigest":null,"generation":{"serviceEpoch":crate::api_test_harness::TEST_SERVICE_EPOCH,"generation":1}}]
    }))
    .expect("endpoint description");
    let application = test_identity()
        .with_endpoints(vec![description])
        .expect("published endpoint")
        .with_journal(Arc::clone(&journal))
        .build();
    let config = api_config(application, directory.path());
    let tcp = ServedApi::tcp(&config).await;
    let unix = ServedApi::unix(&config, &directory.path().join("api.sock"));
    let status = structured(&tcp.call_tool("journal_status", json!({})).await);
    let long_wait = json!({
        "endpoint": endpoint,
        "after": {
            "journalId": status["bounds"]["journalId"],
            "sequence": status["bounds"]["lastSequence"]
        },
        "pageSize": 1,
        "waitMilliseconds": 30000
    });

    for api in [&tcp, &unix] {
        // Act: the call is in flight, then its client goes away.
        api.abandon_tool_call("journal_read", long_wait.clone())
            .await;

        // Assert: the handler was cancelled and released the Router, long before its bound.
        api.await_active_calls(0, Duration::from_secs(5)).await;
    }
    tcp.stop().await;
    unix.stop().await;
    drop(config);
    if let Ok(journal) = Arc::try_unwrap(journal) {
        journal.close().await;
    }
}
