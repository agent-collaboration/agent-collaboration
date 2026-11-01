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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RestoreRegistration {
    Unknown,
    Active,
    Abandoned,
    Retired,
}

#[tokio::test]
async fn provider_registration_load_and_resume_require_and_reuse_the_active_binding() {
    for resume in [false, true] {
        for mode in [
            SessionRegistrationMode::AccessEnrollment,
            SessionRegistrationMode::MachineTokens,
        ] {
            for state in [
                RestoreRegistration::Unknown,
                RestoreRegistration::Active,
                RestoreRegistration::Abandoned,
                RestoreRegistration::Retired,
            ] {
                if mode == SessionRegistrationMode::AccessEnrollment
                    && state != RestoreRegistration::Unknown
                {
                    continue;
                }
                let root = tempfile::tempdir_in("/tmp").unwrap();
                let registrar = ProviderRegistrarFixture::new(mode).unwrap();
                let endpoint: EndpointRef = serde_json::from_value(json!({"serviceId":"019f0000-0000-7000-8000-000000000001","endpointId":"claude-local"})).unwrap();
                let target = SessionRef {
                    endpoint: endpoint.clone(),
                    session_id: "restored-conversation".to_owned().try_into().unwrap(),
                };
                if mode == SessionRegistrationMode::MachineTokens
                    && state != RestoreRegistration::Unknown
                {
                    let _prepared = registrar
                        .create_setup(
                            root.path(),
                            &acp_client_runtime::RequestedProviderSettings::default(),
                        )
                        .await
                        .unwrap();
                    let board_target = message_board::SessionRef {
                        endpoint: message_board::SessionEndpointRef {
                            service_id: String::from(endpoint.service_id.clone())
                                .try_into()
                                .unwrap(),
                            endpoint_id: String::from(endpoint.endpoint_id.clone())
                                .try_into()
                                .unwrap(),
                        },
                        session_id: "restored-conversation".to_owned().try_into().unwrap(),
                    };
                    registrar
                        .registrar
                        .record_provider_created(&registrar.operation, board_target.clone())
                        .await
                        .unwrap();
                    if state == RestoreRegistration::Abandoned {
                        registrar.registrar.abandon_created_setup(&registrar.operation, collaboration_protocol::SessionSetupAbandonmentCause::SetupInterrupted {}).await.unwrap();
                    } else {
                        registrar
                            .registrar
                            .complete_created_setup(&registrar.operation)
                            .await
                            .unwrap();
                        if state == RestoreRegistration::Retired {
                            registrar
                                .registrar
                                .end(
                                    &board_target,
                                    fleet_identity_protocol::SessionEndCause::Archived,
                                )
                                .await
                                .unwrap();
                        }
                    }
                }
                let before = registrar.repository.records.lock().unwrap().clone();
                let registration_count = registrar.authority.registrations.lock().unwrap().len();
                let capture = root.path().join("calls.jsonl");
                let diagnostic = root.path().join("diagnostic.txt");
                let finish_path = root.path().join("finish.sock");
                let finish = tokio::net::UnixListener::bind(&finish_path).unwrap();
                let disallowed = mode == SessionRegistrationMode::MachineTokens
                    && state != RestoreRegistration::Active;
                let method = if resume {
                    "session/resume"
                } else {
                    "session/load"
                };
                let script = AcpFixtureScript::new()
                    .expect_request("initialize", "initialize", json!({"protocolVersion":1}))
                    .respond("initialize", json!({"protocolVersion":1,"agentCapabilities":{"mcpCapabilities":{"http":true},"loadSession":true,"sessionCapabilities":{"resume":{}}}}));
                let script = if disallowed {
                    script.wait_for_socket_signal(&finish_path)
                } else {
                    script
                        .expect_request(
                            "restore",
                            method,
                            json!({"sessionId":"restored-conversation"}),
                        )
                        .respond("restore", json!({}))
                };
                let fixture = script
                    .record_requests(&capture)
                    .record_diagnostics(diagnostic.clone())
                    .launch();
                let runtime = crate::ExternalProviderRuntime::initialize_with_mcp_http(
                    fixture,
                    acp_client_runtime::ProviderModelPicker::Standard,
                    "router-collaboration",
                    "http://127.0.0.1:43210/mcp",
                )
                .await
                .unwrap();
                let peer_retired = runtime.retirement();
                let capability = if resume {
                    collaboration_protocol::ProviderCapabilityName::Resume
                } else {
                    collaboration_protocol::ProviderCapabilityName::Load
                };
                let identity = ProviderBindingIdentity {
                    endpoint: endpoint.clone(), binding_id: "fixture-binding".to_owned().try_into().unwrap(),
                    runtime: collaboration_protocol::ProviderRuntimeIdentity { provider: collaboration_protocol::ProviderKind::ClaudeCode, runtime_name: "fixture".to_owned().try_into().unwrap(), runtime_version: None },
                    transport: collaboration_protocol::ProviderTransport::StdioAcp,
                    generation: serde_json::from_value(json!({"serviceEpoch":"019f0000-0000-7000-8000-000000000005","generation":1})).unwrap(),
                    capabilities: collaboration_protocol::ProviderCapabilities::try_from(vec![collaboration_protocol::ProviderCapability { name: capability, status: collaboration_protocol::ProviderCapabilityStatus::Supported, evidence: collaboration_protocol::ProviderCapabilityEvidence::Advertised }]).unwrap(),
                };
                let store = ProviderOperationStore::open(&root.path().join("operations.sqlite"))
                    .await
                    .unwrap();
                let admission = if mode == SessionRegistrationMode::MachineTokens {
                    ProviderSessionLaunchAdmission::MachineTokens {
                        port: registrar.provider_launch_port(),
                    }
                } else {
                    ProviderSessionLaunchAdmission::AccessEnrollment
                };
                let supervisor = ExternalProviderSupervisor::new(
                    vec![ExternalProviderBinding { identity, runtime }],
                    Arc::new(Mutex::new(store)),
                    admission,
                )
                .unwrap();
                let operation_id: OperationId = "019f0000-0000-7000-8000-000000000021"
                    .to_owned()
                    .try_into()
                    .unwrap();
                let actor = ProviderIdentity::Session(SessionRef {
                    endpoint,
                    session_id: "permission-route-owner".to_owned().try_into().unwrap(),
                });
                if resume {
                    supervisor
                        .resume(ConversationResumeRequest {
                            operation_id: operation_id.clone(),
                            target,
                            generation: None,
                            working_directory: root
                                .path()
                                .to_str()
                                .unwrap()
                                .to_owned()
                                .try_into()
                                .unwrap(),
                            requested_by: actor.clone(),
                            approver: actor,
                            requested_policy: collaboration_protocol::ProviderRequestedPolicy {
                                access: collaboration_protocol::RouterAccess::WorkspaceWrite,
                            },
                        })
                        .await
                        .unwrap();
                } else {
                    supervisor
                        .load(ConversationLoadRequest {
                            operation_id: operation_id.clone(),
                            target,
                            generation: None,
                            working_directory: root
                                .path()
                                .to_str()
                                .unwrap()
                                .to_owned()
                                .try_into()
                                .unwrap(),
                            requested_by: actor.clone(),
                            approver: actor,
                            requested_policy: collaboration_protocol::ProviderRequestedPolicy {
                                access: collaboration_protocol::RouterAccess::WorkspaceWrite,
                            },
                        })
                        .await
                        .unwrap();
                }
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
                if disallowed {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    tokio::select! {
                        _ = peer_retired.cancelled() => {},
                        accepted = finish.accept() => {
                            let (mut acknowledgement, _) = accepted.unwrap();
                            let _sent = acknowledgement.write_all(b"S").await;
                            let mut completed = Vec::new();
                            acknowledgement.read_to_end(&mut completed).await.unwrap();
                        }
                    }
                }
                let stopped = supervisor.shutdown().await;
                let wire_diagnostic = if diagnostic.exists() {
                    std::fs::read_to_string(&diagnostic).unwrap()
                } else {
                    String::new()
                };
                assert!(
                    wire_diagnostic.is_empty(),
                    "{method}/{mode:?}/{state:?}: {wire_diagnostic}"
                );
                stopped.unwrap();
                let calls = std::fs::read_to_string(capture)
                    .unwrap()
                    .lines()
                    .map(|line| serde_json::from_str::<Value>(line).unwrap())
                    .collect::<Vec<_>>();
                let restore = calls.iter().find(|call| call["method"] == method);

                if mode == SessionRegistrationMode::MachineTokens
                    && state != RestoreRegistration::Active
                {
                    assert!(
                        settlement.is_err(),
                        "{method}/{state:?} must refuse before restoring"
                    );
                    assert!(restore.is_none());
                } else {
                    assert!(
                        settlement.is_ok(),
                        "{method}/{mode:?}/{state:?}: {settlement:?}"
                    );
                    let expected = if mode == SessionRegistrationMode::MachineTokens {
                        "http://127.0.0.1:43210/mcp/session/35353535353535353535353535353535"
                    } else {
                        "http://127.0.0.1:43210/mcp"
                    };
                    assert_eq!(
                        restore.unwrap()["params"]["mcpServers"],
                        json!([{"type":"http","name":"router-collaboration","url":expected,"headers":[]}])
                    );
                }
                assert_eq!(*registrar.repository.records.lock().unwrap(), before);
                assert_eq!(
                    registrar.authority.registrations.lock().unwrap().len(),
                    registration_count
                );
            }
        }
    }
}
