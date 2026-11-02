//! Real Host caller/registrar/SQLite boundaries. Central and ACP answers are
//! explicit stand-ins; real central/provider acceptance remains a separate gate.
use super::{
    FIRST, SECOND,
    fixture::{PROJECT, RuntimeFixture, call},
    provider, script, wire,
};
use codex_router_fleet_state::{
    FleetStateStore,
    fleet_session_registration::{
        AbandonedSessionCreation, FleetSessionBindingLifecycle, FleetSessionPreparation,
        FleetSessionRegistrationRecord, PreparedSessionCreation,
    },
    session_registration_repository::FleetSessionRegistrationRepository,
};
use collaboration_service::fleet_session_registrar::{
    FleetSessionRegistrar, FleetSessionRegistrarProps, SessionBindingListener,
    SessionRegistrationMode,
};
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn f4a_real_store_public_provider_create_and_restarted_load_keep_one_registration_and_binding()
 {
    let fixture = RuntimeFixture::new();
    let router_root = fixture.root.path().join("router");
    let store = Arc::new(
        FleetStateStore::open_and_migrate(&router_root)
            .await
            .unwrap(),
    );
    let mut capabilities = fixture.capabilities(SessionRegistrationMode::MachineTokens, true);
    capabilities.repository = Some(store.clone());
    let requests = fixture.root.path().join("create.jsonl");
    let runtime = crate::CollaborationRuntime::runtime_test_start_with_registrar(
        fixture.inputs(true),
        vec![provider(
            script()
                .expect_request("create", "session/new", json!({}))
                .respond("create", json!({"sessionId":"persisted-provider"}))
                .record_requests(&requests),
        )],
        capabilities,
    )
    .await
    .unwrap();
    let stable_address = runtime.mcp_listen_address();

    let created = call(
        &fixture.manifest().mcp.url,
        "conversation_create",
        fixture.create(FIRST, "cursor-local", Some(PROJECT)),
    )
    .await;
    assert_eq!(created["isError"], false, "{created}");
    assert_eq!(created["structuredContent"]["kind"], "created", "{created}");
    let saved = store
        .operation(&FIRST.to_owned().try_into().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        saved.lifecycle().session().unwrap().session_id.as_str(),
        "persisted-provider"
    );
    let binding = saved.preparation().binding_address.clone();
    let target = created["structuredContent"]["target"].clone();
    runtime.shutdown().await.unwrap();
    store.close().await;

    let reopened = Arc::new(
        FleetStateStore::open_and_migrate(&router_root)
            .await
            .unwrap(),
    );
    assert_eq!(
        reopened.binding(&binding).await.unwrap(),
        Some(saved.clone())
    );
    let mut inputs = fixture.inputs(false);
    inputs.mcp_bind = stable_address;
    let mut capabilities = fixture.capabilities(SessionRegistrationMode::MachineTokens, true);
    capabilities.repository = Some(reopened.clone());
    let loads = fixture.root.path().join("load.jsonl");
    let restore=crate::external_provider_runtime::acp_scripted_fixture::AcpFixtureScript::new()
        .expect_request("initialize","initialize",json!({"protocolVersion":1}))
        .respond("initialize",json!({"protocolVersion":1,"agentInfo":{"name":"cursor","version":"fixture"},"agentCapabilities":{"mcpCapabilities":{"http":true},"loadSession":true}}))
        .expect_request("load","session/load",json!({"sessionId":"persisted-provider"}))
        .respond("load",json!({})).record_requests(&loads);
    let runtime = crate::CollaborationRuntime::runtime_test_start_with_registrar(
        inputs,
        vec![provider(restore)],
        capabilities,
    )
    .await
    .unwrap();
    let loaded=call(&fixture.manifest().mcp.url,"conversation_load",json!({"operationId":SECOND,"target":target,"workingDirectory":fixture.root.path(),"requestedBy":{"humanId":"fixture-owner"},"approver":{"humanId":"fixture-owner"},"access":"workspace-write","timeoutSeconds":2})).await;
    assert_eq!(loaded["isError"], false, "{loaded}");
    assert_eq!(loaded["structuredContent"]["kind"], "completed", "{loaded}");
    assert_eq!(reopened.binding(&binding).await.unwrap(), Some(saved));
    assert_eq!(fixture.authority.registrations.lock().unwrap().len(), 1);
    assert!(
        fixture.repository.records.lock().unwrap().is_empty(),
        "the memory stand-in must receive no record"
    );
    runtime.shutdown().await.unwrap();
    let calls = wire(&loads);
    let load = calls
        .iter()
        .find(|request| request["method"] == "session/load")
        .unwrap();
    assert_eq!(
        load["params"]["mcpServers"][0]["url"],
        binding.expose_address()
    );
    assert_eq!(
        wire(&requests)
            .iter()
            .filter(|request| request["method"] == "session/new")
            .count(),
        1
    );
    reopened.close().await;
}

#[tokio::test]
async fn f4a_real_store_restart_settlement_keeps_the_actual_id_and_never_registers_or_replays_setup()
 {
    use collaboration_protocol::SessionSetupAbandonmentCause;
    let fixture = RuntimeFixture::new();
    let router_root = fixture.root.path().join("router");
    let store = Arc::new(
        FleetStateStore::open_and_migrate(&router_root)
            .await
            .unwrap(),
    );
    let installation = "019f0000-0000-7000-8000-000000000111";
    let mut operations = Vec::new();
    let mut addresses = Vec::new();
    for (ordinal, known_created) in [(1u16, false), (2, true)] {
        let preparation=FleetSessionPreparation {
            registration_id:format!("019f0000-0000-7000-8000-{ordinal:012x}").try_into().unwrap(),
            operation_id:format!("019f0000-0000-7000-8000-{:012x}",u32::from(ordinal)+0x100).try_into().unwrap(),
            installation:installation.to_owned().try_into().unwrap(),
            binding_address:format!("http://127.0.0.1:8788/mcp/session/{ordinal:032x}").try_into().unwrap(),
            launch:serde_json::from_value(json!({"endpoint":{"serviceId":installation,"endpointId":"cursor-local"},"projectId":PROJECT,"checkedWorkspace":"019f0000-0000-7000-8000-000000000003","startedBy":{"kind":"routerOperator"},"isolation":{"kind":"host","instanceId":"019f0000-0000-7000-8000-000000000112"},"launchContext":{"workingDirectory":fixture.root.path()},"machineEnvironment":"cloudVm"})).unwrap(),
        };
        operations.push(preparation.operation_id.clone());
        addresses.push(preparation.binding_address.clone());
        let creation = if known_created {
            PreparedSessionCreation::CreatedPendingSetup {session:serde_json::from_value(json!({"endpoint":{"serviceId":installation,"endpointId":"cursor-local"},"sessionId":"actual-before-restart"})).unwrap()}
        } else {
            PreparedSessionCreation::NotDispatched {}
        };
        store
            .insert(
                &FleetSessionRegistrationRecord::new(
                    preparation,
                    FleetSessionBindingLifecycle::Prepared { creation },
                )
                .unwrap(),
            )
            .await
            .unwrap();
    }
    store.close().await;
    let reopened = Arc::new(
        FleetStateStore::open_and_migrate(&router_root)
            .await
            .unwrap(),
    );
    let capabilities = fixture.capabilities(SessionRegistrationMode::MachineTokens, true);
    let registrar = FleetSessionRegistrar::new(FleetSessionRegistrarProps {
        installation: installation.to_owned().try_into().unwrap(),
        listener: SessionBindingListener::Stable(
            "http://127.0.0.1:8788".to_owned().try_into().unwrap(),
        ),
        repository: reopened.clone(),
        connection: fixture.authority.clone(),
        mode: capabilities.mode,
        ids: capabilities.ids,
        entropy: capabilities.entropy,
    });

    registrar
        .settle_interrupted_setups_after_restart()
        .await
        .unwrap();
    registrar
        .settle_interrupted_setups_after_restart()
        .await
        .unwrap();

    let undispatched = reopened.operation(&operations[0]).await.unwrap().unwrap();
    assert!(matches!(
        undispatched.lifecycle(),
        FleetSessionBindingLifecycle::Abandoned {
            creation: AbandonedSessionCreation::NotCreated {}
        }
    ));
    let known = reopened.operation(&operations[1]).await.unwrap().unwrap();
    assert!(
        matches!(known.lifecycle(),FleetSessionBindingLifecycle::Abandoned{creation:AbandonedSessionCreation::KnownCreated{session,cause:SessionSetupAbandonmentCause::SetupInterrupted{}}} if session.session_id.as_str()=="actual-before-restart")
    );
    assert!(
        registrar
            .complete_created_setup(&operations[1])
            .await
            .is_err()
    );
    for address in addresses {
        assert!(matches!(
            reopened
                .binding(&address)
                .await
                .unwrap()
                .unwrap()
                .lifecycle(),
            FleetSessionBindingLifecycle::Abandoned { .. }
        ));
    }
    assert!(fixture.authority.checks.lock().unwrap().is_empty());
    assert!(fixture.authority.registrations.lock().unwrap().is_empty());
    reopened.close().await;
}
