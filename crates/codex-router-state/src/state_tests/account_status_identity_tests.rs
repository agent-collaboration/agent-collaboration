use super::*;

#[tokio::test]
async fn account_status_id_selector_keeps_the_captured_account_after_label_rebinding() {
    let temp_dir = TestTempDir::new("account_status_id_rebinding");
    let database_path = temp_dir.path().join("state.sqlite");
    let store = AsyncSqliteStateStore::open(&database_path).await.unwrap();
    let captured_id = account_id("captured");
    let neighbor_id = account_id("neighbor");
    store
        .upsert_account(&AccountRecord::new(
            Provider::Openai,
            captured_id.clone(),
            "captured",
            AccountStatus::Enabled,
        ))
        .await
        .unwrap();
    // The dialog already captured the ID. Its former label now names another account.
    store
        .upsert_account(&AccountRecord::new(
            Provider::Openai,
            captured_id.clone(),
            "renamed",
            AccountStatus::Enabled,
        ))
        .await
        .unwrap();
    store
        .upsert_account(&AccountRecord::new(
            Provider::Openai,
            neighbor_id.clone(),
            "captured",
            AccountStatus::Enabled,
        ))
        .await
        .unwrap();
    let writer = AsyncWeeklyQuotaFloorMutationStore::open(&database_path)
        .await
        .unwrap();
    for status in [AccountStatus::Disabled, AccountStatus::Enabled] {
        writer
            .set_account_status_by_account_id(&captured_id, status)
            .await
            .unwrap();
        let accounts = store.list_accounts().await.unwrap();
        assert_eq!(
            accounts
                .iter()
                .find(|account| account.account_id() == &captured_id)
                .unwrap()
                .status(),
            status,
            "the captured ID, independent of its renamed label"
        );
        assert_eq!(
            accounts
                .iter()
                .find(|account| account.account_id() == &neighbor_id)
                .unwrap()
                .status(),
            AccountStatus::Enabled,
            "the label's new owner must never be changed"
        );
    }
    let before = store.list_accounts().await.unwrap();
    assert!(matches!(
        writer
            .set_account_status_by_account_id(&account_id("absent"), AccountStatus::Disabled)
            .await,
        Err(StateStoreError::AccountStatusAccountNotFound)
    ));
    assert_eq!(store.list_accounts().await.unwrap(), before);
    writer.close().await;
    store.close().await.unwrap();
}
