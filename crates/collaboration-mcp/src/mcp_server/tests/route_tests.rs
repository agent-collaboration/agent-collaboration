use super::*;

#[tokio::test]
async fn native_sessions_list_routes_claude_to_provider_tool_without_changing_schema() {
    let server = CollaborationMcpServer::catalog_only();
    let request: collaboration_protocol::NativeSessionListParams = serde_json::from_value(
        serde_json::json!({
            "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},
            "view":"active","scope":{"kind":"any"},"source":"interactive",
            "pageSize":10
        }),
    ).expect("Claude list request");
    let result = server
        .sessions_list(super::super::Parameters(request))
        .await;
    assert_eq!(result.is_error, Some(true));
    let content = serde_json::to_string(&result.structured_content).expect("error content");
    assert!(content.contains("provider_sessions_list"), "{content}");
}

#[test]
fn native_control_errors_keep_service_detail_and_only_uncertain_effects_are_unknown() {
    let unsupported = super::super::structured_result::<serde_json::Value>(
        Err(collaboration_client::ClientError::Rejected {
            code: -32050,
            data: Some(serde_json::json!({
                "kind":"unsupportedCapability",
                "stage":"rename",
                "message":"Codex app-server method `thread/name/set` is missing from its cached schema"
            })),
        }),
        collaboration_protocol::OperationEffect::Unknown,
    );
    let unsupported = unsupported.structured_content.expect("unsupported detail");
    assert_eq!(
        unsupported["message"],
        "Codex app-server method `thread/name/set` is missing from its cached schema"
    );
    assert_eq!(unsupported["effect"], "none");
    assert_eq!(unsupported["data"]["kind"], "unsupportedCapability");

    let lost_after_send = super::super::structured_result::<serde_json::Value>(
        Err(collaboration_client::ClientError::Protocol(
            "connection closed after dispatch",
        )),
        collaboration_protocol::OperationEffect::Unknown,
    );
    let lost_after_send = lost_after_send
        .structured_content
        .expect("transport detail");
    assert_eq!(lost_after_send["effect"], "unknown");
    assert_eq!(
        lost_after_send["message"],
        "collaboration API protocol violation: connection closed after dispatch"
    );
}
