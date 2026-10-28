//! Cleanup of the interaction database in an already-stopped native API fixture.
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};
use sqlx::{Connection, SqliteConnection};
use std::{path::Path, time::Duration};

pub(crate) async fn remove_closed_interaction_database(
    database_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // A connect-time journal-mode error drops SQLx's worker without joining it.
    // Own the connection first so even a refused transition finishes shutdown.
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(database_path)
            .busy_timeout(Duration::from_millis(250)),
    )
    .await?;
    let journal_mode = sqlx::query_scalar::<_, String>("PRAGMA journal_mode=DELETE")
        .fetch_one(&mut connection)
        .await;
    let connection_closed = connection.close().await;
    let journal_mode = journal_mode?;
    connection_closed?;
    if !journal_mode.eq_ignore_ascii_case("delete") {
        return Err(std::io::Error::other(format!(
            "interaction cleanup refused journal mode {journal_mode:?}; expected DELETE"
        ))
        .into());
    }
    std::fs::remove_file(database_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cleanup_refuses_live_wal_connection_without_unlinking_database() {
        // Arrange: keep a genuine WAL owner alive, forcing the old cleanup race.
        let directory = tempfile::tempdir().expect("owned cleanup directory");
        let database_path = directory.path().join("interaction.sqlite");
        let mut writer = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(&database_path)
                .create_if_missing(true)
                .journal_mode(SqliteJournalMode::Wal),
        )
        .await
        .expect("open WAL owner");
        sqlx::query("CREATE TABLE cleanup_probe (record_value TEXT NOT NULL)")
            .execute(&mut writer)
            .await
            .expect("create real WAL state");
        assert!(
            database_path
                .with_file_name("interaction.sqlite-wal")
                .exists()
        );

        // Act: an unclosed owner must prevent teardown, never permit unlinking.
        let cleanup = remove_closed_interaction_database(&database_path).await;
        let database_preserved = database_path.exists();
        writer.close().await.expect("join original SQLite worker");
        if database_preserved {
            remove_closed_interaction_database(&database_path)
                .await
                .expect("cleanup after owner closes");
        }
        let remaining_files = std::fs::read_dir(directory.path())
            .expect("cleanup observation")
            .count();
        directory.close().expect("empty fixture cleanup");

        // Assert after joined cleanup so a failed oracle does not leak workers.
        let refusal = cleanup.expect_err("cleanup accepted a live WAL owner");
        let sqlite_error = refusal
            .downcast_ref::<sqlx::Error>()
            .and_then(sqlx::Error::as_database_error)
            .expect("live WAL owner must refuse the SQLite transition");
        assert_eq!(sqlite_error.code().as_deref(), Some("5"));
        assert!(database_preserved, "cleanup unlinked the live database");
        assert_eq!(remaining_files, 0, "SQLite sidecars survived cleanup");
    }

    #[tokio::test]
    async fn cleanup_after_owner_drop_leaves_no_database_or_wal_sidecars() {
        // Arrange: the same real WAL owner that the broker releases on stop.
        let directory = tempfile::tempdir().expect("owned cleanup directory");
        let database_path = directory.path().join("interaction.sqlite");
        let mut writer = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(&database_path)
                .create_if_missing(true)
                .journal_mode(SqliteJournalMode::Wal),
        )
        .await
        .expect("open WAL owner");
        sqlx::query("CREATE TABLE cleanup_probe (record_value TEXT NOT NULL)")
            .execute(&mut writer)
            .await
            .expect("create WAL state");
        drop(writer);

        // Act: cleanup must finish the SQLite transition before file removal.
        remove_closed_interaction_database(&database_path)
            .await
            .expect("closed-owner cleanup");
        let remaining_files = std::fs::read_dir(directory.path())
            .expect("cleanup observation")
            .count();
        directory.close().expect("fixture cleanup");

        // Assert a literal empty directory, not output derived from the helper.
        assert_eq!(remaining_files, 0, "SQLite sidecars survived cleanup");
    }
}
