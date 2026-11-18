use super::*;

#[tokio::test]
async fn machine_local_sqlite_holds_the_outbox_and_provider_operations_does_not() {
    let root = tempfile::tempdir().unwrap();
    let machine_local = root.path().join(MACHINE_LOCAL_DATABASE);

    let mut store = MachineLocalStore::open(&machine_local).await.unwrap();
    let tables: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .fetch_all(&mut store.connection)
            .await
            .unwrap();
    store.close().await.unwrap();
    let reopened = MachineLocalStore::open(&machine_local).await.unwrap();
    reopened.close().await.unwrap();
    let mut provider =
        crate::ProviderOperationStore::open(&root.path().join("provider-operations.sqlite"))
            .await
            .unwrap();
    let provider_outbox: Option<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'outbox_items'",
    )
    .fetch_optional(&mut provider.connection)
    .await
    .unwrap();

    assert!(tables.contains(&"outbox_items".to_owned()), "{tables:?}");
    assert_eq!(
        provider_outbox, None,
        "provider operations hold no link state"
    );
}
