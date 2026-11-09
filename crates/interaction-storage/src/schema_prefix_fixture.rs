//! Build a real preceding native schema without leaving later migration holes.
use sqlx::Connection;
use sqlx_turso::{TursoConnectOptions, TursoConnection};
use std::path::Path;

/// The durable-steps journal a predecessor Router left in its interaction store.
#[derive(Clone, Copy)]
pub(crate) enum PrecedingJournal {
    /// The pinned library's own journal migration.
    Pinned,
    /// agent-fleet-sync b9da4b5 (Router 0.1.125): journal 0001, standard receipts.
    MarkerV1,
    /// agent-fleet-sync 0b27429 (Routers 0.1.126 to 0.1.140): journal 0001 and
    /// 0002, then that library's receipt metadata.
    ExtendedReceipts,
}

pub(crate) async fn create_native_prefix(path: &Path, last_native_version: i64) {
    create_native_prefix_with_journal(path, last_native_version, PrecedingJournal::Pinned).await;
}

/// A predecessor's file: the native prefix and the journal shape that Router's library left.
/// The released journals run the pinned library's own 0001 and 0002, which are
/// byte-identical to b9da4b5's and 0b27429's.
pub(crate) async fn create_native_prefix_with_journal(
    path: &Path,
    last_native_version: i64,
    journal: PrecedingJournal,
) {
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
    let released = match journal {
        PrecedingJournal::Pinned => None,
        PrecedingJournal::MarkerV1 => Some(1),
        PrecedingJournal::ExtendedReceipts => Some(2),
    };
    match released {
        None => durable_steps::turso_store::migrate_journal(&mut transaction)
            .await
            .unwrap(),
        Some(count) => {
            let mut released = sqlx::migrate::Migrator::with_migrations(
                durable_steps::turso_store::journal_migrations()
                    .take(count)
                    .cloned()
                    .collect::<Vec<_>>(),
            );
            released.dangerous_set_table_name("_fleet_sync_migrations");
            released
                .run_direct(None, &mut *transaction, false)
                .await
                .unwrap();
        }
    }
    if matches!(journal, PrecedingJournal::ExtendedReceipts) {
        sqlx::raw_sql(include_str!("released_receipt_metadata_0b27429.sql"))
            .execute(&mut *transaction)
            .await
            .unwrap();
    }
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
