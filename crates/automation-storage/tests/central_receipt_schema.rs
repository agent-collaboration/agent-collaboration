#![expect(
    clippy::panic_in_result_fn,
    reason = "Assertions reject wrong behavior; Result propagates real fixture and storage failures."
)]
//! Central custody survives reopen in the same store as its owed local effects.
use automation_storage::AutomationStore;
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

#[tokio::test]
async fn central_custody_schema_survives_reopen() -> Result<(), Box<dyn std::error::Error>> {
    // Arrange: a real, newly migrated store; no central or provider stand-in.
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    AutomationStore::open(&path).await?.close().await?;
    // Act: reopen through the production schema validator.
    AutomationStore::open(&path).await?.close().await?;
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&path)
            .foreign_keys(true),
    )
    .await?;
    // Assert: R7/R10 require durable receipt and pending reports, not process memory.
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='table' AND name IN ('firing_receipts','central_report_outbox') ORDER BY name",
    ).fetch_all(&mut connection).await?;
    assert_eq!(tables, ["central_report_outbox", "firing_receipts"]);
    for table in ["schedule_definitions", "wakeup_definitions"] {
        let authority: Option<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info(?) WHERE name='authority'")
                .bind(table)
                .fetch_optional(&mut connection)
                .await?;
        assert_eq!(authority.as_deref(), Some("authority"));
    }
    connection.close().await?;
    Ok(())
}
