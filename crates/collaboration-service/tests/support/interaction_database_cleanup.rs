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
#[path = "interaction_database_cleanup_test.rs"]
mod tests;
