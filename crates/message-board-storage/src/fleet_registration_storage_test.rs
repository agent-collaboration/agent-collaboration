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
    let preparation = preparation();
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
