use super::{
    FleetCache,
    login_storage_tests::{FixedClock, FixedIds},
};
use crate::{
    fleet_session_registration::{
        tests::{confirmed, preparation, registration},
        *,
    },
    session_registration_repository::{
        FleetSessionRegistrationRepository, FleetSessionStorageFailure, RegistrationInsert,
    },
};
use collaboration_protocol::SessionSetupAbandonmentCause;
use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};

#[tokio::test]
async fn fleet_registration_typed_columns_keep_all_lifecycles_and_timestamps_after_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("fleet-cache.sqlite");
    let clock = Arc::new(FixedClock(AtomicI64::new(1000)));
    let cache = FleetCache::open_with_sources(&path, clock.clone(), Arc::new(FixedIds))
        .await
        .unwrap();
    let mut preparation = preparation();
    preparation.launch.launch_context.repository = Some(
        "https://repository.invalid/fixture"
            .to_owned()
            .try_into()
            .unwrap(),
    );
    preparation.launch.launch_context.commit = Some(
        "0123456789abcdef0123456789abcdef01234567"
            .to_owned()
            .try_into()
            .unwrap(),
    );
    preparation.launch.launch_context.model = Some("fixture-model".to_owned().try_into().unwrap());
    preparation.launch.machine_environment = fleet_identity_protocol::MachineEnvironment::CloudVm;
    preparation.launch.isolation.kind = fleet_identity_protocol::IsolationKind::Vm;
    let registration = registration(&preparation);
    let session = registration.session.clone();
    let mut current = FleetSessionRegistrationRecord::new(
        preparation.clone(),
        FleetSessionBindingLifecycle::Prepared {
            creation: PreparedSessionCreation::NotDispatched {},
        },
    )
    .unwrap();
    assert!(matches!(
        cache.insert(&current).await.unwrap(),
        RegistrationInsert::Inserted
    ));
    let lifecycles = [
        FleetSessionBindingLifecycle::Prepared {
            creation: PreparedSessionCreation::Dispatched {},
        },
        FleetSessionBindingLifecycle::Prepared {
            creation: PreparedSessionCreation::CreatedPendingSetup {
                session: session.clone(),
            },
        },
        FleetSessionBindingLifecycle::Prepared {
            creation: PreparedSessionCreation::Created {
                registration: Box::new(registration.clone()),
            },
        },
        FleetSessionBindingLifecycle::Active {
            confirmed: confirmed(&preparation),
        },
        FleetSessionBindingLifecycle::Retired {
            confirmed: confirmed(&preparation),
        },
        FleetSessionBindingLifecycle::Abandoned {
            creation: AbandonedSessionCreation::NotCreated {},
        },
        FleetSessionBindingLifecycle::Abandoned {
            creation: AbandonedSessionCreation::KnownCreated {
                session: session.clone(),
                cause: SessionSetupAbandonmentCause::SetupInterrupted {},
            },
        },
        FleetSessionBindingLifecycle::Abandoned {
            creation: AbandonedSessionCreation::RegistrationRefused {
                registration: Box::new(registration),
            },
        },
    ];
    for (index, lifecycle) in lifecycles.into_iter().enumerate() {
        let now = 2000 + i64::try_from(index).unwrap();
        clock.0.store(now, Ordering::SeqCst);
        let next = current.with_lifecycle(lifecycle).unwrap();
        assert!(cache.replace(&current, &next).await.unwrap());
        assert!(!cache.replace(&current, &next).await.unwrap());
        assert_eq!(
            cache.operation(&preparation.operation_id).await.unwrap(),
            Some(next.clone())
        );
        assert_eq!(
            cache.binding(&preparation.binding_address).await.unwrap(),
            Some(next.clone())
        );
        let timestamps: (i64, i64) =
            sqlx::query_as("SELECT created_at,updated_at FROM fleet_session_registrations")
                .fetch_one(&cache.database.pool)
                .await
                .unwrap();
        assert_eq!(timestamps, (1000, now));
        current = next;
    }
    cache.close().await;
    let reopened = FleetCache::open_with_sources(&path, clock, Arc::new(FixedIds))
        .await
        .unwrap();
    assert_eq!(
        reopened.operation(&preparation.operation_id).await.unwrap(),
        Some(current.clone())
    );
    assert_eq!(
        reopened.conversation(&session).await.unwrap(),
        Some(current)
    );
    assert!(reopened.reconcile_candidates().await.unwrap().is_empty());
    reopened.close().await;
}

#[tokio::test]
async fn fleet_registration_unused_payload_and_partial_triples_are_invalid_without_repair() {
    let directory = tempfile::tempdir().unwrap();
    let cache = FleetCache::open_with_sources(
        &directory.path().join("fleet-cache.sqlite"),
        Arc::new(FixedClock(AtomicI64::new(1000))),
        Arc::new(FixedIds),
    )
    .await
    .unwrap();
    let preparation = preparation();
    let record = FleetSessionRegistrationRecord::new(
        preparation.clone(),
        FleetSessionBindingLifecycle::Prepared {
            creation: PreparedSessionCreation::NotDispatched {},
        },
    )
    .unwrap();
    cache.insert(&record).await.unwrap();
    sqlx::query("UPDATE fleet_session_registrations SET session_id='unexplained-partial-session'")
        .execute(&cache.database.pool)
        .await
        .unwrap();
    assert!(matches!(
        cache.operation(&preparation.operation_id).await,
        Err(FleetSessionStorageFailure::InvalidRecord)
    ));
    assert!(matches!(
        cache.reconcile_candidates().await,
        Err(FleetSessionStorageFailure::InvalidRecord)
    ));
    let retained: String = sqlx::query_scalar("SELECT session_id FROM fleet_session_registrations")
        .fetch_one(&cache.database.pool)
        .await
        .unwrap();
    assert_eq!(retained, "unexplained-partial-session");
    cache.close().await;
}

#[tokio::test]
async fn fleet_registration_native_uniqueness_replays_the_operation_and_rolls_back_other_conflicts()
{
    let directory = tempfile::tempdir().unwrap();
    let cache = FleetCache::open_with_sources(
        &directory.path().join("fleet-cache.sqlite"),
        Arc::new(FixedClock(AtomicI64::new(1000))),
        Arc::new(FixedIds),
    )
    .await
    .unwrap();
    let initial = FleetSessionRegistrationRecord::new(
        preparation(),
        FleetSessionBindingLifecycle::Prepared {
            creation: PreparedSessionCreation::NotDispatched {},
        },
    )
    .unwrap();
    cache.insert(&initial).await.unwrap();
    let mut other = initial.preparation().clone();
    other.registration_id = "01900000-0000-7000-8000-000000000011"
        .to_owned()
        .try_into()
        .unwrap();
    other.binding_address = "http://127.0.0.1:8788/mcp/session/11111111111111111111111111111111"
        .to_owned()
        .try_into()
        .unwrap();
    let replay =
        FleetSessionRegistrationRecord::new(other.clone(), initial.lifecycle().clone()).unwrap();
    assert!(
        matches!(cache.insert(&replay).await.unwrap(),RegistrationInsert::Existing(record) if *record==initial)
    );
    other.operation_id = "01900000-0000-7000-8000-000000000012"
        .to_owned()
        .try_into()
        .unwrap();
    other.registration_id = initial.preparation().registration_id.clone();
    let key_conflict =
        FleetSessionRegistrationRecord::new(other.clone(), initial.lifecycle().clone()).unwrap();
    assert!(matches!(
        cache.insert(&key_conflict).await,
        Err(FleetSessionStorageFailure::Conflict)
    ));
    other.registration_id = "01900000-0000-7000-8000-000000000011"
        .to_owned()
        .try_into()
        .unwrap();
    other.binding_address = initial.preparation().binding_address.clone();
    let binding_conflict =
        FleetSessionRegistrationRecord::new(other.clone(), initial.lifecycle().clone()).unwrap();
    assert!(matches!(
        cache.insert(&binding_conflict).await,
        Err(FleetSessionStorageFailure::Conflict)
    ));
    let active = initial
        .with_lifecycle(FleetSessionBindingLifecycle::Active {
            confirmed: confirmed(initial.preparation()),
        })
        .unwrap();
    assert!(cache.replace(&initial, &active).await.unwrap());
    other.binding_address = "http://127.0.0.1:8788/mcp/session/11111111111111111111111111111111"
        .to_owned()
        .try_into()
        .unwrap();
    let conversation_conflict =
        FleetSessionRegistrationRecord::new(other, active.lifecycle().clone()).unwrap();
    assert!(matches!(
        cache.insert(&conversation_conflict).await,
        Err(FleetSessionStorageFailure::Conflict)
    ));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fleet_session_registrations")
        .fetch_one(&cache.database.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(
        cache
            .operation(&initial.preparation().operation_id)
            .await
            .unwrap(),
        Some(active)
    );
    cache.close().await;
}

#[tokio::test]
async fn fleet_registration_native_competing_transitions_have_one_winner_and_keep_preparation_immutable()
 {
    let directory = tempfile::tempdir().unwrap();
    let cache = FleetCache::open_with_sources(
        &directory.path().join("fleet-cache.sqlite"),
        Arc::new(FixedClock(AtomicI64::new(1000))),
        Arc::new(FixedIds),
    )
    .await
    .unwrap();
    let initial = FleetSessionRegistrationRecord::new(
        preparation(),
        FleetSessionBindingLifecycle::Prepared {
            creation: PreparedSessionCreation::NotDispatched {},
        },
    )
    .unwrap();
    cache.insert(&initial).await.unwrap();
    let dispatched = initial
        .with_lifecycle(FleetSessionBindingLifecycle::Prepared {
            creation: PreparedSessionCreation::Dispatched {},
        })
        .unwrap();
    let abandoned = initial
        .with_lifecycle(FleetSessionBindingLifecycle::Abandoned {
            creation: AbandonedSessionCreation::NotCreated {},
        })
        .unwrap();
    let (left, right) = tokio::join!(
        cache.replace(&initial, &dispatched),
        cache.replace(&initial, &abandoned)
    );
    let (left, right) = (left.unwrap(), right.unwrap());
    assert_ne!(left, right);
    let expected = if left { dispatched } else { abandoned };
    assert_eq!(
        cache
            .operation(&initial.preparation().operation_id)
            .await
            .unwrap(),
        Some(expected.clone())
    );
    let mut changed = expected.preparation().clone();
    changed.launch.project_id = "01900000-0000-7000-8000-000000000013"
        .to_owned()
        .try_into()
        .unwrap();
    let invalid_replacement =
        FleetSessionRegistrationRecord::new(changed, expected.lifecycle().clone()).unwrap();
    assert!(
        !cache
            .replace(&expected, &invalid_replacement)
            .await
            .unwrap()
    );
    assert_eq!(
        cache
            .operation(&initial.preparation().operation_id)
            .await
            .unwrap(),
        Some(expected)
    );
    cache.close().await;
}

#[tokio::test]
async fn fleet_registration_native_decode_rejects_each_invalid_typed_column_without_deleting_rows()
{
    let directory = tempfile::tempdir().unwrap();
    let text_columns = [
        "registration_id",
        "operation_id",
        "installation_id",
        "binding_address",
        "endpoint_service_id",
        "endpoint_id",
        "project_id",
        "checked_workspace_id",
        "started_by_kind",
        "started_by_service_id",
        "started_by_endpoint_id",
        "started_by_session_id",
        "remote_start_id",
        "isolation_kind",
        "isolation_instance_id",
        "working_directory",
        "repository",
        "commit_id",
        "model",
        "machine_environment",
        "lifecycle_state",
        "creation_state",
        "session_service_id",
        "session_endpoint_id",
        "session_id",
        "organization_id",
        "abandonment_cause",
        "refused_setting",
    ];
    for column in text_columns.into_iter().chain(["created_at", "updated_at"]) {
        let path = directory.path().join(format!("corrupt-{column}.sqlite"));
        let cache = FleetCache::open_with_sources(
            &path,
            Arc::new(FixedClock(AtomicI64::new(1000))),
            Arc::new(FixedIds),
        )
        .await
        .unwrap();
        let record = FleetSessionRegistrationRecord::new(
            preparation(),
            FleetSessionBindingLifecycle::Prepared {
                creation: PreparedSessionCreation::NotDispatched {},
            },
        )
        .unwrap();
        cache.insert(&record).await.unwrap();
        let sql = format!("UPDATE fleet_session_registrations SET {column}=?");
        if matches!(column, "created_at" | "updated_at") {
            sqlx::query(&sql)
                .bind(-1_i64)
                .execute(&cache.database.pool)
                .await
                .unwrap();
        } else {
            sqlx::query(&sql)
                .bind("")
                .execute(&cache.database.pool)
                .await
                .unwrap();
        }
        assert!(
            cache.reconcile_candidates().await.is_err(),
            "invalid {column} accepted"
        );
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fleet_session_registrations")
            .fetch_one(&cache.database.pool)
            .await
            .unwrap();
        assert_eq!(count, 1, "invalid {column} was silently removed");
        cache.close().await;
    }
}
