use super::*;
use collaboration_service::collaboration_application::MessageFailureStage;
use serde_json::{Value, json};

#[test]
fn too_old_message_refusal_keeps_its_identity_and_no_effect_in_the_advertised_schema() {
    let rejection = MessageFailure {
        kind: MessageFailureKind::TooOld,
        stage: MessageFailureStage::Discovery,
        message: "Item is older than the 60-day limit".to_owned(),
        existing: None,
    };
    let target = serde_json::from_value(json!({
        "endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000001","endpointId":"codex-local"},"sessionId":"old-message"
    })).unwrap();
    let tool = CollaborationMcpServer::catalog_only()
        .resolved_tools()
        .into_iter()
        .find(|tool| tool.name == "message_send")
        .unwrap();
    let schema = Value::Object((*tool.output_schema.unwrap()).clone());
    let validator = jsonschema::validator_for(&schema).unwrap();

    let result = message_failure_result(&rejection, "target", target);

    assert_eq!(result.is_error, Some(true));
    let structured = result.structured_content.unwrap();
    assert_eq!(structured["serviceKind"], "tooOld");
    assert_eq!(structured["data"]["kind"], "tooOld");
    assert_eq!(structured["code"], -32602);
    assert_eq!(structured["effect"], "none");
    assert!(structured.get("target").is_none_or(Value::is_null));
    validator.validate(&structured).unwrap();
}
