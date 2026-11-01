//! Real Host route composition and native wire; memory store/central and native answers stand in.
//! Oracle: Lead01a19876/01a19879, PD bindings wherever StartThread/ForkThread is built.
use super::{
    FIRST, SECOND,
    fixture::{PROJECT, RuntimeFixture},
    native_fixture::NativeFixture,
};
use collaboration_contracts::{
    DeliveryContractError, DeliveryFuture, PreparationEvidenceSink, PreparedTarget,
    RunEvidenceDisposition, RunEvidenceSink, SchedulePreparationOutcome,
    SchedulePreparationRequest, ScheduledRunExecution,
};
use collaboration_protocol::{CodexGeneration, DestinationPreparation, EndpointRef, SessionRef};
use collaboration_service::fleet_session_registrar::SessionRegistrationMode;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Evidence(Mutex<Vec<agent_automation::RouteEffectEvidence<SessionRef, CodexGeneration>>>);
impl PreparationEvidenceSink for Evidence {
    fn record_intent(
        &self,
        evidence: agent_automation::RouteEffectEvidence<SessionRef, CodexGeneration>,
    ) -> DeliveryFuture<'_, bool> {
        self.0.lock().unwrap().push(evidence);
        Box::pin(async { Ok(true) })
    }
    fn record_prepared<'a>(
        &'a self,
        _: &'a PreparedTarget,
    ) -> DeliveryFuture<
        'a,
        automation_storage::ScheduleInspection<SessionRef, EndpointRef, message_board::ProjectId>,
    > {
        Box::pin(async { Err(DeliveryContractError::EvidencePersistence) })
    }
    fn record_failure<'a>(
        &'a self,
        _: &'a collaboration_protocol::ScheduleFailure,
        _: agent_automation::RouteEffectEvidence<SessionRef, CodexGeneration>,
        _: bool,
    ) -> DeliveryFuture<'a, ()> {
        Box::pin(async { Err(DeliveryContractError::EvidencePersistence) })
    }
}
impl RunEvidenceSink for Evidence {
    fn record(
        &self,
        evidence: agent_automation::RouteEffectEvidence<SessionRef, CodexGeneration>,
    ) -> DeliveryFuture<'_, RunEvidenceDisposition> {
        self.0.lock().unwrap().push(evidence);
        Box::pin(async { Ok(RunEvidenceDisposition::Recorded { timing: None }) })
    }
    fn record_stop_intent(&self) -> DeliveryFuture<'_, RunEvidenceDisposition> {
        Box::pin(async { Err(DeliveryContractError::EvidencePersistence) })
    }
}

struct DirectRuntime {
    fixture: RuntimeFixture,
    native: NativeFixture,
    endpoint: EndpointRef,
    route: Arc<collaboration_runtime::SessionDeliveryRouter>,
}
impl DirectRuntime {
    async fn new(mode: SessionRegistrationMode, unavailable: bool) -> Self {
        let fixture = RuntimeFixture::new();
        let native = NativeFixture::start(fixture.root.path()).await;
        let service_id = "019f0000-0000-7000-8000-000000000777"
            .to_owned()
            .try_into()
            .unwrap();
        let endpoint = EndpointRef {
            service_id,
            endpoint_id: "codex-local".to_owned().try_into().unwrap(),
        };
        let gate = collaboration_runtime::NativeGenerationGate::default();
        gate.activate(
            serde_json::from_value(json!({"serviceEpoch":endpoint.service_id,"generation":1}))
                .unwrap(),
            fixture.root.path().join("backend.sock"),
            Some(Arc::new(
                codex_native_integration::NativePayloadSchemas::from_bundle(native.export.bundle())
                    .unwrap(),
            )),
        )
        .unwrap();
        let backend = collaboration_runtime::NativeControlBackend {
            endpoint: endpoint.clone(),
            gate,
            codex_home: fixture.root.path().join("codex-home"),
        };
        let input = match mode {
            SessionRegistrationMode::AccessEnrollment => {
                super::super::registrar_composition::RegistrarStartupInput::AccessEnrollment
            }
            SessionRegistrationMode::MachineTokens => {
                super::super::registrar_composition::RegistrarStartupInput::Injected(
                    fixture.capabilities(mode, true),
                )
            }
        };
        let registrar = input
            .compose(
                &endpoint.service_id,
                "127.0.0.1:8788".parse().unwrap(),
                "http://127.0.0.1:8788/mcp",
            )
            .unwrap();
        let registration = if unavailable {
            collaboration_runtime::NativeSessionRegistration::MachineTokensUnavailable
        } else {
            registrar.direct_native_registration()
        };
        let broker = collaboration_runtime::ServiceInteractionBroker::load(
            endpoint.service_id.clone(),
            backend.clone(),
            fixture.root.path().join("approvals.json"),
        )
        .await
        .unwrap();
        let routes = crate::session_message_route_composition::compose_session_message_routes(
            crate::session_message_route_composition::SessionMessageRouteInputs {
                provider_endpoints: Default::default(),
                directory: collaboration_runtime::EndpointDirectory::new(
                    endpoint.service_id.clone(),
                ),
                native_backend: backend,
                native_session_registration: registration,
                approval_broker: broker,
                unmaterialized_threads: Arc::new(
                    collaboration_runtime::UnmaterializedThreadHolder::new(),
                ),
                provider_supervisor: None,
                provider_store: None,
                peer_registry_directory: fixture.root.path().join("peers"),
            },
        )
        .unwrap();
        Self {
            fixture,
            native,
            endpoint,
            route: routes.router,
        }
    }
    fn request(&self, fork: bool, operation: &str) -> SchedulePreparationRequest {
        let cwd = self.fixture.root.path().to_str().unwrap().to_owned();
        let project_id = Some(PROJECT.to_owned().try_into().unwrap());
        let destination = if fork {
            DestinationPreparation::Fork {
                source: SessionRef {
                    endpoint: self.endpoint.clone(),
                    session_id: "fork-source".to_owned().try_into().unwrap(),
                },
                through_turn_id: "fork-turn".to_owned().try_into().unwrap(),
                cwd,
                project_id,
            }
        } else {
            DestinationPreparation::Fresh {
                endpoint: self.endpoint.clone(),
                cwd,
                project_id,
            }
        };
        SchedulePreparationRequest {
            operation_id: operation.to_owned().try_into().unwrap(),
            schedule_id: FIRST.to_owned().try_into().unwrap(),
            expected_change_id: SECOND.to_owned().try_into().unwrap(),
            destination,
            instruction_text: "Fixture task".to_owned(),
            model: Some("fixture-native-model".to_owned()),
            effort: "medium".to_owned(),
        }
    }
}

// Catch an unwired direct route, registration before the actual ID, policy
// overrides, or repeat dispatch. No pre-fix semantic red observed yet.
#[tokio::test]
async fn f4a_native_delivery_create_and_fork_bind_and_register_once_without_changing_permissions() {
    let runtime = DirectRuntime::new(SessionRegistrationMode::MachineTokens, false).await;
    for (fork, operation) in [(false, FIRST), (true, SECOND)] {
        let request = runtime.request(fork, operation);
        let result = runtime
            .route
            .prepare_destination(request.clone(), &Evidence::default())
            .await
            .unwrap();
        assert!(
            matches!(result, SchedulePreparationOutcome::Prepared(_)),
            "{result:?}"
        );
        let repeated = runtime
            .route
            .prepare_destination(request, &Evidence::default())
            .await
            .unwrap();
        assert!(
            matches!(repeated, SchedulePreparationOutcome::Failed(_)),
            "{repeated:?}"
        );
    }
    let registrations = runtime
        .fixture
        .authority
        .registrations
        .lock()
        .unwrap()
        .clone();
    assert_eq!(registrations.len(), 2);
    let records = runtime.fixture.repository.records.lock().unwrap().clone();
    let creates = runtime.native.creates.lock().unwrap().clone();
    assert_eq!(creates.len(), 2);
    for (index, (create, record)) in creates.iter().zip(&records).enumerate() {
        assert_eq!(
            registrations[index].session.session_id.as_str(),
            format!("runtime-native-{}", index + 1)
        );
        assert!(matches!(
            registrations[index].started_by,
            fleet_identity_protocol::StartedBy::RouterOperator {}
        ));
        assert_eq!(
            create["params"]["config"],
            json!({"model_reasoning_effort":"medium","mcp_servers":{"agent-router":{"url":record.preparation().binding_address.expose_address()}}})
        );
        assert!(create["params"].get("permissions").is_none());
        assert!(create["params"].get("approvalPolicy").is_none());
    }
    assert_eq!(creates[0]["method"], "thread/start");
    assert_eq!(creates[1]["method"], "thread/fork");
    assert_eq!(creates[1]["params"]["threadId"], "fork-source");
    assert_eq!(creates[1]["params"]["lastTurnId"], "fork-turn");
    runtime.native.shutdown().await;
}

#[tokio::test]
async fn f4a_native_delivery_missing_capability_never_creates() {
    let runtime = DirectRuntime::new(SessionRegistrationMode::MachineTokens, true).await;
    let result = runtime
        .route
        .prepare_destination(runtime.request(false, FIRST), &Evidence::default())
        .await
        .unwrap();
    assert!(
        matches!(result, SchedulePreparationOutcome::Failed(_)),
        "{result:?}"
    );
    assert!(runtime.native.creates.lock().unwrap().is_empty());
    assert!(
        runtime
            .fixture
            .repository
            .records
            .lock()
            .unwrap()
            .is_empty()
    );
    assert!(
        runtime
            .fixture
            .authority
            .registrations
            .lock()
            .unwrap()
            .is_empty()
    );
    runtime.native.shutdown().await;
}

#[tokio::test]
async fn f4a_native_delivery_access_keeps_the_original_unbound_config() {
    let runtime = DirectRuntime::new(SessionRegistrationMode::AccessEnrollment, false).await;
    let mut request = runtime.request(false, FIRST);
    let DestinationPreparation::Fresh { project_id, .. } = &mut request.destination else {
        unreachable!()
    };
    *project_id = None;
    let result = runtime
        .route
        .prepare_destination(request, &Evidence::default())
        .await
        .unwrap();
    assert!(
        matches!(result, SchedulePreparationOutcome::Prepared(_)),
        "{result:?}"
    );
    assert_eq!(
        runtime.native.creates.lock().unwrap()[0]["params"]["config"],
        json!({"model_reasoning_effort":"medium"})
    );
    assert!(runtime.fixture.authority.checks.lock().unwrap().is_empty());
    assert!(
        runtime
            .fixture
            .repository
            .records
            .lock()
            .unwrap()
            .is_empty()
    );
    runtime.native.shutdown().await;
}

#[tokio::test]
async fn f4a_native_delivery_fresh_run_uses_its_durable_run_id_and_never_replays() {
    let runtime = DirectRuntime::new(SessionRegistrationMode::MachineTokens, false).await;
    let request = collaboration_contracts::FreshSessionRequest {
        run_id: SECOND.to_owned().try_into().unwrap(), endpoint: runtime.endpoint.clone(),
        working_directory: runtime.fixture.root.path().to_str().unwrap().to_owned(),
        project_id: Some(PROJECT.to_owned().try_into().unwrap()), message: "Fixture task".to_owned().try_into().unwrap(),
        inputs: serde_json::from_value(json!({"scheduleChangeId":FIRST,"instructionRevisionId":FIRST,"instructionText":"Fixture task","continuity":{"kind":"none"},"executionConfiguration":{"destination":{"kind":"freshEachRun","endpoint":runtime.endpoint,"cwd":runtime.fixture.root.path(),"projectId":PROJECT},"executionTimeoutSeconds":null,"model":"fixture-native-model","effort":"medium"}})).unwrap(),
    };
    runtime
        .route
        .prepare_fresh_session(request.clone(), &Evidence::default())
        .await
        .unwrap();
    assert!(
        runtime
            .route
            .prepare_fresh_session(request, &Evidence::default())
            .await
            .is_err()
    );
    assert_eq!(
        runtime
            .fixture
            .authority
            .registrations
            .lock()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(runtime.native.creates.lock().unwrap().len(), 1);
    assert_eq!(
        runtime.fixture.repository.records.lock().unwrap()[0]
            .preparation()
            .launch
            .operation_id
            .as_str(),
        SECOND
    );
    runtime.native.shutdown().await;
}
