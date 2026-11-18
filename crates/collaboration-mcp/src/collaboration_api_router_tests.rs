//! The collaboration API through the real axum app on both listeners: localhost TCP and a
//! Unix socket. Public calls are stateless; metadata uses one fixture machine session.
use crate::api_test_harness::{
    ServedApi, TEST_PROTOCOL_VERSION, TEST_SERVICE_ID, api_config, test_identity,
};
use collaboration_service::CollaborationApplication;
use reqwest::header::{ACCEPT, CONTENT_TYPE, ORIGIN};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;

fn private_directory() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt as _;
    // Unix socket paths are short; the system temporary directory can exceed the limit.
    let directory = tempfile::tempdir_in("/tmp").expect("private test directory");
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private directory permissions");
    directory
}

async fn automation_store(
    directory: &std::path::Path,
) -> Arc<Mutex<automation_storage::AutomationStore>> {
    let automation =
        automation_storage::AutomationStore::open(&directory.join("automation.sqlite"))
            .await
            .expect("automation store");
    Arc::new(Mutex::new(automation))
}

async fn wake_application(directory: &std::path::Path) -> CollaborationApplication {
    (test_identity().with_automation_store(automation_store(directory).await)).build()
}

/// One application clock, held still, so every read stamps the same time.
fn fixed_clock() -> Arc<router_clock::ManualClock> {
    Arc::new(
        router_clock::ManualClock::starting_at(runtime_primitives::SyncTime::from_unix_millis(
            1_792_432_800_000,
        ))
        .expect("fixed application time"),
    )
}

fn structured(result: &rmcp::model::CallToolResult) -> Value {
    result
        .structured_content
        .clone()
        .expect("structured tool result")
}

fn wake_send_arguments() -> Value {
    json!({
        "operationId": collaboration_protocol::OperationId::generate(),
        "message": {
            "target": {
                "endpoint": {"serviceId": TEST_SERVICE_ID, "endpointId": "codex-local"},
                "sessionId": "wake-target"
            },
            "content": {"kind": "humanUser", "text": "wake proof"},
            "delivery": "auto",
            "generationGuard": null
        },
        "timing": {"kind": "after", "seconds": 600},
        "requestedBy":{"kind":"owner"},"replace":false,"expiry": {"kind": "none"}
    })
}

#[tokio::test]
async fn a_wake_wait_returns_a_cursor_and_the_next_wait_from_it_misses_no_change() {
    // Arrange
    let directory = private_directory();
    let config = api_config(wake_application(directory.path()).await, directory.path());
    let api = ServedApi::unix(&config, &directory.path().join("api.sock"));
    let wake = api.call_tool("wake_send", wake_send_arguments()).await;
    let wakeup_id = structured(&wake)["definition"]["wakeupId"].clone();
    let first = api
        .call_tool(
            "wake_wait_until_first_fire",
            json!({"wakeupId": wakeup_id, "timeoutSeconds": 1}),
        )
        .await;
    let cursor = structured(&first)["cursor"].clone();
    // The wake pauses and resumes between the two waits: only the cursor still sees it.
    for operation in ["wake_pause", "wake_resume"] {
        let changed = api
            .call_tool(
                operation,
                json!({
                    "operationId": collaboration_protocol::OperationId::generate(),
                    "wakeupId": wakeup_id
                }),
            )
            .await;
        assert_eq!(changed.is_error, Some(false), "{operation}: {changed:?}");
    }

    // Act
    let resumed = api
        .call_tool(
            "wake_wait_until_first_fire",
            json!({"wakeupId": wakeup_id, "after": cursor, "timeoutSeconds": 1}),
        )
        .await;
    let fresh = api
        .call_tool(
            "wake_wait_until_first_fire",
            json!({"wakeupId": wakeup_id, "timeoutSeconds": 1}),
        )
        .await;

    // Assert
    assert_eq!(structured(&first)["outcome"]["kind"], "timedOut");
    assert!(cursor.is_string(), "the bounded wait returns its cursor");
    assert_eq!(structured(&resumed)["outcome"]["kind"], "paused");
    assert_ne!(structured(&resumed)["cursor"], cursor);
    assert_eq!(
        structured(&fresh)["outcome"]["kind"],
        "timedOut",
        "a wait without the cursor starts from the current state and never sees the pause"
    );
    api.stop().await;
}

#[tokio::test]
async fn arguments_that_do_not_decode_get_a_structured_validation_failure_on_both_listeners() {
    // Arrange: tools whose arguments rmcp decodes before the handler runs.
    let directory = private_directory();
    let config = api_config(wake_application(directory.path()).await, directory.path());
    let tcp = ServedApi::tcp(&config).await;
    let unix = ServedApi::unix(&config, &directory.path().join("api.sock"));
    let target = json!({
        "endpoint": {"serviceId": TEST_SERVICE_ID, "endpointId": "codex-local"},
        "sessionId": "rename-target"
    });
    let malformed = [
        ("session_rename", json!({"target": target, "name": null})),
        ("session_rename", json!({"target": target})),
        ("approval_decide", json!({"requestId": 7})),
        ("events_observe", json!({})),
    ];

    for api in [&tcp, &unix] {
        for (tool, arguments) in &malformed {
            // Act
            let answer = api.call_tool(tool, arguments.clone()).await;

            // Assert: a typed refusal before dispatch, not text a client cannot read.
            assert_eq!(answer.is_error, Some(true), "{tool}: {answer:?}");
            let failure = answer
                .structured_content
                .unwrap_or_else(|| panic!("{tool} answered without structured content"));
            assert_eq!(failure["kind"], "protocolViolation", "{tool}: {failure}");
            assert_eq!(failure["stage"], "validation", "{tool}: {failure}");
            assert_eq!(failure["effect"], "none", "{tool}: {failure}");
            assert!(
                failure["message"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty()),
                "{tool}: {failure}"
            );
        }
    }
    tcp.stop().await;
    unix.stop().await;
}

#[tokio::test]
async fn a_listener_past_its_request_limit_answers_overloaded() {
    // Arrange
    let directory = private_directory();
    let mut config = api_config(wake_application(directory.path()).await, directory.path());
    config.concurrent_requests = 1;
    let api = ServedApi::tcp(&config).await;
    // Raw stateless calls: an rmcp client's handshake would itself take the only slot.
    let (_, wake) = api
        .post(
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
                "name":"wake_send","arguments":wake_send_arguments()
            }}),
        )
        .await;
    let wakeup_id = wake["result"]["structuredContent"]["definition"]["wakeupId"].clone();
    let url = api.url();
    let holding = tokio::spawn(async move {
        reqwest::Client::new()
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json, text/event-stream")
            .header("mcp-protocol-version", TEST_PROTOCOL_VERSION)
            .json(
                &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
                    "name":"wake_wait_until_first_fire",
                    "arguments":{"wakeupId": wakeup_id, "timeoutSeconds": 2}
                }}),
            )
            .send()
            .await
            .expect("holding call")
            .status()
    });
    api.await_active_calls(1, Duration::from_secs(5)).await;

    // Act
    let (status, shed) = api
        .post(
            &json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{
                "name":"endpoints_list","arguments":{}
            }}),
        )
        .await;

    // Assert
    assert_eq!(status, reqwest::StatusCode::OK);
    assert_eq!(shed["id"], 7);
    assert_eq!(shed["result"]["isError"], true);
    assert_eq!(shed["result"]["structuredContent"]["kind"], "overloaded");
    assert_eq!(shed["result"]["structuredContent"]["effect"], "none");
    assert!(holding.await.expect("holding call join").is_success());
    let (_, admitted) = api
        .post(
            &json!({"jsonrpc":"2.0","id":8,"method":"tools/call","params":{
                "name":"endpoints_list","arguments":{}
            }}),
        )
        .await;
    assert_ne!(
        admitted["result"]["structuredContent"]["kind"], "overloaded",
        "capacity returns once the holding call ends"
    );
    api.stop().await;
}

#[path = "collaboration_api_router_tests/call_lifecycle.rs"]
mod call_lifecycle;

#[tokio::test]
async fn a_foreign_browser_origin_is_refused_before_dispatch() {
    // Arrange: the default listener, with no proxy authority configured.
    let directory = private_directory();
    let config = api_config(wake_application(directory.path()).await, directory.path());
    let api = ServedApi::tcp(&config).await;

    // Act
    let response = reqwest::Client::new()
        .post(api.url())
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json, text/event-stream")
        .header(ORIGIN, "https://invalid.example")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}))
        .send()
        .await
        .expect("origin response");

    // Assert
    assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
    api.stop().await;
}

#[tokio::test]
async fn the_api_serves_on_ipv6_loopback() {
    // Arrange
    let directory = private_directory();
    let config = api_config(wake_application(directory.path()).await, directory.path());
    let api = ServedApi::tcp_at(&config, "[::1]:0").await;

    // Act
    let (status, listed) = api
        .post(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}))
        .await;

    // Assert
    assert!(status.is_success());
    assert!(
        listed["result"]["tools"]
            .as_array()
            .is_some_and(|tools| !tools.is_empty())
    );
    api.stop().await;
}

#[tokio::test]
async fn the_same_call_returns_the_same_structured_result_on_tcp_and_unix() {
    // Arrange: one wake, written through TCP, on a still application clock.
    let directory = private_directory();
    let application = test_identity()
        .with_runtime_clock(fixed_clock())
        .with_automation_store(automation_store(directory.path()).await)
        .build();
    let config = api_config(application, directory.path());
    let tcp = ServedApi::tcp(&config).await;
    let unix = ServedApi::unix(&config, &directory.path().join("api.sock"));
    let wake = tcp.call_tool("wake_send", wake_send_arguments()).await;
    assert_eq!(wake.is_error, Some(false), "{wake:?}");
    let wakeup_id = structured(&wake)["definition"]["wakeupId"].clone();
    let show = json!({"wakeupId": wakeup_id});

    // Act
    let over_tcp = tcp.call_tool("wake_show", show.clone()).await;
    let over_unix = unix.call_tool("wake_show", show).await;
    let tcp_tools = tcp
        .client()
        .await
        .list_all_tools()
        .await
        .expect("TCP tools");
    let unix_tools = unix
        .client()
        .await
        .list_all_tools()
        .await
        .expect("Unix tools");

    // Assert
    assert_eq!(structured(&over_tcp), structured(&over_unix));
    assert_eq!(
        structured(&over_tcp)["definition"]["wakeupId"],
        wakeup_id,
        "both listeners read the wake the TCP call wrote"
    );
    assert_eq!(tcp_tools, unix_tools, "both listeners offer the same tools");
    tcp.stop().await;
    unix.stop().await;
}

#[tokio::test]
async fn schema_required_fields_reach_each_tool_without_missing_field_errors() {
    // Arrange: no stores behind the tools; each call only has to decode.
    let directory = private_directory();
    let config = api_config(test_identity().build(), directory.path());
    let api = ServedApi::tcp(&config).await;
    let (_, listed) = api
        .post(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}))
        .await;
    let tools = listed["result"]["tools"]
        .as_array()
        .expect("advertised tools")
        .clone();

    for (index, tool) in tools.iter().enumerate() {
        let name = tool["name"].as_str().expect("tool name");
        let schema = &tool["inputSchema"];
        let arguments = schema_required_object(schema, &schema["$defs"]);

        // Act
        let (_, response) = api
            .post(&json!({
                "jsonrpc":"2.0","id":index + 2,"method":"tools/call",
                "params":{"name":name,"arguments":arguments}
            }))
            .await;

        // Assert
        let encoded = response.to_string();
        assert!(
            !encoded.contains("missing field") && !encoded.contains("requires "),
            "{name} rejected the schema-required input as incomplete: {encoded}"
        );
        if name == "wake_send" {
            assert!(
                !encoded.contains("explicit nullable generationGuard"),
                "wake_send must accept an omitted optional generationGuard: {encoded}"
            );
        }
    }
    api.stop().await;
}

fn schema_required_object(schema: &Value, definitions: &Value) -> Value {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let name = reference.strip_prefix("#/$defs/").unwrap_or_default();
        return schema_required_object(&definitions[name], definitions);
    }
    if let Some(constant) = schema.get("const") {
        return constant.clone();
    }
    if let Some(variants) = schema.get("enum").and_then(Value::as_array) {
        return variants.first().cloned().unwrap_or(Value::Null);
    }
    for keyword in ["oneOf", "anyOf"] {
        if let Some(variants) = schema.get(keyword).and_then(Value::as_array) {
            return variants
                .first()
                .map(|variant| schema_required_object(variant, definitions))
                .unwrap_or(Value::Null);
        }
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("object") => {
            let required = schema["required"].as_array().cloned().unwrap_or_default();
            let mut object = serde_json::Map::new();
            for field in required.iter().filter_map(Value::as_str) {
                object.insert(
                    field.to_owned(),
                    schema_required_object(&schema["properties"][field], definitions),
                );
            }
            Value::Object(object)
        }
        Some("array") => Value::Array(Vec::new()),
        Some("integer" | "number") => schema.get("minimum").cloned().unwrap_or(json!(1)),
        Some("boolean") => Value::Bool(false),
        Some("string") => {
            if schema.get("format").and_then(Value::as_str) == Some("date-time") {
                json!("2026-09-24T00:00:00Z")
            } else if schema
                .get("pattern")
                .and_then(Value::as_str)
                .is_some_and(|pattern| pattern.contains("7[0-9a-f]{3}"))
            {
                json!("019f0000-0000-7000-8000-000000000001")
            } else if schema
                .get("pattern")
                .and_then(Value::as_str)
                .is_some_and(|pattern| pattern.contains("[a-z]"))
            {
                json!("fixture")
            } else if schema.get("minLength").and_then(Value::as_u64).unwrap_or(0) > 0 {
                json!("x")
            } else {
                json!("")
            }
        }
        _ => Value::Null,
    }
}
