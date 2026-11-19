//! A caller that disconnects while its mutation waits to commit leaves no mutation behind.
use super::*;
use crate::api_test_harness::TEST_SERVICE_ID;
use std::time::Duration;

fn create_arguments(directory: &std::path::Path, operation_id: &str) -> Value {
    let endpoint = json!({"serviceId":SERVICE_ID,"endpointId":"claude-code"});
    let actor = json!({"endpoint":endpoint,"sessionId":"caller-session"});
    json!({
        "operationId":operation_id,
        "endpoint":endpoint,
        "generation":generation(),
        "workingDirectory":directory,
        "createdBy":actor,
        "approver":actor,
        "access":"workspace-write"
    })
}

#[tokio::test]
async fn a_client_that_disconnects_from_a_mutation_waiting_on_the_store_leaves_no_mutation() {
    // Arrange: every create waits behind the backend's gate before it commits, and the test
    // holds the gate. Two creates are abandoned and two are retried.
    assert_eq!(
        SERVICE_ID, TEST_SERVICE_ID,
        "one service for endpoints and identity"
    );
    let directory = tempfile::tempdir_in("/tmp").expect("private test directory");
    let operations = [
        collaboration_protocol::OperationId::generate(),
        collaboration_protocol::OperationId::generate(),
    ];
    let mut backend = ScriptedProviderBackend::new();
    for operation in operations.iter().chain(operations.iter()) {
        backend = backend.answer(
            "create",
            json!({"gate": true, "result": {"admission":"admitted",
                "operation": operation_snapshot(operation.as_str(), "conversationCreate")}}),
        );
    }
    // Each retried create then waits for its settlement.
    for operation in &operations {
        backend = backend.answer(
            "wait",
            json!({"result": {
                "operation": terminal_snapshot(operation.as_str(), "conversationCreate"),
                "output": {"kind":"available","settlement":{"kind":"created",
                    "target":{"endpoint":{"serviceId":SERVICE_ID,"endpointId":"claude-code"},"sessionId":"provider-thread"},
                    "effectiveSettings":{"requestedPolicy":{"access":"workspace-write"},"mappingStatus":"verified","authentication":"authenticated"}}}
            }}),
        );
    }
    let identity = test_identity()
        .with_endpoints(vec![
            serde_json::from_value(provider_endpoint()).expect("provider endpoint"),
        ])
        .expect("endpoint directory")
        .with_provider_conversation_backend(Arc::new(backend.clone()));
    let config = api_config(identity.build(), directory.path());
    let tcp = ServedApi::tcp(&config).await;
    let unix = ServedApi::unix(&config, &directory.path().join("api.sock"));
    let gate = backend.gate();
    let held = gate.lock().await;

    // Act: each listener's client sends a create, then goes away while it waits on the gate.
    for (api, operation) in [&tcp, &unix].into_iter().zip(&operations) {
        api.abandon_tool_call(
            "conversation_create",
            create_arguments(directory.path(), operation.as_str()),
        )
        .await;
        api.await_active_calls(0, Duration::from_secs(5)).await;
    }
    drop(held);

    // A valid retry is the sensitivity control. The gate is first-come first-served, so an
    // abandoned create still running would commit before the retries do.
    for (api, operation) in [&tcp, &unix].into_iter().zip(&operations) {
        let reply = api
            .call_tool(
                "conversation_create",
                create_arguments(directory.path(), operation.as_str()),
            )
            .await;
        assert_eq!(reply.is_error, Some(false), "{reply:?}");
    }

    // Assert: only the two retries committed; both abandoned creates reached the backend
    // and were dropped before their commit.
    let committed = backend.committed();
    let committed_operations = committed
        .iter()
        .map(|(_, request)| request["operationId"].clone())
        .collect::<Vec<_>>();
    assert_eq!(
        committed_operations,
        operations
            .iter()
            .map(|operation| json!(operation.as_str()))
            .collect::<Vec<_>>(),
        "an abandoned create committed"
    );
    assert_eq!(
        backend
            .calls()
            .iter()
            .filter(|(name, _)| *name == "create")
            .count(),
        4,
        "the abandoned creates reached the backend before they were dropped"
    );
    tcp.stop().await;
    unix.stop().await;
}
