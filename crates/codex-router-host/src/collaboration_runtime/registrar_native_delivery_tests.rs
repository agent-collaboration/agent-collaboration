//! Real Host route composition and native wire; memory store/central and native answers stand in.
//! Oracle: Lead01a19876/01a19879, PD bindings wherever StartThread/ForkThread is built.
//! No pre-fix semantic red was observed for these new API scenarios.
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
use serde_json::json;
use std::sync::{Arc, Mutex, atomic::Ordering};

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
    // The actual Runtime retains this owner in its listeners. Delivery borrows
    // it weakly to avoid the broker -> delivery -> broker cycle.
    _approval_broker: Arc<collaboration_runtime::ServiceInteractionBroker>,
}
impl DirectRuntime {
    async fn new(mode: SessionRegistrationMode, unavailable: bool) -> Self {
        Self::with_repository(mode, unavailable, None).await
    }

    async fn with_repository(
        mode: SessionRegistrationMode,
        unavailable: bool,
        repository: Option<Arc<dyn codex_router_fleet_state::session_registration_repository::FleetSessionRegistrationRepository>>,
    ) -> Self {
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
        let generation: CodexGeneration =
            serde_json::from_value(json!({"serviceEpoch":endpoint.service_id,"generation":1}))
                .unwrap();
        let schemas = Arc::new(
            codex_native_integration::NativePayloadSchemas::from_bundle(native.export.bundle())
                .unwrap(),
        );
        gate.activate(
            generation.clone(),
            fixture.root.path().join("backend.sock"),
            Some(schemas.clone()),
        )
        .unwrap();
        let directory = collaboration_runtime::EndpointDirectory::new(endpoint.service_id.clone());
        directory.publish(serde_json::from_value(json!({
            "endpoint":endpoint,"label":"native fixture",
            "availability":{"state":"available","observedAt":"2026-11-01T00:00:00Z"},
            "channels":[{"kind":"nativeCodex","transport":"unixWebSocket","path":fixture.root.path().join("backend.sock"),"schemaDigest":schemas.schema_digest(),"generation":generation}]
        })).unwrap()).unwrap();
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
                let mut capabilities = fixture.capabilities(mode, true);
                if let Some(repository) = repository {
                    capabilities.repository = Some(repository);
                }
                super::super::registrar_composition::RegistrarStartupInput::Injected(capabilities)
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
                directory,
                native_backend: backend,
                native_session_registration: registration,
                approval_broker: broker.clone(),
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
            _approval_broker: broker,
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

    fn fresh_run_request(&self) -> collaboration_contracts::FreshSessionRequest {
        collaboration_contracts::FreshSessionRequest {
            run_id: SECOND.to_owned().try_into().unwrap(), endpoint: self.endpoint.clone(),
            working_directory: self.fixture.root.path().to_str().unwrap().to_owned(),
            project_id: Some(PROJECT.to_owned().try_into().unwrap()), message: "Fixture task".to_owned().try_into().unwrap(),
            inputs: serde_json::from_value(json!({"scheduleChangeId":FIRST,"instructionRevisionId":FIRST,"instructionText":"Fixture task","continuity":{"kind":"none"},"executionConfiguration":{"destination":{"kind":"freshEachRun","endpoint":self.endpoint,"cwd":self.fixture.root.path(),"projectId":PROJECT},"executionTimeoutSeconds":null,"model":"fixture-native-model","effort":"medium"}})).unwrap(),
        }
    }
}

#[path = "registrar_native_real_store_tests.rs"]
mod real_store;

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
            registrations[index].machine_environment,
            fleet_identity_protocol::MachineEnvironment::CloudVm
        );
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
        panic!("fresh native fixture destination")
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
    let request = runtime.fresh_run_request();
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
            .operation_id
            .as_str(),
        SECOND
    );
    runtime.native.shutdown().await;
}

// Catch a global/unbound cold resume or re-registration during restore. The
// cold native state is controlled by an event fact, without timing or polling.
#[tokio::test]
async fn f4a_native_delivery_cold_resume_reuses_the_saved_binding_and_refuses_an_unregistered_target()
 {
    let runtime = DirectRuntime::new(SessionRegistrationMode::MachineTokens, false).await;
    let prepared = runtime
        .route
        .prepare_destination(runtime.request(false, FIRST), &Evidence::default())
        .await
        .unwrap();
    let SchedulePreparationOutcome::Prepared(prepared) = prepared else {
        panic!("native start should be prepared");
    };
    runtime.native.cold.store(true, Ordering::SeqCst);
    let run = collaboration_contracts::ScheduledRunSubmission {
        run_id: SECOND.to_owned().try_into().unwrap(),
        target: prepared.target,
        payload: collaboration_contracts::ScheduledRunPayload::Fresh {
            task_input: "Fixture task".to_owned().try_into().unwrap(),
        },
        precondition: collaboration_contracts::DeliveryPrecondition::Unpinned,
        inputs: runtime.fresh_run_request().inputs,
        recorded: prepared.evidence,
    };
    let outcome = runtime
        .route
        .submit_run(run.clone(), &Evidence::default())
        .await
        .unwrap();
    assert!(
        matches!(
            outcome,
            collaboration_contracts::RunSubmission::NotStartedBusy
        ),
        "{outcome:?}"
    );
    let resumes = runtime.native.resumes.lock().unwrap().clone();
    assert_eq!(resumes.len(), 1);
    assert_eq!(resumes[0]["params"]["threadId"], "runtime-native-1");
    assert_eq!(
        resumes[0]["params"]["config"],
        json!({"mcp_servers":{"agent-router":{"url":runtime.fixture.repository.records.lock().unwrap()[0].preparation().binding_address.expose_address()}}})
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
    let mut unregistered = run;
    unregistered.target.session_id = "unregistered-thread".to_owned().try_into().unwrap();
    let agent_automation::RouteEffectEvidence::CodexAppServer(evidence) =
        &mut unregistered.recorded
    else {
        panic!("native evidence");
    };
    evidence.target = Some(unregistered.target.clone());
    let refused = runtime
        .route
        .submit_run(unregistered, &Evidence::default())
        .await
        .unwrap();
    assert!(
        matches!(refused, collaboration_contracts::RunSubmission::Rejected(_)),
        "{refused:?}"
    );
    assert_eq!(runtime.native.resumes.lock().unwrap().len(), 1);
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
    runtime.native.shutdown().await;
}

// Ordinary messages own a separate ResumeThread call from scheduled submission.
// Catch a missing injection there through the real composed route and native wire.
#[tokio::test]
async fn f4a_native_delivery_ordinary_message_resumes_with_the_same_active_binding() {
    use collaboration_contracts::SessionMessageDelivery;
    let runtime = DirectRuntime::new(SessionRegistrationMode::MachineTokens, false).await;
    let prepared = runtime
        .route
        .prepare_destination(runtime.request(false, FIRST), &Evidence::default())
        .await
        .unwrap();
    let SchedulePreparationOutcome::Prepared(prepared) = prepared else {
        panic!("native start should be prepared");
    };
    runtime.native.cold.store(true, Ordering::SeqCst);
    let push_id: collaboration_protocol::PushId = FIRST.to_owned().try_into().unwrap();
    let link = collaboration_protocol::RouterLink::new(
        collaboration_protocol::MachineId::from(runtime.endpoint.service_id.clone()),
        push_id.clone(),
    );
    let request = collaboration_contracts::layer_zero::DeliveryRequest {
        payload: collaboration_contracts::layer_zero::PreparedPush {
            push_id,
            line: format!("✉️ Fixture · \"Fixture message\" · {link}")
                .try_into()
                .unwrap(),
            load_policy: collaboration_contracts::LoadPolicy::MayLoad,
        },
        target: prepared.target,
        mode: collaboration_protocol::MessageDelivery::Auto,
        precondition: collaboration_contracts::DeliveryPrecondition::Unpinned,
        correlation: FIRST.to_owned().try_into().unwrap(),
        attempt: FIRST.to_owned().try_into().unwrap(),
    };
    let delivered = runtime
        .route
        .deliver(
            request.clone(),
            &collaboration_contracts::UnstoredAttemptEvidenceSink,
        )
        .await
        .unwrap();
    assert_eq!(
        delivered.receipt().outcome,
        collaboration_protocol::DeliveryOutcome::StartedOrSteered,
        "{:?}",
        delivered.receipt()
    );
    let resumes = runtime.native.resumes.lock().unwrap().clone();
    assert_eq!(resumes.len(), 1);
    assert_eq!(
        resumes[0]["params"]["config"],
        json!({"mcp_servers":{"agent-router":{"url":runtime.fixture.repository.records.lock().unwrap()[0].preparation().binding_address.expose_address()}}})
    );
    let mut unregistered = request;
    unregistered.target.session_id = "unregistered-thread".to_owned().try_into().unwrap();
    let refused = runtime
        .route
        .deliver(
            unregistered,
            &collaboration_contracts::UnstoredAttemptEvidenceSink,
        )
        .await
        .unwrap();
    assert_ne!(
        refused.receipt().outcome,
        collaboration_protocol::DeliveryOutcome::StartedOrSteered
    );
    assert_eq!(runtime.native.resumes.lock().unwrap().len(), 1);
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
    runtime.native.shutdown().await;
}
