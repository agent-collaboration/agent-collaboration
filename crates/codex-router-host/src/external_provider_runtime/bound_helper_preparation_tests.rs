//! The real bound MCP tool must refuse before carrier creation when its Host prepare port is absent.
use super::setup_registrar_fixture::ProviderRegistrarFixture;
use acp_client_runtime::RequestedProviderSettings;
use collaboration_mcp::{
    CollaborationApiConfig, CollaborationApiListener, collaboration_api_router,
    serve_collaboration_api,
};
use collaboration_service::fleet_session_registrar::SessionRegistrationMode;
use serde_json::{Value, json};

#[tokio::test]
async fn bound_helper_missing_prepare_port_refuses_before_carrier_creation() {
    let root = tempfile::tempdir_in("/tmp").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let fixture = ProviderRegistrarFixture::at_origin(
        SessionRegistrationMode::MachineTokens,
        &format!("http://{address}"),
    )
    .unwrap();
    let _caller_create = fixture
        .create_setup(root.path(), &RequestedProviderSettings::default())
        .await
        .unwrap();
    let caller = json!({"endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000001","endpointId":"claude-local"},"sessionId":"bound-starter"});
    fixture
        .registrar
        .record_provider_created(
            &fixture.operation,
            serde_json::from_value(caller.clone()).unwrap(),
        )
        .await
        .unwrap();
    fixture
        .registrar
        .complete_created_setup(&fixture.operation)
        .await
        .unwrap();
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

    let response = reqwest::Client::new().post(format!("http://{address}/mcp/session/35353535353535353535353535353535"))
        .header("Content-Type", "application/json").header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2025-11-25")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"conversation_create","arguments":{
            "operationId":"019f0000-0000-7000-8000-000000000021",
            "endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000001","endpointId":"codex-local"},
            "workingDirectory":root.path(),"projectId":"019f0000-0000-7000-8000-000000000002",
            "access":"workspace-write","createdBy":caller,"model":"fixture-model","effort":"low"
        }}})).send().await.unwrap();
    let body = response.text().await.unwrap();
    let encoded = body
        .lines()
        .find_map(|line| {
            line.strip_prefix("data: ")
                .filter(|value| !value.is_empty())
        })
        .unwrap_or(body.as_str());
    let answer: Value = serde_json::from_str(encoded).unwrap();
    shutdown.cancel();
    server.await.unwrap().unwrap();

    assert_eq!(
        answer["result"]["structuredContent"]["kind"], "startPreparationUnavailable",
        "{answer}"
    );
    assert_eq!(answer["result"]["structuredContent"]["effect"], "none");
    assert_eq!(
        fixture.repository.records.lock().unwrap().len(),
        1,
        "only the active caller may exist"
    );
    assert_eq!(fixture.authority.registrations.lock().unwrap().len(), 1);
}
