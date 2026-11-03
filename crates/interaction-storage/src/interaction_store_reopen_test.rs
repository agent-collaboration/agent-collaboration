use super::*;

#[tokio::test]
async fn current_delivery_schema_reopens_after_its_owner_closes()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("interaction.sqlite");
    // Arrange: the public owner applies its real current library migrations.
    let store = InteractionStore::open(&path).await?;
    store.close().await?;
    // Act: cold reopen runs the same non-creating admission as a restarted Router.
    let reopened = InteractionStore::open(&path).await?;
    reopened.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A forged library index must fail schema admission while fixture errors propagate."
)]
#[tokio::test]
async fn altered_delivery_indexes_are_refused_without_rewriting_them()
-> Result<(), Box<dyn std::error::Error>> {
    use sqlx::Connection;
    use sqlx_turso::{TursoConnectOptions, TursoConnection};
    for (index, drop_index, altered_index) in [
        (
            "_fleet_sync_inbox_accounting",
            "DROP INDEX _fleet_sync_inbox_accounting",
            "CREATE INDEX _fleet_sync_inbox_accounting ON _fleet_sync_inbox_items(item_id)",
        ),
        (
            "_fleet_sync_status_owed_machine",
            "DROP INDEX _fleet_sync_status_owed_machine",
            "CREATE INDEX _fleet_sync_status_owed_machine ON _fleet_sync_status_owed(item_id)",
        ),
    ] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("interaction.sqlite");
        let store = InteractionStore::open(&path).await?;
        store.close().await?;
        let mut connection =
            TursoConnection::connect_with(&TursoConnectOptions::new().filename(&path)).await?;
        sqlx::query(drop_index).execute(&mut connection).await?;
        sqlx::query(altered_index).execute(&mut connection).await?;
        connection.close().await?;
        assert!(
            matches!(
                InteractionStore::open(&path).await,
                Err(StorageError::InvalidSchema)
            ),
            "altered index {index} was admitted"
        );
        let mut observer = TursoConnection::connect_with(
            &TursoConnectOptions::new().filename(&path).read_only(true),
        )
        .await?;
        let retained: String = sqlx::query_scalar("SELECT sql FROM sqlite_schema WHERE name=?")
            .bind(index)
            .fetch_one(&mut observer)
            .await?;
        assert!(
            retained.contains("(item_id)"),
            "schema rejection rewrote the forged index"
        );
        observer.close().await?;
    }
    Ok(())
}
