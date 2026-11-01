//! Automatic delivery loads must use the same retained registration as explicit restores.
use super::*;
use crate::{
    ExternalProviderBinding, ProviderSessionLaunchAdmission,
    external_provider_runtime::{
        acp_scripted_fixture::AcpFixtureScript, setup_registrar_fixture::ProviderRegistrarFixture,
    },
};
use collaboration_service::fleet_session_registrar::SessionRegistrationMode;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct NotLiveElsewhere;
impl LiveSessionOwnershipCheck for NotLiveElsewhere {
    fn check<'a>(
        &'a self,
        _: &'a SessionRef,
    ) -> collaboration_contracts::DeliveryFuture<'a, LiveSessionOwnership> {
        Box::pin(async { Ok(LiveSessionOwnership::NotLive) })
    }
}

#[tokio::test]
async fn provider_registration_delivery_load_requires_and_reuses_the_active_binding() {
    for (mode, active) in [
        (SessionRegistrationMode::AccessEnrollment, false),
        (SessionRegistrationMode::MachineTokens, false),
        (SessionRegistrationMode::MachineTokens, true),
    ] {
        let root = tempfile::tempdir_in("/tmp").unwrap();
        let registrar = ProviderRegistrarFixture::new(mode).unwrap();
        let target: SessionRef = serde_json::from_value(json!({
            "endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000001","endpointId":"claude-local"},
            "sessionId":"delivery-restored-conversation"
        }))
        .unwrap();
        if active {
            let _prepared = registrar
                .create_setup(root.path(), &Default::default())
                .await
                .unwrap();
            let board_target = message_board::SessionRef {
                endpoint: message_board::SessionEndpointRef {
                    service_id: String::from(target.endpoint.service_id.clone())
                        .try_into()
                        .unwrap(),
                    endpoint_id: String::from(target.endpoint.endpoint_id.clone())
                        .try_into()
                        .unwrap(),
                },
                session_id: target.session_id.as_str().to_owned().try_into().unwrap(),
            };
            registrar
                .registrar
                .record_provider_created(&registrar.operation, board_target)
                .await
                .unwrap();
            registrar
                .registrar
                .complete_created_setup(&registrar.operation)
                .await
                .unwrap();
        }
        let before = registrar.repository.records.lock().unwrap().clone();
        let registration_count = registrar.authority.registrations.lock().unwrap().len();
        let disallowed = mode == SessionRegistrationMode::MachineTokens && !active;
        let expected_url = if active {
            "http://127.0.0.1:43210/mcp/session/35353535353535353535353535353535"
        } else {
            "http://127.0.0.1:43210/mcp"
        };
        let capture = root.path().join("calls.jsonl");
        let diagnostic = root.path().join("diagnostic.txt");
        let finish_path = root.path().join("finish.sock");
        let finish = tokio::net::UnixListener::bind(&finish_path).unwrap();
        let script = AcpFixtureScript::new()
            .expect_request("initialize", "initialize", json!({"protocolVersion":1}))
            .respond("initialize", json!({"protocolVersion":1,"agentCapabilities":{"mcpCapabilities":{"http":true},"loadSession":true}}));
        let script = if disallowed {
            script.wait_for_socket_signal(&finish_path)
        } else {
            script
                .expect_request("load", "session/load", json!({
                    "sessionId":"delivery-restored-conversation",
                    "mcpServers":[{"type":"http","name":"router-collaboration","url":expected_url,"headers":[]}]
                }))
                .respond("load", json!({}))
        };
        let runtime = ExternalProviderRuntime::initialize_with_mcp_http(
            script
                .record_requests(&capture)
                .record_diagnostics(diagnostic.clone())
                .launch(),
            acp_client_runtime::ProviderModelPicker::Standard,
            "router-collaboration",
            "http://127.0.0.1:43210/mcp",
        )
        .await
        .unwrap();
        let retired = runtime.retirement();
        let identity = serde_json::from_value(json!({
            "endpoint":target.endpoint,
            "bindingId":"fixture-binding",
            "runtime":{"provider":"claudeCode","runtimeName":"fixture","runtimeVersion":null},
            "transport":"stdioAcp",
            "generation":{"serviceEpoch":"019f0000-0000-7000-8000-000000000005","generation":1},
            "capabilities":[{"name":"load","status":"supported","evidence":"advertised"}]
        }))
        .unwrap();
        let store = Arc::new(Mutex::new(
            ProviderOperationStore::open(&root.path().join("operations.sqlite"))
                .await
                .unwrap(),
        ));
        store
            .lock()
            .await
            .record_session(&collaboration_runtime::ProviderSessionRecord {
                target: target.clone(),
                working_directory: root.path().to_str().unwrap().to_owned().try_into().unwrap(),
                requested_policy: collaboration_protocol::ProviderRequestedPolicy {
                    access: collaboration_protocol::RouterAccess::WorkspaceWrite,
                },
                created_by: target.clone().into(),
                approver: target.clone().into(),
                updated_at_ms: 1,
            })
            .await
            .unwrap();
        let admission = match mode {
            SessionRegistrationMode::AccessEnrollment => {
                ProviderSessionLaunchAdmission::AccessEnrollment
            }
            SessionRegistrationMode::MachineTokens => {
                ProviderSessionLaunchAdmission::MachineTokens {
                    port: registrar.provider_launch_port(),
                }
            }
        };
        let supervisor = ExternalProviderSupervisor::new(
            vec![ExternalProviderBinding { identity, runtime }],
            store.clone(),
            admission,
        )
        .unwrap();

        let outcome = ensure_provider_session_loaded(
            &supervisor,
            &store,
            &NotLiveElsewhere,
            &target,
            LoadPolicy::MayLoad,
        )
        .await;

        if disallowed {
            tokio::select! {
                _ = retired.cancelled() => {},
                accepted = finish.accept() => {
                    let (mut acknowledgement, _) = accepted.unwrap();
                    let _sent = acknowledgement.write_all(b"S").await;
                    let mut completed = Vec::new();
                    acknowledgement.read_to_end(&mut completed).await.unwrap();
                }
            }
        }
        let stopped = supervisor.shutdown().await;
        let wire_diagnostic = std::fs::read_to_string(&diagnostic).unwrap_or_default();
        assert!(
            wire_diagnostic.is_empty(),
            "{mode:?}/{active}: {wire_diagnostic}"
        );
        stopped.unwrap();
        let calls = std::fs::read_to_string(&capture)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        if disallowed {
            assert!(matches!(
                outcome,
                ProviderSessionLoadOutcome::Unavailable { .. }
            ));
            assert!(calls.iter().all(|call| call["method"] == "initialize"));
        } else {
            assert_eq!(outcome, ProviderSessionLoadOutcome::Ready);
            assert_eq!(
                calls.last().unwrap()["params"]["mcpServers"][0]["url"],
                expected_url
            );
        }
        assert_eq!(*registrar.repository.records.lock().unwrap(), before);
        assert_eq!(
            registrar.authority.registrations.lock().unwrap().len(),
            registration_count
        );
    }
}
