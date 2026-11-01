//! Declared input identity is separate from the persisted F1 registration facts.
use super::*;

#[tokio::test]
async fn fleet_session_registrar_refuses_changed_effort_and_access_under_a_prepared_operation() {
    let fixture = Fixture::new();
    let intent = fixture.intent();
    fixture
        .registrar
        .prepare(&intent, &fixture.identity(&intent))
        .await
        .unwrap();
    let before = fixture
        .repository
        .records
        .lock()
        .unwrap()
        .first()
        .unwrap()
        .clone();
    for change_effort in [true, false] {
        let mut changed = fixture.declared_input(&intent);
        let collaboration_contracts::HarnessDeclaredSettings::Native { effort, access, .. } =
            &mut changed.settings
        else {
            panic!("native fixture settings");
        };
        if change_effort {
            *effort = Some("high".to_owned().try_into().unwrap());
        } else {
            *access = collaboration_protocol::RouterAccess::WriteRestricted;
        }
        let identity = changed.identity().unwrap();

        let answer = fixture.registrar.prepare(&intent, &identity).await;

        assert!(matches!(answer, Err(RegistrarFailure::OperationConflict)));
        assert!(matches!(
            fixture
                .registrar
                .prepare_harness_start(&intent, &identity)
                .await,
            Err(RegistrarFailure::OperationConflict)
        ));
        assert_eq!(
            fixture.repository.records.lock().unwrap().first(),
            Some(&before)
        );
        assert!(fixture.authority.registrations.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn fleet_session_registrar_harness_handoff_preserves_the_prepared_starter_and_consumes_once()
{
    let fixture = Fixture::new();
    let mut trusted = fixture.intent();
    trusted.started_by = fleet_identity_protocol::StartedBy::Session {
        session: fixture.session(),
    };
    let identity = fixture.identity(&trusted);
    fixture
        .registrar
        .prepare(&trusted, &identity)
        .await
        .unwrap();
    let mut harness = trusted.clone();
    harness.started_by = fleet_identity_protocol::StartedBy::RouterOperator {};

    let PrepareSessionStart::Existing(retained) = fixture
        .registrar
        .prepare_harness_start(&harness, &identity)
        .await
        .unwrap()
    else {
        panic!("prepared handoff must retain its row");
    };
    assert_eq!(retained.preparation().launch.started_by, trusted.started_by);
    assert_eq!(retained.preparation().launch.isolation, trusted.isolation);
    fixture
        .registrar
        .mark_dispatched(&trusted.operation_id)
        .await
        .unwrap();
    assert!(matches!(
        fixture
            .registrar
            .prepare_harness_start(&harness, &identity)
            .await,
        Err(RegistrarFailure::InvalidTransition)
    ));
    assert!(fixture.authority.registrations.lock().unwrap().is_empty());
}

#[tokio::test]
async fn fleet_session_registrar_harness_without_preparation_records_operator_despite_a_session_claim()
 {
    let fixture = Fixture::new();
    let mut harness = fixture.intent();
    harness.started_by = fleet_identity_protocol::StartedBy::Session {
        session: fixture.session(),
    };
    let identity = fixture.identity(&harness);

    let PrepareSessionStart::Prepared(retained) = fixture
        .registrar
        .prepare_harness_start(&harness, &identity)
        .await
        .unwrap()
    else {
        panic!("operator start must prepare a new row");
    };

    assert_eq!(
        retained.preparation().launch.started_by,
        fleet_identity_protocol::StartedBy::RouterOperator {}
    );
    assert!(fixture.authority.registrations.lock().unwrap().is_empty());
}

#[tokio::test]
async fn fleet_session_registrar_harness_never_reconstructs_a_lost_in_memory_identity() {
    let fixture = Fixture::new();
    let harness = fixture.intent();
    let identity = fixture.identity(&harness);
    fixture
        .registrar
        .prepare(&harness, &identity)
        .await
        .unwrap();
    let reconstructed = Fixture::make_registrar(
        fixture.repository.clone(),
        fixture.authority.clone(),
        fixture.mode.clone(),
        "http://127.0.0.1:8788",
    );

    assert!(matches!(
        reconstructed
            .prepare_harness_start(&harness, &identity)
            .await,
        Err(RegistrarFailure::OperationConflict)
    ));
    reconstructed
        .settle_interrupted_setups_after_restart()
        .await
        .unwrap();
    assert!(matches!(
        reconstructed
            .prepare_harness_start(&harness, &identity)
            .await,
        Err(RegistrarFailure::InvalidTransition)
    ));
    assert!(fixture.authority.registrations.lock().unwrap().is_empty());
}
