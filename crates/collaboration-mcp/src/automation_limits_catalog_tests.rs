//! Publication proof uses the real axum/rmcp catalog, with no model or central calls.
use crate::api_test_harness::{ServedApi, api_config, test_identity};
use collaboration_protocol::limits;
use serde_json::json;

#[tokio::test]
async fn central_automation_limits_are_published_in_the_served_catalog() {
    let directory = tempfile::tempdir().unwrap();
    let application = test_identity().build();
    let api = ServedApi::tcp(&api_config(application, directory.path())).await;
    let client = api.client().await;
    let tools = client.list_all_tools().await.unwrap();
    drop(client);
    api.stop().await;

    for name in ["wake_send", "schedule_create", "instruction_create"] {
        let tool = tools.iter().find(|tool| tool.name == name).unwrap();
        let description = tool.description.as_deref().unwrap();
        assert!(
            description.contains(&limits::description()),
            "{name} omitted the product limits"
        );
    }
    let wake = tools.iter().find(|tool| tool.name == "wake_send").unwrap();
    let schema = serde_json::to_value(&wake.input_schema).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let mut request = json!({
        "operationId":"019f0000-0000-7000-8000-000000000001",
        "requestedBy":{"kind":"owner"}, "replace":false,
        "message":{"target":{"endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000002","endpointId":"claude-local"},"sessionId":"fixture"},
            "content":{"kind":"humanUser","text":"fixture"},"delivery":"auto"},
        "timing":{"kind":"interval","seconds":600},"expiry":{"kind":"after","seconds":3600}
    });
    assert!(validator.is_valid(&request));
    request["timing"]["firstDue"] = json!("2026-10-31T10:15:00Z");
    assert!(validator.is_valid(&request));
    request["timing"]["firstDue"] = json!("2026-10-31T10:12:00Z");
    assert!(!validator.is_valid(&request));
    request["timing"]["firstDue"] = json!("2026-10-31T10:15:00Z");
    request["timing"]["seconds"] = json!(300);
    assert!(
        !validator.is_valid(&request),
        "the served wake schema must reject a five-minute repeat"
    );
    request["timing"] = json!({"kind":"after","seconds":600});
    assert!(!validator.is_valid(&request));
}
