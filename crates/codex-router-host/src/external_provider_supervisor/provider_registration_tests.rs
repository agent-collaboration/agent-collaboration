//! Provider supervisor creates must consume registration admission, not connection enrollment defaults.
use super::*;
use crate::external_provider_runtime::{
    acp_scripted_fixture::AcpFixtureScript, setup_registrar_fixture::ProviderRegistrarFixture,
};
use collaboration_service::fleet_session_registrar::SessionRegistrationMode;
use serde_json::{Value, json};

#[tokio::test]
async fn provider_registration_supervisor_create_activates_the_returned_session_with_operator_origin()
 {
    let root = tempfile::tempdir_in("/tmp").unwrap();
    let registrar = ProviderRegistrarFixture::new(SessionRegistrationMode::MachineTokens).unwrap();
    let capture = root.path().join("calls.jsonl");
    let fixture = AcpFixtureScript::new()
        .expect_request("initialize", "initialize", json!({"protocolVersion":1}))
        .respond(
            "initialize",
            json!({"protocolVersion":1,"agentCapabilities":{"mcpCapabilities":{"http":true}}}),
        )
        .expect_request("create", "session/new", json!({}))
        .respond(
            "create",
            json!({"sessionId":"actual-supervised-conversation"}),
        )
        .record_requests(&capture)
        .launch();
    let runtime = crate::ExternalProviderRuntime::initialize_with_mcp_http(
        fixture,
        acp_client_runtime::ProviderModelPicker::Standard,
        "router-collaboration",
        "http://127.0.0.1:43210/mcp",
    )
    .await
    .unwrap();
    let endpoint: EndpointRef = serde_json::from_value(
        json!({"serviceId":"019f0000-0000-7000-8000-000000000001","endpointId":"claude-local"}),
    )
    .unwrap();
    let identity = ProviderBindingIdentity {
        endpoint: endpoint.clone(),
        binding_id: "fixture-binding".to_owned().try_into().unwrap(),
        runtime: collaboration_protocol::ProviderRuntimeIdentity {
            provider: collaboration_protocol::ProviderKind::ClaudeCode,
            runtime_name: "fixture".to_owned().try_into().unwrap(),
            runtime_version: None,
        },
        transport: collaboration_protocol::ProviderTransport::StdioAcp,
        generation: serde_json::from_value(
            json!({"serviceEpoch":"019f0000-0000-7000-8000-000000000005","generation":1}),
        )
        .unwrap(),
        capabilities: collaboration_protocol::ProviderCapabilities::try_from(vec![
            collaboration_protocol::ProviderCapability {
                name: collaboration_protocol::ProviderCapabilityName::Create,
                status: collaboration_protocol::ProviderCapabilityStatus::Supported,
                evidence: collaboration_protocol::ProviderCapabilityEvidence::Advertised,
            },
        ])
        .unwrap(),
    };
    let store = ProviderOperationStore::open(&root.path().join("operations.sqlite"))
        .await
        .unwrap();
    let supervisor = ExternalProviderSupervisor::new(
        vec![ExternalProviderBinding { identity, runtime }],
        Arc::new(Mutex::new(store)),
        ProviderSessionLaunchAdmission::MachineTokens {
            port: registrar.provider_launch_port(),
        },
    )
    .unwrap();
    let permission_route = ProviderIdentity::Session(SessionRef {
        endpoint: endpoint.clone(),
        session_id: "permission-route-owner".to_owned().try_into().unwrap(),
    });
    let operation_id = registrar.operation.clone();
    let request = ConversationCreateRequest {
        operation_id: operation_id.clone(),
        endpoint,
        project_id: Some(
            "019f0000-0000-7000-8000-000000000002"
                .to_owned()
                .try_into()
                .unwrap(),
        ),
        generation: None,
        working_directory: root.path().to_str().unwrap().to_owned().try_into().unwrap(),
        created_by: permission_route.clone(),
        approver: permission_route,
        requested_policy: collaboration_protocol::ProviderRequestedPolicy {
            access: collaboration_protocol::RouterAccess::WorkspaceWrite,
        },
        settings: None,
    };

    supervisor.create(request).await.unwrap();
    let live = {
        supervisor
            .inner
            .live_operations
            .lock()
            .unwrap()
            .operations
            .get(&operation_id)
            .unwrap()
            .clone()
    };
    let settlement = loop {
        let changed = live.changed.notified();
        if let Some(result) = live.result.lock().await.clone() {
            break result;
        }
        changed.await;
    };
    supervisor.shutdown().await.unwrap();

    assert!(
        matches!(settlement, Ok(ConversationOperationSettlement::Created { target, .. }) if String::from(target.session_id) == "actual-supervised-conversation")
    );
    let registrations = registrar.authority.registrations.lock().unwrap().clone();
    assert_eq!(registrations.len(), 1);
    assert_eq!(
        registrations[0].session.session_id.as_str(),
        "actual-supervised-conversation"
    );
    assert_eq!(
        registrations[0].started_by,
        fleet_identity_protocol::StartedBy::RouterOperator {}
    );
    assert_eq!(
        registrations[0].isolation.kind,
        fleet_identity_protocol::IsolationKind::Host
    );
    assert_eq!(
        registrations[0].machine_environment,
        fleet_identity_protocol::MachineEnvironment::CloudVm
    );
    let calls = std::fs::read_to_string(capture)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let create = calls
        .iter()
        .find(|call| call["method"] == "session/new")
        .unwrap();
    assert_eq!(
        create["params"]["mcpServers"],
        json!([{"type":"http","name":"router-collaboration","url":"http://127.0.0.1:43210/mcp/session/35353535353535353535353535353535","headers":[]}])
    );
    assert!(
        registrar
            .registrar
            .resolve_binding_capability("35353535353535353535353535353535")
            .await
            .is_ok()
    );
}
