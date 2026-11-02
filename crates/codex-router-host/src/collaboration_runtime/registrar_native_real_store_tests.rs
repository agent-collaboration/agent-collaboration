//! Actual Host native route and SQL rows; central/native replies remain stand-ins.
use super::{DirectRuntime, Evidence, FIRST, SECOND};
use codex_router_fleet_state::{
    FleetStateStore, session_registration_repository::FleetSessionRegistrationRepository,
};
use collaboration_contracts::{SchedulePreparationOutcome, ScheduledRunExecution};
use collaboration_service::fleet_session_registrar::SessionRegistrationMode;
use serde_json::json;
use std::{os::unix::fs::PermissionsExt, sync::Arc};

#[tokio::test]
async fn f4a_real_store_native_create_and_fork_keep_their_returned_ids_and_bindings_after_reopen() {
    let root = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let store = Arc::new(
        FleetStateStore::open_and_migrate(root.path())
            .await
            .unwrap(),
    );
    let runtime = DirectRuntime::with_repository(
        SessionRegistrationMode::MachineTokens,
        false,
        Some(store.clone()),
    )
    .await;
    let mut retained = Vec::new();
    for (fork, operation) in [(false, FIRST), (true, SECOND)] {
        let request = runtime.request(fork, operation);
        let outcome = runtime
            .route
            .prepare_destination(request.clone(), &Evidence::default())
            .await
            .unwrap();
        let SchedulePreparationOutcome::Prepared(prepared) = outcome else {
            panic!("native preparation failed: {outcome:?}");
        };
        let record = store
            .operation(&operation.to_owned().try_into().unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            record.lifecycle().session().unwrap().session_id.as_str(),
            String::from(prepared.target.session_id)
        );
        retained.push(record);
        assert!(matches!(
            runtime
                .route
                .prepare_destination(request, &Evidence::default())
                .await
                .unwrap(),
            SchedulePreparationOutcome::Failed(_)
        ));
    }
    assert_eq!(
        runtime
            .fixture
            .authority
            .registrations
            .lock()
            .unwrap()
            .len(),
        2
    );
    assert!(
        runtime
            .fixture
            .repository
            .records
            .lock()
            .unwrap()
            .is_empty(),
        "memory repository must not receive native records"
    );
    let creates = runtime.native.creates.lock().unwrap().clone();
    assert_eq!(creates.len(), 2);
    for (create, record) in creates.iter().zip(&retained) {
        assert_eq!(
            create["params"]["config"],
            json!({"model_reasoning_effort":"medium","mcp_servers":{"agent-router":{"url":record.preparation().binding_address.expose_address()}}})
        );
        assert!(create["params"].get("permissions").is_none());
    }
    runtime.native.shutdown().await;
    store.close().await;

    let reopened = FleetStateStore::open_read_only(root.path()).await.unwrap();
    for record in retained {
        assert_eq!(
            reopened
                .binding(&record.preparation().binding_address)
                .await
                .unwrap(),
            Some(record.clone())
        );
        assert_eq!(
            reopened
                .conversation(record.lifecycle().session().unwrap())
                .await
                .unwrap(),
            Some(record)
        );
    }
    reopened.close().await;
}
