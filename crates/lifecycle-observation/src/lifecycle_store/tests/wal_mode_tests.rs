use sqlx::{
    Connection, SqliteConnection,
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
};

use super::{ObservationJournal, identity};

#[tokio::test]
async fn existing_delete_mode_journal_reopens_in_wal() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let database_path = directory.path().join("session-registry.sqlite");
    let journal = ObservationJournal::open(&database_path, identity())
        .await
        .expect("new journal database should open");
    journal
        .connection
        .close()
        .await
        .expect("fresh journal connection should close");

    let mut legacy = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&database_path)
            .journal_mode(SqliteJournalMode::Delete),
    )
    .await
    .expect("existing journal database should open in DELETE mode");
    let legacy_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut legacy)
        .await
        .expect("legacy journal mode should be readable");
    legacy
        .close()
        .await
        .expect("legacy connection should close");
    assert_eq!(legacy_mode, "delete");

    let mut reopened = ObservationJournal::open(&database_path, identity())
        .await
        .expect("DELETE-mode journal database should reopen");
    let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut reopened.connection)
        .await
        .expect("reopened journal mode should be readable");
    let auto_vacuum: i64 = sqlx::query_scalar("PRAGMA auto_vacuum")
        .fetch_one(&mut reopened.connection)
        .await
        .expect("reopened auto-vacuum mode should be readable");

    assert_eq!(journal_mode, "wal");
    assert_eq!(auto_vacuum, 2);
    reopened
        .connection
        .close()
        .await
        .expect("reopened journal connection should close");
}
