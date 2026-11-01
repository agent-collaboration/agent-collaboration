//! Admission refusals must precede the ACP create and keep known-no-create state.
use super::*;
use crate::external_provider_runtime::{
    acp_scripted_fixture::AcpFixtureScript, setup_registrar_fixture::ProviderRegistrarFixture,
};
use collaboration_service::fleet_session_registrar::SessionRegistrationMode;
use serde_json::{Value, json};

// Wrong behaviors: sending session/new without HTTP binding support or falling
// back from machine mode to the legacy connection. Oracle: PD prepared-launch
// gate and explicit enrollment-mode boundary. No pre-fix red observed for this
// additional refusal test; the earlier real supervisor-create red is retained.
#[tokio::test]
async fn provider_registration_refuses_missing_http_and_a_mode_downgrade_before_create() {
    for (registration_mode, supports_http, expected_kind) in [
        (
            SessionRegistrationMode::MachineTokens,
            false,
            ConversationOperationFailureKind::UnsupportedCapability,
        ),
        (
            SessionRegistrationMode::AccessEnrollment,
            true,
            ConversationOperationFailureKind::Unavailable,
        ),
    ] {
        let root = tempfile::tempdir_in("/tmp").unwrap();
        let registrar = ProviderRegistrarFixture::new(registration_mode).unwrap();
        let capture = root.path().join("calls.jsonl");
        let fixture = AcpFixtureScript::new()
        .expect_request("initialize", "initialize", json!({"protocolVersion":1}))
        .respond(
            "initialize",
            json!({"protocolVersion":1,"agentCapabilities":{"mcpCapabilities":{"http":supports_http}}}),
        )
        .expect_request("create", "session/new", json!({}))
        .respond(
            "create",
            json!({"sessionId":"actual-supervised-conversation"}),
        )
        .record_requests(&capture)
        .launch();
        let runtime = crate::ExternalProviderRuntime::initialize(fixture)
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

        // Assert the admission cause, not just an arbitrary transport failure.
        assert!(matches!(settlement, Err(failure) if failure.kind == expected_kind));
        assert!(registrar.authority.registrations.lock().unwrap().is_empty());
        let calls = std::fs::read_to_string(capture).unwrap();
        assert!(!calls.lines().any(|line| {
            serde_json::from_str::<Value>(line).unwrap()["method"] == "session/new"
        }));
        if registration_mode == SessionRegistrationMode::MachineTokens {
            use codex_router_fleet_state::fleet_session_registration::{
                AbandonedSessionCreation, FleetSessionBindingLifecycle,
            };
            let records = registrar.repository.records.lock().unwrap();
            assert_eq!(records.len(), 1);
            let record = &records[0];
            assert!(matches!(
                record.lifecycle(),
                FleetSessionBindingLifecycle::Abandoned {
                    creation: AbandonedSessionCreation::NotCreated {},
                }
            ));
        }
    }
}
