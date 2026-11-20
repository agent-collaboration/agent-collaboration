//! Discovery failure before managed creation over the real stateless listener.
use crate::api_test_harness::{
    ServedApi, TEST_SERVICE_EPOCH as SERVICE_EPOCH, TEST_SERVICE_ID as SERVICE_ID, api_config,
};
use collaboration_service::ApplicationFixture;
use serde_json::{Value, json};

async fn mcp_call(listener: &ServedApi, id: u64, name: &str, arguments: Value) -> Value {
    listener.post(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}})).await.1
}

#[tokio::test]
async fn stateless_http_create_reports_discovery_failure_without_creation_uncertainty() {
    // The listener is real, but its Router publishes no endpoints and its service directory
    // has no manifest. The create tool discovers carriers from the Router's own endpoint
    // directory, never the manifest, so the failure is that discovery, before ACP setup or
    // dispatch.
    let directory = tempfile::tempdir_in("/tmp").expect("private service directory");
    let listener = ServedApi::tcp(&api_config(
        (ApplicationFixture::new(SERVICE_ID, SERVICE_EPOCH).expect("service identity")).build(),
        directory.path(),
    ))
    .await;

    let endpoint = json!({"serviceId":SERVICE_ID,"endpointId":"codex-local"});
    let local_identity = json!({"endpoint":endpoint,"sessionId":"mcp-caller"});
    let foreign_identity = json!({
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000099","endpointId":"codex-local"},
        "sessionId":"foreign-caller"
    });
    for (id, created_by, approver) in [
        (2, Value::Null, local_identity.clone()),
        (3, foreign_identity, local_identity.clone()),
    ] {
        let missing_creator = created_by.is_null();
        let response = mcp_call(
            &listener,
            id,
            "conversation_create",
            json!({
                "operationId":collaboration_protocol::OperationId::generate(),
                "endpoint":endpoint,"workingDirectory":"/tmp", "fork":null,
                "model":"gpt-5.6-sol", "effort":"low", "access":"workspace-write",
                "createdBy":created_by, "approver":approver, "rootMessageId":null
            }),
        )
        .await;
        if missing_creator {
            assert_eq!(response["result"]["isError"], true);
            assert!(
                response["result"]["content"][0]["text"]
                    .as_str()
                    .is_some_and(|message| message.contains("expected struct SessionRef")),
                "{response}"
            );
            let refusal = &response["result"]["structuredContent"];
            assert_eq!(refusal["kind"], "protocolViolation", "{response}");
            assert_eq!(refusal["stage"], "validation", "{response}");
            assert_eq!(refusal["effect"], "none", "{response}");
            continue;
        }
        let failure = &response["result"]["structuredContent"];
        assert_eq!(response["result"]["isError"], true);
        assert_eq!(failure["target"], Value::Null);
        assert_eq!(failure["effect"], "none", "{response}");
        assert_eq!(failure["stage"], "validation");
    }

    let response = mcp_call(
        &listener,
        4,
        "conversation_create",
        json!({
            "operationId":collaboration_protocol::OperationId::generate(),
            "endpoint":endpoint,"workingDirectory":"/tmp", "fork":null,
            "model":"gpt-5.6-sol", "effort":"low", "access":"workspace-write",
            "createdBy":local_identity, "approver":local_identity, "rootMessageId":null
        }),
    )
    .await;
    let failure = &response["result"]["structuredContent"];
    assert_eq!(failure["effect"], "none", "{response}");
    assert_eq!(failure["kind"], "protocolViolation", "{response}");
    assert!(
        failure["message"]
            .as_str()
            .is_some_and(|message| message.contains("conversation endpoint not found")),
        "{response}"
    );

    listener.stop().await;
}
