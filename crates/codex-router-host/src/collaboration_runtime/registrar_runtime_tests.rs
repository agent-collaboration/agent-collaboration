//! Public in-process caller composition; stand-ins earn no storage or restart proof.
#[path = "registrar_runtime_test_fixture.rs"]
mod fixture;
#[path = "registrar_native_test_fixture.rs"]
mod native_fixture;
use crate::external_provider_runtime::acp_scripted_fixture::AcpFixtureScript;
use collaboration_service::fleet_session_registrar::SessionRegistrationMode;
use fixture::{PROJECT, RuntimeFixture, call};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;

const FIRST: &str = "019f0000-0000-7000-8000-000000000301";
const SECOND: &str = "019f0000-0000-7000-8000-000000000302";

fn provider(script: AcpFixtureScript) -> crate::ExternalProviderStartup {
    let launch = script.launch();
    let mut binding =
        crate::ExternalProviderLaunchBinding::cursor(launch.executable, launch.arguments).unwrap();
    binding.launch.environment = launch.environment;
    crate::ExternalProviderStartup::Launch(binding)
}
fn script() -> AcpFixtureScript {
    AcpFixtureScript::new()
        .expect_request("initialize", "initialize", json!({"protocolVersion":1}))
        .respond("initialize", json!({"protocolVersion":1,"agentInfo":{"name":"cursor","version":"fixture"},"agentCapabilities":{"mcpCapabilities":{"http":true}}}))
}
fn wire(path: &std::path::Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

// Oracle: Lead01a1984d and existing explicit mode/repository contracts.
// Wrong behavior: selecting Access when token-mode storage is unavailable.
// No pre-fix semantic red observed for the newly composed entrypoint.
#[tokio::test]
async fn f4a_registrar_runtime_missing_repository_is_typed_unavailable_before_startup() {
    let fixture = RuntimeFixture::new();
    let result = crate::CollaborationRuntime::runtime_test_start_with_registrar(
        fixture.inputs(true),
        Vec::new(),
        fixture.capabilities(SessionRegistrationMode::MachineTokens, false),
    )
    .await;
    let error = result.err().unwrap();
    assert_eq!(error.get_ref().and_then(|source|source.downcast_ref::<codex_router_fleet_state::session_registration_repository::FleetSessionStorageFailure>()),Some(&codex_router_fleet_state::session_registration_repository::FleetSessionStorageFailure::Unavailable));
    assert!(!fixture.root.path().join("service/service.json").exists());
}

#[tokio::test]
async fn f4a_registrar_runtime_project_and_authority_refusals_never_dispatch_provider_create() {
    let fixture = RuntimeFixture::new();
    let requests = fixture.root.path().join("provider.jsonl");
    let runtime = fixture
        .start(
            true,
            SessionRegistrationMode::MachineTokens,
            vec![provider(script().record_requests(&requests))],
        )
        .await
        .unwrap();
    let url = fixture.manifest().mcp.url;

    let absent = call(
        &url,
        "conversation_create",
        fixture.create(FIRST, "cursor-local", None),
    )
    .await;
    assert!(absent.to_string().contains("projectRequired"), "{absent}");
    assert!(fixture.authority.checks.lock().unwrap().is_empty());
    fixture.authority.reject.store(true, Ordering::SeqCst);
    let refused = call(
        &url,
        "conversation_create",
        fixture.create(SECOND, "cursor-local", Some(PROJECT)),
    )
    .await;
    assert!(
        refused.to_string().contains("machineNotAllowed"),
        "{refused}"
    );
    assert_eq!(fixture.authority.checks.lock().unwrap().len(), 1);
    assert!(fixture.repository.records.lock().unwrap().is_empty());
    runtime.shutdown().await.unwrap();
    assert_eq!(
        wire(&requests)
            .iter()
            .filter(|request| request["method"] == "session/new")
            .count(),
        0
    );
}

#[tokio::test]
async fn f4a_registrar_runtime_public_create_registers_actual_provider_and_bound_helper_starter() {
    let fixture = RuntimeFixture::new();
    let requests = fixture.root.path().join("provider.jsonl");
    let provider = provider(
        script()
            .expect_request("first", "session/new", json!({}))
            .respond("first", json!({"sessionId":"runtime-provider-one"}))
            .expect_request("helper", "session/new", json!({}))
            .respond("helper", json!({"sessionId":"runtime-provider-two"}))
            .record_requests(&requests),
    );
    let runtime = fixture
        .start(true, SessionRegistrationMode::MachineTokens, vec![provider])
        .await
        .unwrap();
    let first = call(
        &fixture.manifest().mcp.url,
        "conversation_create",
        fixture.create(FIRST, "cursor-local", Some(PROJECT)),
    )
    .await;
    assert_eq!(first["isError"], false, "{first}");
    assert_eq!(first["structuredContent"]["kind"], "created", "{first}");
    let record = fixture.repository.records.lock().unwrap()[0].clone();
    let bound_url = record
        .preparation()
        .binding_address
        .expose_address()
        .to_owned();
    let target = first["structuredContent"]["target"].clone();
    let spoofed=call(&bound_url,"message_send",json!({
        "target":target,"message":{"kind":"agent","sender":{"endpoint":{"serviceId":fixture.manifest().service_id,"endpointId":"cursor-local"},"sessionId":"spoofed-session"},"text":"must not dispatch"}
    })).await;
    assert_eq!(spoofed["isError"], true, "{spoofed}");
    assert_eq!(
        spoofed["structuredContent"]["kind"], "senderMismatch",
        "{spoofed}"
    );
    let helper = call(
        &bound_url,
        "conversation_create",
        fixture.create(SECOND, "cursor-local", Some(PROJECT)),
    )
    .await;
    assert_eq!(helper["isError"], false, "{helper}");
    assert_eq!(helper["structuredContent"]["kind"], "created", "{helper}");
    let registrations = fixture.authority.registrations.lock().unwrap().clone();
    assert_eq!(registrations.len(), 2);
    assert_eq!(
        registrations[0].session.session_id.as_str(),
        "runtime-provider-one"
    );
    assert_eq!(
        registrations[1].session.session_id.as_str(),
        "runtime-provider-two"
    );
    assert!(matches!(
        registrations[0].started_by,
        fleet_identity_protocol::StartedBy::RouterOperator {}
    ));
    assert!(
        matches!(&registrations[1].started_by,fleet_identity_protocol::StartedBy::Session{session} if session==&registrations[0].session)
    );
    assert!(
        registrations.iter().all(|record| record.machine_environment
            == fleet_identity_protocol::MachineEnvironment::CloudVm)
    );
    runtime.shutdown().await.unwrap();
    let creates: Vec<_> = wire(&requests)
        .into_iter()
        .filter(|request| request["method"] == "session/new")
        .collect();
    assert_eq!(creates.len(), 2);
    for (create, record) in creates
        .iter()
        .zip(fixture.repository.records.lock().unwrap().iter())
    {
        assert_eq!(
            create["params"]["mcpServers"][0]["url"],
            record.preparation().binding_address.expose_address()
        );
        assert_eq!(
            create["params"]["mcpServers"][0]["name"],
            "router-collaboration"
        );
    }
}

// R8 and the bound native config use the real public client/ACP/native wire;
// only the app-server answer and central/store are stand-ins. No pre-fix red.
#[tokio::test]
async fn f4a_registrar_runtime_native_create_uses_the_same_owner_before_thread_start() {
    let fixture = RuntimeFixture::new();
    let native = native_fixture::NativeFixture::start(fixture.root.path()).await;
    let mut inputs = fixture.inputs(true);
    inputs.native_schema = Some(native.export.clone());
    let mut runtime = crate::CollaborationRuntime::runtime_test_start_with_registrar(
        inputs,
        Vec::new(),
        fixture.capabilities(SessionRegistrationMode::MachineTokens, true),
    )
    .await
    .unwrap();
    runtime
        .backend_ready(
            "2026-11-01T00:00:00Z".to_owned().try_into().unwrap(),
            Some(crate::BackendSchemaEvidence {
                running_executable: native.export.executable(),
                export: native.export.as_ref(),
            }),
        )
        .await
        .unwrap();
    let endpoint = json!({"serviceId":fixture.manifest().service_id,"endpointId":"codex-local"});
    let permission_actor = json!({"endpoint":endpoint,"sessionId":"local-native-requester"});
    let mut missing = fixture.create(FIRST, "codex-local", None);
    missing["createdBy"] = permission_actor.clone();
    missing["approver"] = permission_actor.clone();
    let refused = call(
        &fixture.manifest().mcp.url,
        "conversation_create",
        missing.clone(),
    )
    .await;
    assert!(refused.to_string().contains("projectRequired"), "{refused}");
    assert!(native.creates.lock().unwrap().is_empty());
    missing["operationId"] = json!(SECOND);
    missing["projectId"] = json!(PROJECT);
    missing["model"] = json!("gpt-6-sol");
    missing["effort"] = json!("medium");
    let created = call(&fixture.manifest().mcp.url, "conversation_create", missing).await;
    assert_eq!(created["isError"], false, "{created}");
    assert_eq!(created["structuredContent"]["kind"], "created", "{created}");
    let registrations = fixture.authority.registrations.lock().unwrap().clone();
    assert_eq!(registrations.len(), 1);
    assert_eq!(
        registrations[0].session.session_id.as_str(),
        "runtime-native-1"
    );
    let record = fixture.repository.records.lock().unwrap()[0].clone();
    let creates = native.creates.lock().unwrap().clone();
    assert_eq!(creates.len(), 1);
    assert_eq!(
        creates[0]["params"]["config"]["mcp_servers.agent-router.url"],
        record.preparation().binding_address.expose_address()
    );
    runtime.shutdown().await.unwrap();
    native.shutdown().await;
}

#[tokio::test]
async fn f4a_registrar_runtime_access_control_preserves_projectless_provider_create() {
    let fixture = RuntimeFixture::new();
    let provider = provider(
        script()
            .expect_request("create", "session/new", json!({}))
            .respond("create", json!({"sessionId":"access-session"})),
    );
    let runtime = fixture
        .start(
            true,
            SessionRegistrationMode::AccessEnrollment,
            vec![provider],
        )
        .await
        .unwrap();
    let result = call(
        &fixture.manifest().mcp.url,
        "conversation_create",
        fixture.create(FIRST, "cursor-local", None),
    )
    .await;
    assert_eq!(result["isError"], false, "{result}");
    assert_eq!(result["structuredContent"]["kind"], "created", "{result}");
    assert!(fixture.authority.checks.lock().unwrap().is_empty());
    assert!(fixture.repository.records.lock().unwrap().is_empty());
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn f4a_registrar_runtime_ephemeral_listener_refuses_before_provider_create() {
    let fixture = RuntimeFixture::new();
    let requests = fixture.root.path().join("provider.jsonl");
    let runtime = fixture
        .start(
            false,
            SessionRegistrationMode::MachineTokens,
            vec![provider(script().record_requests(&requests))],
        )
        .await
        .unwrap();
    let result = call(
        &fixture.manifest().mcp.url,
        "conversation_create",
        fixture.create(FIRST, "cursor-local", Some(PROJECT)),
    )
    .await;
    assert_eq!(result["isError"], true, "{result}");
    assert!(fixture.authority.checks.lock().unwrap().is_empty());
    runtime.shutdown().await.unwrap();
    assert_eq!(
        wire(&requests)
            .iter()
            .filter(|request| request["method"] == "session/new")
            .count(),
        0
    );
}
