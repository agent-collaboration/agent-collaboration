//! Real SQLite is the repository contract's floor; no memory or clock stands in.
use super::*;
use crate::{
    fleet_login::{FleetLoginCandidate, SavedFleetCentral},
    fleet_login_repository::FleetLoginRepository,
};
use fleet_identity_protocol::MachineEnvironment;
use std::os::unix::fs::PermissionsExt;

fn private_root() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}

fn candidate(environment: Option<MachineEnvironment>) -> FleetLoginCandidate {
    FleetLoginCandidate {
        sign_in: "https://signin.example.test/issuer"
            .to_owned()
            .try_into()
            .unwrap(),
        organization_id: "019f0000-0000-7000-8000-000000000001"
            .to_owned()
            .try_into()
            .unwrap(),
        enrollment_id: "019f0000-0000-7000-8000-000000000002"
            .to_owned()
            .try_into()
            .unwrap(),
        requested_machine_environment: environment,
        central: SavedFleetCentral {
            receiver: "019f0000-0000-7000-8000-000000000003"
                .to_owned()
                .try_into()
                .unwrap(),
            relay: "019f0000-0000-7000-8000-000000000004"
                .to_owned()
                .try_into()
                .unwrap(),
        },
    }
}

#[tokio::test]
async fn fleet_sqlite_login_repository_honors_the_same_atomic_contract_as_the_stand_in() {
    let root = private_root();
    let store = FleetStateStore::open_and_migrate(root.path())
        .await
        .unwrap();

    crate::test_support::verify_login_repository_contract(&store)
        .await
        .unwrap();

    store.close().await;
}

#[tokio::test]
async fn fleet_sqlite_reopen_keeps_the_whole_confirmed_row_and_read_only_never_migrates() {
    let root = private_root();
    let store = FleetStateStore::open_and_migrate(root.path())
        .await
        .unwrap();
    let saved = store
        .replace_fleet_login(&candidate(Some(MachineEnvironment::CloudVm)))
        .await
        .unwrap();
    store.close().await;

    let reopened = FleetStateStore::open_read_only(root.path()).await.unwrap();
    assert_eq!(reopened.load_fleet_login().await.unwrap(), Some(saved));
    reopened.close().await;
    let before = std::fs::read(root.path().join(crate::FLEET_DATABASE_FILENAME)).unwrap();
    let writable = FleetStateStore::open_and_migrate(root.path())
        .await
        .unwrap();
    let retained = writable
        .replace_fleet_login(&candidate(None))
        .await
        .unwrap();
    assert_eq!(retained.machine_environment, MachineEnvironment::CloudVm);
    writable.close().await;
    assert!(!before.is_empty());
    let absent = private_root();
    assert!(
        FleetStateStore::open_read_only(absent.path())
            .await
            .is_err()
    );
    assert!(!absent.path().join(crate::FLEET_DATABASE_FILENAME).exists());
}

#[tokio::test]
async fn fleet_sqlite_owns_only_its_file_and_keeps_live_database_sidecars_private() {
    let root = private_root();
    let proxy = root.path().join("state.sqlite");
    let original = b"unrelated proxy state must remain byte-identical";
    std::fs::write(&proxy, original).unwrap();

    let store = FleetStateStore::open_and_migrate(root.path())
        .await
        .unwrap();
    store.replace_fleet_login(&candidate(None)).await.unwrap();

    for name in ["fleet.sqlite", "fleet.sqlite-wal", "fleet.sqlite-shm"] {
        let mode = std::fs::metadata(root.path().join(name))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "{name}");
    }
    assert_eq!(std::fs::read(&proxy).unwrap(), original);
    store.close().await;
    assert_eq!(std::fs::read(proxy).unwrap(), original);
}

#[tokio::test]
async fn fleet_sqlite_rejects_an_unregistered_database_before_creating_migration_history() {
    use sqlx::Connection;
    let root = private_root();
    let path = root.path().join(crate::FLEET_DATABASE_FILENAME);
    std::fs::File::create(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(&path);
    let mut connection = sqlx::SqliteConnection::connect_with(&options)
        .await
        .unwrap();
    sqlx::query("CREATE TABLE unrelated (payload TEXT NOT NULL)")
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();

    assert!(matches!(
        FleetStateStore::open_and_migrate(root.path()).await,
        Err(FleetStateOpenError::InvalidSchema)
    ));

    let mut connection = sqlx::SqliteConnection::connect_with(&options)
        .await
        .unwrap();
    let history: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_schema WHERE name='_sqlx_migrations'")
            .fetch_one(&mut connection)
            .await
            .unwrap();
    assert_eq!(history, 0);
    connection.close().await.unwrap();
}

#[tokio::test]
async fn fleet_sqlite_rejects_corrupt_stored_fields_without_coercing_or_overwriting_them() {
    use crate::fleet_login_repository::FleetLoginStorageFailure;
    for (column, value, update, read) in [
        (
            "enrollment_id",
            "invalid",
            "UPDATE fleet_login SET enrollment_id=?1",
            "SELECT enrollment_id FROM fleet_login",
        ),
        (
            "sign_in",
            "http://remote.invalid",
            "UPDATE fleet_login SET sign_in=?1",
            "SELECT sign_in FROM fleet_login",
        ),
        (
            "organization_id",
            "invalid",
            "UPDATE fleet_login SET organization_id=?1",
            "SELECT organization_id FROM fleet_login",
        ),
        (
            "machine_environment",
            "future",
            "UPDATE fleet_login SET machine_environment=?1",
            "SELECT machine_environment FROM fleet_login",
        ),
        (
            "receiver_id",
            "invalid",
            "UPDATE fleet_login SET receiver_id=?1",
            "SELECT receiver_id FROM fleet_login",
        ),
        (
            "relay_peer_id",
            "invalid",
            "UPDATE fleet_login SET relay_peer_id=?1",
            "SELECT relay_peer_id FROM fleet_login",
        ),
    ] {
        let root = private_root();
        let store = FleetStateStore::open_and_migrate(root.path())
            .await
            .unwrap();
        store.replace_fleet_login(&candidate(None)).await.unwrap();
        sqlx::query(update)
            .bind(value)
            .execute(&store.pool)
            .await
            .unwrap();

        assert_eq!(
            store.load_fleet_login().await.unwrap_err(),
            FleetLoginStorageFailure::InvalidRecord { field: column }
        );
        assert!(store.replace_fleet_login(&candidate(None)).await.is_err());

        let retained: String = sqlx::query_scalar(read)
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(retained, value);
        store.close().await;
    }
}

#[tokio::test]
async fn fleet_sqlite_refuses_public_roots_or_files_without_changing_their_permissions() {
    for public_root in [true, false] {
        let root = private_root();
        let path = if public_root {
            root.path().to_path_buf()
        } else {
            let path = root.path().join(crate::FLEET_DATABASE_FILENAME);
            std::fs::write(&path, b"").unwrap();
            path
        };
        let mode = if public_root { 0o755 } else { 0o644 };
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();

        assert!(matches!(
            FleetStateStore::open_and_migrate(root.path()).await,
            Err(FleetStateOpenError::Unavailable)
        ));

        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            mode
        );
    }
}
