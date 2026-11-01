//! Narrow policy evidence on event-controlled stand-ins; SQLite restart and real harness proofs remain separate.
use super::*;
use codex_router_state::fleet_session_registration::{
    FleetSessionBindingLifecycle, PreparedSessionCreation,
};
use collaboration_protocol::SessionSetupAbandonmentCause;
use fleet_identity_protocol::{
    RegisterFleetSessionResult, SessionEndCause, SessionStartCheck, SessionStartRefusal,
};
use futures_util::FutureExt;

#[path = "policy_test_fixture.rs"]
mod fixture;
use fixture::*;

#[path = "handoff_identity_tests.rs"]
mod handoff_identity_tests;

#[path = "identity_disclosure_tests.rs"]
mod identity_disclosure_tests;

#[tokio::test]
async fn fleet_session_registrar_restart_abandons_a_prepared_launch_before_dispatch() {
    let fixture = Fixture::new();
    let intent = fixture.intent();
    let PrepareSessionStart::Prepared(prepared) = fixture
        .registrar
        .prepare(&intent, &fixture.identity(&intent))
        .await
        .unwrap()
    else {
        panic!("fixture launch was not prepared");
    };
    let reconstructed = Fixture::make_registrar(
        fixture.repository.clone(),
        fixture.authority.clone(),
        fixture.mode.clone(),
        "http://127.0.0.1:8788",
    );

    reconstructed
        .settle_interrupted_setups_after_restart()
        .await
        .unwrap();

    assert!(matches!(
        reconstructed
            .resolve_binding(&prepared.preparation().binding_address)
            .await,
        Err(RegistrarFailure::InactiveBinding)
    ));
    let records = fixture.repository.records.lock().unwrap();
    assert!(matches!(records.first().unwrap().lifecycle(), FleetSessionBindingLifecycle::Abandoned {
        creation: codex_router_state::fleet_session_registration::AbandonedSessionCreation::NotCreated {}
    }));
    assert!(fixture.authority.registrations.lock().unwrap().is_empty());
}

#[tokio::test]
async fn fleet_session_registrar_known_created_local_refusals_keep_identity_and_never_register() {
    for cause in [
        SessionSetupAbandonmentCause::ModelMismatch {},
        SessionSetupAbandonmentCause::EffectiveWorkingDirectoryMismatch {},
        SessionSetupAbandonmentCause::WorkspaceRootsMismatch {},
    ] {
        let fixture = Fixture::new();
        let intent = fixture.intent();
        fixture
            .registrar
            .prepare(&intent, &fixture.identity(&intent))
            .await
            .unwrap();
        let address = fixture
            .registrar
            .mark_dispatched(&intent.operation_id)
            .await
            .unwrap();
        let session = fixture.session();
        fixture
            .registrar
            .record_provider_created(&intent.operation_id, session.clone())
            .await
            .unwrap();
        assert!(matches!(
            fixture.registrar.resolve_binding(&address).await,
            Err(RegistrarFailure::InactiveBinding)
        ));
        fixture
            .registrar
            .abandon_created_setup(&intent.operation_id, cause)
            .await
            .unwrap();

        {
            let records = fixture.repository.records.lock().unwrap();
            let record = records.first().unwrap();
            assert_eq!(record.lifecycle().session(), Some(&session));
            assert!(
                matches!(record.lifecycle(), FleetSessionBindingLifecycle::Abandoned {
                creation: codex_router_state::fleet_session_registration::AbandonedSessionCreation::KnownCreated { session: actual, cause: actual_cause }
            } if actual == &session && actual_cause == &cause)
            );
        }
        assert!(matches!(
            fixture
                .registrar
                .reconcile_registration(&intent.operation_id)
                .await,
            Err(RegistrarFailure::InactiveBinding)
        ));
        assert!(matches!(
            fixture
                .registrar
                .complete_created_setup(&intent.operation_id)
                .await,
            Err(RegistrarFailure::InvalidTransition)
        ));
        assert!(fixture.authority.registrations.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn fleet_session_registrar_reconstructed_owner_settles_pending_setup_without_registration() {
    let fixture = Fixture::new();
    let intent = fixture.intent();
    fixture
        .registrar
        .prepare(&intent, &fixture.identity(&intent))
        .await
        .unwrap();
    let address = fixture
        .registrar
        .mark_dispatched(&intent.operation_id)
        .await
        .unwrap();
    let session = fixture.session();
    fixture
        .registrar
        .record_provider_created(&intent.operation_id, session.clone())
        .await
        .unwrap();
    let live_reconcile = fixture
        .registrar
        .reconcile_registration(&intent.operation_id)
        .now_or_never();
    assert_eq!(
        live_reconcile.unwrap().unwrap(),
        RegistrationCompletion::Pending
    );
    let reconstructed = Fixture::make_registrar(
        fixture.repository.clone(),
        fixture.authority.clone(),
        fixture.mode.clone(),
        "http://127.0.0.1:8788",
    );

    reconstructed
        .settle_interrupted_setups_after_restart()
        .await
        .unwrap();

    assert!(matches!(
        reconstructed.resolve_binding(&address).await,
        Err(RegistrarFailure::InactiveBinding)
    ));
    let records = fixture.repository.records.lock().unwrap();
    let record = records.first().unwrap();
    assert_eq!(record.lifecycle().session(), Some(&session));
    assert!(matches!(record.lifecycle(), FleetSessionBindingLifecycle::Abandoned {
        creation: codex_router_state::fleet_session_registration::AbandonedSessionCreation::KnownCreated { cause: SessionSetupAbandonmentCause::SetupInterrupted {}, .. }
    }));
    assert!(fixture.authority.registrations.lock().unwrap().is_empty());
}

#[tokio::test]
async fn fleet_session_registrar_success_after_pending_setup_uses_the_original_registration() {
    let fixture = Fixture::new();
    let intent = fixture.intent();
    fixture
        .registrar
        .prepare(&intent, &fixture.identity(&intent))
        .await
        .unwrap();
    let address = fixture
        .registrar
        .mark_dispatched(&intent.operation_id)
        .await
        .unwrap();
    let session = fixture.session();
    fixture
        .registrar
        .record_provider_created(&intent.operation_id, session.clone())
        .await
        .unwrap();
    fixture.grant();

    let RegistrationCompletion::Registered(bound) = fixture
        .registrar
        .complete_created_setup(&intent.operation_id)
        .await
        .unwrap()
    else {
        panic!("completed setup did not register");
    };

    assert_eq!(bound.session(), &session);
    assert_eq!(
        fixture.registrar.resolve_binding(&address).await.unwrap(),
        bound
    );
    let sent = fixture.authority.registrations.lock().unwrap();
    assert_eq!(sent.len(), 1);
    let registration: fleet_identity_protocol::RegisterFleetSession =
        serde_json::from_slice(sent.first().unwrap()).unwrap();
    assert_eq!(registration.session, session);
    assert_eq!(Some(registration.project_id), intent.project_id);
}

#[tokio::test]
async fn fleet_session_registrar_missing_project_and_each_live_refusal_store_no_preparation() {
    let fixture = Fixture::new();
    let mut intent = fixture.intent();
    intent.project_id = None;
    assert!(matches!(
        fixture
            .registrar
            .prepare(&intent, &fixture.identity(&intent))
            .await,
        Err(RegistrarFailure::ProjectRequired)
    ));
    assert!(fixture.authority.events.lock().unwrap().is_empty());
    intent.project_id = Some(PROJECT.to_owned().try_into().unwrap());
    for reason in [
        SessionStartRefusal::ProjectUnknown {},
        SessionStartRefusal::MachineNotAllowed {},
        SessionStartRefusal::DifferentWorkspace {},
        SessionStartRefusal::StarterNotActive {},
    ] {
        *fixture.authority.check.lock().unwrap() = Ok(SessionStartCheck::Refused {
            reason: reason.clone(),
        });
        assert!(
            matches!(fixture.registrar.prepare(&intent, &fixture.identity(&intent)).await, Err(RegistrarFailure::StartRefused(actual)) if actual == reason)
        );
        assert!(fixture.repository.records.lock().unwrap().is_empty());
    }
    *fixture.authority.check.lock().unwrap() = Err(FleetSessionConnectionFailure::Unavailable);
    assert!(matches!(
        fixture
            .registrar
            .prepare(&intent, &fixture.identity(&intent))
            .await,
        Err(RegistrarFailure::Connection(_))
    ));
    assert!(fixture.repository.records.lock().unwrap().is_empty());
}

#[tokio::test]
async fn fleet_session_registrar_access_enrollment_preserves_the_old_no_registration_path() {
    let fixture = Fixture::new();
    *fixture.mode.0.lock().unwrap() = SessionRegistrationMode::AccessEnrollment;
    let mut intent = fixture.intent();
    intent.project_id = None;
    assert!(matches!(
        fixture
            .registrar
            .prepare(&intent, &fixture.identity(&intent))
            .await
            .unwrap(),
        PrepareSessionStart::AccessEnrollment
    ));
    assert!(fixture.authority.events.lock().unwrap().is_empty());
    assert!(fixture.repository.records.lock().unwrap().is_empty());
    *fixture.mode.0.lock().unwrap() = SessionRegistrationMode::MachineTokens;
    assert!(matches!(
        fixture
            .registrar
            .prepare(&intent, &fixture.identity(&intent))
            .await,
        Err(RegistrarFailure::ProjectRequired)
    ));
}

#[tokio::test]
async fn fleet_session_registrar_ephemeral_listener_keeps_step_one_and_refuses_new_registration() {
    let fixture = Fixture::new();
    let registrar = fixture.ephemeral_registrar();
    let intent = fixture.intent();
    *fixture.mode.0.lock().unwrap() = SessionRegistrationMode::AccessEnrollment;
    assert!(matches!(
        registrar
            .prepare(&intent, &fixture.identity(&intent))
            .await
            .unwrap(),
        PrepareSessionStart::AccessEnrollment
    ));
    *fixture.mode.0.lock().unwrap() = SessionRegistrationMode::MachineTokens;
    assert!(matches!(
        registrar.prepare(&intent, &fixture.identity(&intent)).await,
        Err(RegistrarFailure::EphemeralListener)
    ));
    assert!(fixture.authority.events.lock().unwrap().is_empty());
    assert!(fixture.repository.records.lock().unwrap().is_empty());
}

#[tokio::test]
async fn fleet_session_registrar_persists_actual_identity_before_registration_and_holds_binding_until_granted()
 {
    let fixture = Fixture::new();
    let intent = fixture.intent();
    let PrepareSessionStart::Prepared(record) = fixture
        .registrar
        .prepare(&intent, &fixture.identity(&intent))
        .await
        .unwrap()
    else {
        panic!("new preparation missing");
    };
    let address = fixture
        .registrar
        .mark_dispatched(&intent.operation_id)
        .await
        .unwrap();
    assert_eq!(address, record.preparation().binding_address);
    let session = fixture.session();
    let registering = fixture
        .registrar
        .created(&intent.operation_id, session.clone());
    tokio::pin!(registering);
    assert!(registering.as_mut().now_or_never().is_none());
    assert_eq!(
        *fixture.authority.events.lock().unwrap(),
        [
            "startChecked",
            "preparedStored",
            "lifecycleStored",
            "lifecycleStored",
            "registrationSent"
        ]
    );
    assert!(matches!(
        fixture.repository.records.lock().unwrap()[0].lifecycle(),
        FleetSessionBindingLifecycle::Prepared {
            creation: PreparedSessionCreation::Created { .. }
        }
    ));
    assert!(matches!(
        fixture.registrar.resolve_binding(&address).await,
        Err(RegistrarFailure::InactiveBinding)
    ));
    fixture.grant();
    let RegistrationCompletion::Registered(bound) = registering.await.unwrap() else {
        panic!("confirmed registration missing");
    };
    assert_eq!(bound.session(), &session);
    bound.require_sender(&session).unwrap();
    let mut forged = session.clone();
    forged.session_id = "different-conversation".to_owned().try_into().unwrap();
    assert_eq!(
        bound.require_sender(&forged),
        Err(RegistrarFailure::SenderMismatch)
    );
    assert_eq!(
        fixture
            .registrar
            .binding_for_restore(&session)
            .await
            .unwrap(),
        address
    );
    fixture
        .registrar
        .end(&session, SessionEndCause::Archived)
        .await
        .unwrap();
    assert!(matches!(
        fixture.registrar.resolve_binding(&address).await,
        Err(RegistrarFailure::InactiveBinding)
    ));
    assert!(matches!(
        fixture.registrar.binding_for_restore(&session).await,
        Err(RegistrarFailure::InactiveBinding)
    ));
    assert_eq!(
        *fixture.authority.events.lock().unwrap().last().unwrap(),
        "endSent"
    );
}

#[tokio::test]
async fn fleet_session_registrar_ambiguous_registration_resends_identical_bytes_without_another_dispatch()
 {
    let fixture = Fixture::new();
    let intent = fixture.intent();
    fixture
        .registrar
        .prepare(&intent, &fixture.identity(&intent))
        .await
        .unwrap();
    let address = fixture
        .registrar
        .mark_dispatched(&intent.operation_id)
        .await
        .unwrap();
    fixture
        .answers
        .send(Err(FleetSessionConnectionFailure::MalformedReply))
        .unwrap();
    assert_eq!(
        fixture
            .registrar
            .created(&intent.operation_id, fixture.session())
            .await
            .unwrap(),
        RegistrationCompletion::Pending
    );
    assert!(matches!(
        fixture.registrar.resolve_binding(&address).await,
        Err(RegistrarFailure::InactiveBinding)
    ));
    assert!(matches!(
        fixture
            .registrar
            .prepare(&intent, &fixture.identity(&intent))
            .await
            .unwrap(),
        PrepareSessionStart::Existing(_)
    ));
    assert_eq!(
        fixture
            .registrar
            .mark_dispatched(&intent.operation_id)
            .await,
        Err(RegistrarFailure::InvalidTransition)
    );
    fixture.grant();
    assert!(matches!(
        fixture
            .registrar
            .reconcile_registration(&intent.operation_id)
            .await
            .unwrap(),
        RegistrationCompletion::Registered(_)
    ));
    {
        let sent = fixture.authority.registrations.lock().unwrap();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0], sent[1]);
    }
    let mut different = intent.clone();
    different.project_id = Some(
        "019f0000-0000-7000-8000-000000000099"
            .to_owned()
            .try_into()
            .unwrap(),
    );
    assert!(matches!(
        fixture
            .registrar
            .prepare(&different, &fixture.identity(&different))
            .await,
        Err(RegistrarFailure::OperationConflict)
    ));
}

#[tokio::test]
async fn fleet_session_registrar_created_then_refused_and_known_failed_creation_never_restore() {
    for created in [false, true] {
        let fixture = Fixture::new();
        let intent = fixture.intent();
        fixture
            .registrar
            .prepare(&intent, &fixture.identity(&intent))
            .await
            .unwrap();
        let address = fixture
            .registrar
            .mark_dispatched(&intent.operation_id)
            .await
            .unwrap();
        if created {
            fixture
                .answers
                .send(Ok(RegisterFleetSessionResult::Refused {
                    reason: fleet_identity_protocol::SessionRefusal::MachineNotAllowed {},
                }))
                .unwrap();
            assert!(matches!(
                fixture
                    .registrar
                    .created(&intent.operation_id, fixture.session())
                    .await
                    .unwrap(),
                RegistrationCompletion::Refused(_)
            ));
        } else {
            fixture
                .registrar
                .creation_refused(&intent.operation_id)
                .await
                .unwrap();
        }
        assert!(matches!(
            fixture.repository.records.lock().unwrap()[0].lifecycle(),
            FleetSessionBindingLifecycle::Abandoned { .. }
        ));
        assert!(matches!(
            fixture.registrar.resolve_binding(&address).await,
            Err(RegistrarFailure::InactiveBinding)
        ));
        assert!(matches!(
            fixture
                .registrar
                .binding_for_restore(&fixture.session())
                .await,
            Err(RegistrarFailure::InactiveBinding)
        ));
    }
}

#[tokio::test]
async fn fleet_session_registrar_origin_change_refuses_an_existing_registration_without_authority_io()
 {
    let fixture = Fixture::new();
    let intent = fixture.intent();
    fixture
        .registrar
        .prepare(&intent, &fixture.identity(&intent))
        .await
        .unwrap();
    fixture.authority.events.lock().unwrap().clear();
    let moved = Fixture::make_registrar(
        fixture.repository.clone(),
        fixture.authority.clone(),
        fixture.mode.clone(),
        "http://127.0.0.1:8789",
    );
    assert!(matches!(
        moved.prepare(&intent, &fixture.identity(&intent)).await,
        Err(RegistrarFailure::OriginChanged)
    ));
    assert!(fixture.authority.events.lock().unwrap().is_empty());
}
