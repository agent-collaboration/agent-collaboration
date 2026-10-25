//! Build a real preceding native schema without leaving later migration holes.
use sqlx::Connection;
use sqlx_turso::{TursoConnectOptions, TursoConnection};
use std::path::Path;

pub(crate) async fn create_native_prefix(path: &Path, last_native_version: i64) {
    let mut connection = TursoConnection::connect_with(
        &TursoConnectOptions::new()
            .filename(path)
            .create_if_missing(true),
    )
    .await
    .unwrap();
    let migrations = sqlx::migrate!("./migrations");
    let prefix = sqlx::migrate::Migrator::with_migrations(
        migrations
            .iter()
            .take_while(|migration| migration.version <= last_native_version)
            .cloned()
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        prefix.iter().last().map(|migration| migration.version),
        Some(last_native_version),
        "fixture must end at the requested native migration"
    );
    prefix
        .run_direct(None, &mut connection, false)
        .await
        .unwrap();
    sqlx::query("INSERT INTO interaction_history_revision(metadata_id,revision) VALUES(1,0)")
        .execute(&mut connection)
        .await
        .unwrap();
    let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await.unwrap();
    durable_steps::turso_store::migrate_journal(&mut transaction)
        .await
        .unwrap();
    let mut delivery = sqlx::migrate::Migrator::with_migrations(
        fleet_sync_delivery::DELIVERY_MIGRATOR
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
    );
    delivery.dangerous_set_table_name("_fleet_sync_delivery_migrations");
    delivery
        .run_direct(None, &mut *transaction, false)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    connection.close().await.unwrap();
}
