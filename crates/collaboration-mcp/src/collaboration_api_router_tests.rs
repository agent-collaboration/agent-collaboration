//! The collaboration API through the real axum app on both listeners: localhost TCP and a
//! Unix socket. Public calls are stateless; metadata uses one fixture machine session.
use crate::api_test_harness::{ServedApi, TEST_SERVICE_ID, api_config, test_identity};
use reqwest::header::{ACCEPT, CONTENT_TYPE, ORIGIN};
use serde_json::{Value, json};

fn private_directory() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt as _;
    // Unix socket paths are short; the system temporary directory can exceed the limit.
    let directory = tempfile::tempdir_in("/tmp").expect("private test directory");
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private directory permissions");
    directory
}

fn structured(result: &rmcp::model::CallToolResult) -> Value {
    result
        .structured_content
        .clone()
        .expect("structured tool result")
}

#[tokio::test]
async fn the_same_call_returns_the_same_structured_result_on_tcp_and_unix() {
    // Arrange
    let directory = private_directory();
    let config = api_config(test_identity().build(), directory.path());
    let tcp = ServedApi::tcp(&config).await;
    let unix = ServedApi::unix(&config, &directory.path().join("api.sock"));

    // Act
    let over_tcp = tcp.call_tool("endpoints_list", json!({})).await;
    let over_unix = unix.call_tool("endpoints_list", json!({})).await;
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
    assert_ne!(over_tcp.is_error, Some(true), "{over_tcp:?}");
    assert_eq!(structured(&over_tcp), structured(&over_unix));
    assert_eq!(tcp_tools, unix_tools, "both listeners offer the same tools");
    tcp.stop().await;
    unix.stop().await;
}

#[tokio::test]
async fn arguments_that_do_not_decode_get_a_structured_validation_failure_on_both_listeners() {
    // Arrange: tools whose arguments rmcp decodes before the handler runs.
    let directory = private_directory();
    let config = api_config(test_identity().build(), directory.path());
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
async fn a_foreign_browser_origin_is_refused_before_dispatch() {
    // Arrange
    let directory = private_directory();
    let config = api_config(test_identity().build(), directory.path());
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
    let config = api_config(test_identity().build(), directory.path());
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
async fn schema_required_fields_reach_each_tool_without_missing_field_errors() {
    // Arrange
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

#[path = "collaboration_api_router_tests/call_lifecycle.rs"]
mod call_lifecycle;
