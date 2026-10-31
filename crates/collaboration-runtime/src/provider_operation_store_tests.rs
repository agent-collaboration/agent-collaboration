use std::error::Error;

use super::ProviderOperationStore;

#[tokio::test]
async fn new_provider_operation_database_uses_incremental_auto_vacuum() -> Result<(), Box<dyn Error>>
{
    let directory = tempfile::tempdir()?;
    let database_path = directory.path().join("provider-operations.sqlite");
    let mut store = ProviderOperationStore::open(&database_path).await?;

    let mode: i64 = sqlx::query_scalar("PRAGMA auto_vacuum")
        .fetch_one(&mut store.connection)
        .await?;
    assert_eq!(mode, 2);
    store.close().await?;
    Ok(())
}
