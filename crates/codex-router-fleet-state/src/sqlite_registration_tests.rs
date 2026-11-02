//! S1 real SQLite floor: full envelope, projection validation, unique identities,
//! atomic expected-value moves and reopen. Central/harness recovery is separate.
//! No pre-fix semantic red observed for this newly supplied backend.
use super::tests::confirmed;
use super::*;
use crate::{
    FleetStateOpenError, FleetStateStore,
    session_registration_repository::{
        FleetSessionRegistrationRepository, FleetSessionStorageFailure, RegistrationInsert,
    },
};
use std::os::unix::fs::PermissionsExt;

fn root() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}
fn record(ordinal: u16) -> FleetSessionRegistrationRecord {
    let mut preparation = super::tests::preparation();
    preparation.registration_id = format!("019f0000-0000-7000-8000-{ordinal:012x}")
        .try_into()
        .unwrap();
    preparation.operation_id = format!(
        "019f0000-0000-7000-8000-{:012x}",
        u32::from(ordinal) + 0x100
    )
    .try_into()
    .unwrap();
    preparation.binding_address = format!("http://127.0.0.1:8788/mcp/session/{ordinal:032x}")
        .try_into()
        .unwrap();
    FleetSessionRegistrationRecord::new(
        preparation,
        FleetSessionBindingLifecycle::Prepared {
            creation: PreparedSessionCreation::NotDispatched {},
        },
    )
    .unwrap()
}
fn active(record: &FleetSessionRegistrationRecord) -> FleetSessionRegistrationRecord {
    let mut confirmed = confirmed(record.preparation());
    confirmed.registration.session.session_id =
        format!("provider-{}", record.preparation().registration_id.as_str())
            .try_into()
            .unwrap();
    record
        .with_lifecycle(FleetSessionBindingLifecycle::Active { confirmed })
        .unwrap()
}

#[tokio::test]
async fn fleet_sqlite_registration_reopens_actual_identity_and_whole_preparation_through_all_indexes()
 {
    let root = root();
    let store = FleetStateStore::open_and_migrate(root.path())
        .await
        .unwrap();
    let original = record(1);
    assert!(matches!(
        store.insert(&original).await.unwrap(),
        RegistrationInsert::Inserted
    ));
    let session = active(&original).lifecycle().session().unwrap().clone();
    let pending = original
        .with_lifecycle(FleetSessionBindingLifecycle::Prepared {
            creation: PreparedSessionCreation::CreatedPendingSetup {
                session: session.clone(),
            },
        })
        .unwrap();
    assert!(store.replace(&original, &pending).await.unwrap());
    store.close().await;

    let reopened = FleetStateStore::open_and_migrate(root.path())
        .await
        .unwrap();
    assert_eq!(
        reopened
            .operation(&original.preparation().operation_id)
            .await
            .unwrap(),
        Some(pending.clone())
    );
    assert_eq!(
        reopened
            .binding(&original.preparation().binding_address)
            .await
            .unwrap(),
        Some(pending.clone())
    );
    assert_eq!(
        reopened.conversation(&session).await.unwrap(),
        Some(pending.clone())
    );
    let active = active(&original);
    assert!(reopened.replace(&pending, &active).await.unwrap());
    reopened.close().await;
    let read_only = FleetStateStore::open_read_only(root.path()).await.unwrap();
    assert_eq!(
        read_only
            .operation(&original.preparation().operation_id)
            .await
            .unwrap(),
        Some(active.clone())
    );
    assert_eq!(
        read_only.reconcile_candidates().await.unwrap(),
        vec![active]
    );
    read_only.close().await;
}

#[tokio::test]
async fn fleet_sqlite_registration_racing_expected_value_moves_consume_once_and_preserve_immutable_input()
 {
    let root = root();
    let store = FleetStateStore::open_and_migrate(root.path())
        .await
        .unwrap();
    let original = record(1);
    store.insert(&original).await.unwrap();
    let dispatched = original
        .with_lifecycle(FleetSessionBindingLifecycle::Prepared {
            creation: PreparedSessionCreation::Dispatched {},
        })
        .unwrap();
    let abandoned = original
        .with_lifecycle(FleetSessionBindingLifecycle::Abandoned {
            creation: AbandonedSessionCreation::NotCreated {},
        })
        .unwrap();

    let (left, right) = tokio::join!(
        store.replace(&original, &dispatched),
        store.replace(&original, &abandoned)
    );
    assert_ne!(left.unwrap(), right.unwrap());
    let current = store
        .operation(&original.preparation().operation_id)
        .await
        .unwrap()
        .unwrap();
    assert!(current == dispatched || current == abandoned);
    assert!(!store.replace(&original, &original).await.unwrap());
    let mut changed = current.preparation().clone();
    changed.operation_id = record(9).preparation().operation_id.clone();
    let changed =
        FleetSessionRegistrationRecord::new(changed, current.lifecycle().clone()).unwrap();
    assert!(!store.replace(&current, &changed).await.unwrap());
    assert_eq!(
        store
            .operation(&original.preparation().operation_id)
            .await
            .unwrap(),
        Some(current)
    );
    store.close().await;
}

#[tokio::test]
async fn fleet_sqlite_registration_duplicate_operation_reuses_the_row_and_other_identities_refuse_atomically()
 {
    let root = root();
    let store = FleetStateStore::open_and_migrate(root.path())
        .await
        .unwrap();
    let original = record(1);
    store.insert(&original).await.unwrap();
    let mut replay = record(2).preparation().clone();
    replay.operation_id = original.preparation().operation_id.clone();
    let replay = FleetSessionRegistrationRecord::new(replay, original.lifecycle().clone()).unwrap();
    let RegistrationInsert::Existing(existing) = store.insert(&replay).await.unwrap() else {
        panic!("duplicate operation must retain original");
    };
    assert_eq!(*existing, original);
    for duplicate_binding in [true, false] {
        let mut duplicate = record(3).preparation().clone();
        if duplicate_binding {
            duplicate.binding_address = original.preparation().binding_address.clone();
        } else {
            duplicate.registration_id = original.preparation().registration_id.clone();
        }
        let duplicate =
            FleetSessionRegistrationRecord::new(duplicate, original.lifecycle().clone()).unwrap();
        assert!(matches!(
            store.insert(&duplicate).await,
            Err(FleetSessionStorageFailure::Conflict)
        ));
    }
    let first = active(&original);
    assert!(store.replace(&original, &first).await.unwrap());
    let second = record(2);
    store.insert(&second).await.unwrap();
    let second_same_session = second
        .with_lifecycle(FleetSessionBindingLifecycle::Prepared {
            creation: PreparedSessionCreation::CreatedPendingSetup {
                session: first.lifecycle().session().unwrap().clone(),
            },
        })
        .unwrap();
    assert_eq!(
        store
            .replace(&second, &second_same_session)
            .await
            .unwrap_err(),
        FleetSessionStorageFailure::Conflict
    );
    assert_eq!(
        store
            .operation(&second.preparation().operation_id)
            .await
            .unwrap(),
        Some(second)
    );
    assert_eq!(
        store
            .operation(&first.preparation().operation_id)
            .await
            .unwrap(),
        Some(first)
    );
    store.close().await;
}

#[tokio::test]
async fn fleet_sqlite_registration_refuses_every_mismatched_index_projection_and_invalid_envelope()
{
    for (update, value) in [
        (
            "UPDATE fleet_session_registrations SET registration_id=?1",
            "019f0000-0000-7000-8000-000000000099",
        ),
        (
            "UPDATE fleet_session_registrations SET operation_id=?1",
            "019f0000-0000-7000-8000-000000000099",
        ),
        (
            "UPDATE fleet_session_registrations SET installation_id=?1",
            "019f0000-0000-7000-8000-000000000099",
        ),
        (
            "UPDATE fleet_session_registrations SET binding_address=?1",
            "http://127.0.0.1:8788/mcp/session/99999999999999999999999999999999",
        ),
        (
            "UPDATE fleet_session_registrations SET session_service_id=?1",
            "019f0000-0000-7000-8000-000000000099",
        ),
        (
            "UPDATE fleet_session_registrations SET session_endpoint_id=?1",
            "claude-local",
        ),
        (
            "UPDATE fleet_session_registrations SET session_id=?1",
            "unrecorded-conversation",
        ),
        (
            "UPDATE fleet_session_registrations SET lifecycle_json=?1",
            "{\"version\":\"2\"}",
        ),
    ] {
        let root = root();
        let store = FleetStateStore::open_and_migrate(root.path())
            .await
            .unwrap();
        let original = record(1);
        store.insert(&original).await.unwrap();
        sqlx::query(update)
            .bind(value)
            .execute(&store.pool)
            .await
            .unwrap();

        assert_eq!(
            store.reconcile_candidates().await.unwrap_err(),
            FleetSessionStorageFailure::InvalidRecord
        );

        store.close().await;
    }
}

#[tokio::test]
async fn fleet_sqlite_registration_reconciliation_excludes_retired_and_abandoned_without_deleting_them()
 {
    let root = root();
    let store = FleetStateStore::open_and_migrate(root.path())
        .await
        .unwrap();
    let prepared = record(1);
    let active = active(&record(2));
    let FleetSessionBindingLifecycle::Active { confirmed } =
        self::active(&record(3)).lifecycle().clone()
    else {
        panic!("active fixture");
    };
    let retired = record(3)
        .with_lifecycle(FleetSessionBindingLifecycle::Retired { confirmed })
        .unwrap();
    let abandoned = record(4)
        .with_lifecycle(FleetSessionBindingLifecycle::Abandoned {
            creation: AbandonedSessionCreation::NotCreated {},
        })
        .unwrap();
    for record in [&prepared, &active, &retired, &abandoned] {
        store.insert(record).await.unwrap();
    }

    assert_eq!(
        store.reconcile_candidates().await.unwrap(),
        vec![prepared, active]
    );
    assert_eq!(
        store
            .operation(&retired.preparation().operation_id)
            .await
            .unwrap(),
        Some(retired)
    );
    assert_eq!(
        store
            .operation(&abandoned.preparation().operation_id)
            .await
            .unwrap(),
        Some(abandoned)
    );
    store.close().await;
}

#[tokio::test]
async fn fleet_sqlite_registration_open_rejects_a_changed_unique_projection_schema() {
    let root = root();
    let store = FleetStateStore::open_and_migrate(root.path())
        .await
        .unwrap();
    sqlx::query("DROP INDEX fleet_session_registration_conversation")
        .execute(&store.pool)
        .await
        .unwrap();
    sqlx::query("CREATE UNIQUE INDEX fleet_session_registration_conversation ON fleet_session_registrations(session_id)").execute(&store.pool).await.unwrap();
    store.close().await;

    assert!(matches!(
        FleetStateStore::open_and_migrate(root.path()).await,
        Err(FleetStateOpenError::InvalidSchema)
    ));
    assert!(matches!(
        FleetStateStore::open_read_only(root.path()).await,
        Err(FleetStateOpenError::InvalidSchema)
    ));
}
