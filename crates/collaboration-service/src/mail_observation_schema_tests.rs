//! Construct genuine migration prefixes; deleting an ALTER is not its inverse DDL.
use interaction_storage::{InteractionStore, StorageError};
use sqlx::{Connection, migrate::Migrator};
use sqlx_turso::{TursoConnectOptions, TursoConnection};

async fn seed_prefix(last_version: i64) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    let migrations = Migrator::new(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../interaction-storage/migrations")
            .as_path(),
    )
    .await
    .unwrap();
    let prefix = Migrator::with_migrations(
        migrations
            .iter()
            .filter(|migration| migration.version <= last_version)
            .cloned()
            .collect(),
    );
    let mut connection = TursoConnection::connect_with(
        &TursoConnectOptions::new()
            .filename(directory.path().join("interaction.sqlite"))
            .create_if_missing(true),
    )
    .await
    .unwrap();
    let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await.unwrap();
    prefix
        .run_direct(None, &mut *transaction, false)
        .await
        .unwrap();
    sqlx::query("INSERT INTO interaction_history_revision(metadata_id,revision) VALUES(1,0)")
        .execute(&mut *transaction)
        .await
        .unwrap();
    durable_steps::turso_store::migrate_journal(&mut transaction)
        .await
        .unwrap();
    let mut delivery = Migrator::with_migrations(
        fleet_sync_delivery::DELIVERY_MIGRATOR
            .iter()
            .cloned()
            .collect(),
    );
    delivery.dangerous_set_table_name("_fleet_sync_delivery_migrations");
    delivery
        .run_direct(None, &mut *transaction, false)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    connection.close().await.unwrap();
    directory
}

async fn change_schema(directory: &tempfile::TempDir, statement: &'static str) {
    let mut observer = TursoConnection::connect_with(
        &TursoConnectOptions::new().filename(directory.path().join("interaction.sqlite")),
    )
    .await
    .unwrap();
    sqlx::raw_sql(statement)
        .execute(&mut observer)
        .await
        .unwrap();
    observer.close().await.unwrap();
}

pub(super) async fn assert_historical_prefixes_and_exact_ledger() {
    // Original CREATE syntax is preserved at each genuine prefix, before ADD COLUMN.
    for version in [202610190002, 202610190003, 202610200001] {
        let directory = seed_prefix(version).await;
        let path = directory.path().join("interaction.sqlite");
        let owner = InteractionStore::open(&path).await.unwrap();
        let push = uuid::Uuid::now_v7().to_string().try_into().unwrap();
        assert_eq!(owner.local_delivery_retry_attempts(&push).await.unwrap(), 0);
        owner.close().await.unwrap();
        InteractionStore::open(&path)
            .await
            .unwrap()
            .close()
            .await
            .unwrap();
    }
    for statement in [
        "DROP TABLE local_delivery_retries",
        "DELETE FROM _sqlx_migrations WHERE version=202610190003",
    ] {
        let directory = seed_prefix(202610190003).await;
        change_schema(&directory, statement).await;
        assert!(
            matches!(
                InteractionStore::open(directory.path().join("interaction.sqlite")).await,
                Err(StorageError::InvalidSchema)
            ),
            "retry ledger mismatch was admitted: {statement}"
        );
    }
}

#[tokio::test]
async fn observation_columns_and_ledger_must_agree_on_cold_reopen() {
    for statement in [
        "DELETE FROM _sqlx_migrations WHERE version=202610210001",
        "ALTER TABLE router_pushes DROP COLUMN last_outcome_at",
        "ALTER TABLE local_delivery_retries DROP COLUMN scheduled_retry_at",
    ] {
        // This genuine prefix predates the index that depends on scheduled_retry_at,
        // so each corruption still challenges the column/ledger rule itself.
        let directory = seed_prefix(202610210002).await;
        let path = directory.path().join("interaction.sqlite");
        change_schema(&directory, statement).await;
        assert!(
            matches!(
                InteractionStore::open(&path).await,
                Err(StorageError::InvalidSchema)
            ),
            "observation schema/ledger mismatch was admitted: {statement}"
        );
    }
}

#[tokio::test]
async fn provider_route_metadata_upgrades_the_genuine_prefix_and_refuses_mismatched_ledgers() {
    let directory = seed_prefix(202610210003).await;
    let path = directory.path().join("interaction.sqlite");
    InteractionStore::open(&path)
        .await
        .unwrap()
        .close()
        .await
        .unwrap();
    InteractionStore::open(&path)
        .await
        .unwrap()
        .close()
        .await
        .unwrap();
    for statement in [
        "ALTER TABLE router_pushes DROP COLUMN last_route_evidence_json",
        "DELETE FROM _sqlx_migrations WHERE version=202610210004",
    ] {
        let directory = seed_prefix(202610210003).await;
        let path = directory.path().join("interaction.sqlite");
        InteractionStore::open(&path)
            .await
            .unwrap()
            .close()
            .await
            .unwrap();
        change_schema(&directory, statement).await;
        assert!(matches!(
            InteractionStore::open(&path).await,
            Err(StorageError::InvalidSchema)
        ));
    }
    let directory = seed_prefix(202610210003).await;
    change_schema(
        &directory,
        "ALTER TABLE router_pushes ADD COLUMN last_route_evidence_json TEXT",
    )
    .await;
    assert!(matches!(
        InteractionStore::open(directory.path().join("interaction.sqlite")).await,
        Err(StorageError::InvalidSchema)
    ));
}
