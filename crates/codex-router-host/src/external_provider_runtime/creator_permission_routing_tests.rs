//! Creator permission metadata admits create-only calls; prompt authors still require binding.
use super::setup_registrar_fixture::ProviderRegistrarFixture;
use collaboration_mcp::{
    CollaborationApiConfig, CollaborationApiListener, collaboration_api_router,
    serve_collaboration_api,
};
use collaboration_service::fleet_session_registrar::SessionRegistrationMode;
use serde_json::{Value, json};

#[tokio::test]
async fn creator_permission_routing_unbound_create_keeps_prompt_and_service_guards() {
    let root = tempfile::tempdir_in("/tmp").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let fixture = ProviderRegistrarFixture::at_origin(
        SessionRegistrationMode::MachineTokens,
        &format!("http://{address}"),
    )
    .unwrap();
    // An empty directory exposes discovery as the first effect after admission.
    let application = collaboration_service::ApplicationFixture::new(
        "019f0000-0000-7000-8000-000000000001",
        "019f0000-0000-7000-8000-000000000005",
    )
    .unwrap()
    .build();
    let (_relation, receiver) =
        tokio::sync::watch::channel(collaboration_protocol::RouterExecutableRelation::Match);
    let shutdown = tokio_util::sync::CancellationToken::new();
    let config = CollaborationApiConfig {
        bound_conversation_preparation: None,
        application,
        session_registrar: Some(fixture.registrar.clone()),
        service_directory: root.path().to_owned(),
        native_definitions: None,
        router_executable_relation: receiver,
        concurrent_requests: collaboration_mcp::DEFAULT_CONCURRENT_REQUESTS,
        shutdown: shutdown.clone(),
    };
    let router = collaboration_api_router(
        &config,
        CollaborationApiListener::LoopbackTcp {
            address,
            proxy_authority: None,
        },
    );
    let server = tokio::spawn(serve_collaboration_api(listener, router, shutdown.clone()));
    let creator = json!({
        "endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000001","endpointId":"claude-local"},
        "sessionId":"permission-route-owner"
    });
    let create = json!({
        "operationId":"019f0000-0000-7000-8000-000000000021",
        "endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000001","endpointId":"claude-local"},
        "workingDirectory":root.path(),"projectId":"019f0000-0000-7000-8000-000000000002",
        "access":"workspace-write","createdBy":creator
    });
    let mut other_service = create.clone();
    other_service["createdBy"]["endpoint"]["serviceId"] =
        json!("019f0000-0000-7000-8000-000000000099");
    let mut typed_creator = create.clone();
    typed_creator["createdBy"] = json!({"kind":"session","session":creator});
    let cases = [
        ("conversation_create", create.clone(), "protocolViolation"),
        ("conversation_create", typed_creator, "protocolViolation"),
        ("conversation_create", other_service, "invalidRequest"),
        (
            "conversation_create_and_prompt",
            json!({"create":create,"message":{"kind":"agent","sender":creator,"text":"must-not-be-sent"}}),
            "sessionBindingRequired",
        ),
    ];
    let mut answers = Vec::new();
    for (tool, arguments, expected_kind) in cases {
        let response = reqwest::Client::new().post(format!("http://{address}/mcp"))
            .header("Content-Type", "application/json").header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", "2025-11-25")
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":arguments}})).send().await.unwrap();
        let body = response.text().await.unwrap();
        let encoded = body
            .lines()
            .find_map(|line| {
                line.strip_prefix("data: ")
                    .filter(|value| !value.is_empty())
            })
            .unwrap_or(body.as_str());
        answers.push((
            expected_kind,
            serde_json::from_str::<Value>(encoded).unwrap(),
        ));
    }
    shutdown.cancel();
    server.await.unwrap().unwrap();

    for (expected_kind, answer) in answers {
        assert_eq!(
            answer["result"]["structuredContent"]["kind"], expected_kind,
            "{answer}"
        );
        assert_eq!(answer["result"]["structuredContent"]["effect"], "none");
        if expected_kind == "protocolViolation" {
            assert_eq!(
                answer["result"]["structuredContent"]["message"],
                "collaboration API protocol violation: conversation endpoint not found"
            );
        }
    }
    assert!(fixture.repository.records.lock().unwrap().is_empty());
    assert!(fixture.authority.registrations.lock().unwrap().is_empty());
}
