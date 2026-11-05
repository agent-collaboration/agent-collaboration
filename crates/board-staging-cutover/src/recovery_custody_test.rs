//! SQLite itself must refuse writes by the offline custody observer.
use super::*;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions follow closing the isolated database observer; errors propagate setup failures."
)]
async fn custody_connection_refuses_a_write_to_an_existing_database()
-> Result<(), Box<dyn std::error::Error>> {
    // Arrange a real existing store, then close its only writer.
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("custody.sqlite");
    let mut writer = TursoConnection::connect_with(
        &TursoConnectOptions::new()
            .filename(&path)
            .create_if_missing(true),
    )
    .await?;
    // Deliberately one probe cell, not a domain table: no records or identifiers are generated.
    sqlx::query("CREATE TABLE observer_probe (value INTEGER)")
        .execute(&mut writer)
        .await?;
    sqlx::query("INSERT INTO observer_probe VALUES (1)")
        .execute(&mut writer)
        .await?;
    writer.close().await?;
    // Act through the same connection constructor as semantic verification.
    let mut observer = RecoveryCustody::read_connection(&path).await?;
    let attempted = sqlx::query("UPDATE observer_probe SET value=2")
        .execute(&mut observer)
        .await;
    let actual: i64 = sqlx::query_scalar("SELECT value FROM observer_probe")
        .fetch_one(&mut observer)
        .await?;
    observer.close().await?;
    // Assert both refusal and preserved content; a writable observer breaks this.
    assert!(attempted.is_err(), "custody observer accepted a write");
    assert_eq!(actual, 1);
    Ok(())
}
