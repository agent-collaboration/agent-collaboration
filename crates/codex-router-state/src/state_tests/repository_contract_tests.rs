use super::*;
use sqlx::Connection as _;

#[tokio::test]
async fn quota_snapshot_upsert_keeps_code_review_out_of_selector_projection() {
    let temp_dir = TestTempDir::new("code_review_status_only");
    let database_path = temp_dir.path().join("state.sqlite");
    let store = match AsyncSqliteStateStore::open(&database_path).await {
        Ok(store) => store,
        Err(error) => panic!("state store should open and migrate: {error}"),
    };
    let account_id = account_id("acct_code_review_status_only");
    let account = AccountRecord::new(
        codex_router_core::provider::Provider::Openai,
        account_id.clone(),
        "status-only",
        AccountStatus::Enabled,
    )
    .with_active_credential_generation(1);
    if let Err(error) = store.upsert_account(&account).await {
        panic!("account should persist: {error}");
    }
    let snapshot =
        PersistedQuotaSnapshot::new(account_id.clone(), QuotaSnapshotSource::MockEndpoint)
            .with_observed_unix_seconds(3_000)
            .with_route_band("code_review", 88)
            .with_reset_unix_seconds(4_000)
            .with_stale_penalty(false);

    if let Err(error) = store.upsert_quota_snapshot(&snapshot).await {
        panic!("code_review quota snapshot should persist: {error}");
    }

    assert_eq!(
        store
            .load_quota_snapshot_for_route_band(&account_id, "code_review")
            .await,
        Ok(Some(snapshot))
    );
    let selector_inputs = match store
        .selector_inputs_for_route_band("code_review", 3_000)
        .await
    {
        Ok(inputs) => inputs,
        Err(error) => panic!("code_review selector input should load: {error}"),
    };
    assert_eq!(selector_inputs.len(), 1);
    assert!(selector_inputs[0].windows().is_empty());

    store.close().await.expect("state store should close");
}

#[tokio::test]
async fn corrupt_account_metadata_fails_closed_for_that_account_only() {
    let temp_dir = TestTempDir::new("corrupt_account");
    let database_path = temp_dir.path().join("state.sqlite");
    let store = match AsyncSqliteStateStore::open(&database_path).await {
        Ok(store) => store,
        Err(error) => panic!("state store should open and migrate: {error}"),
    };
    let healthy_id = match AccountId::new("acct_healthy") {
        Ok(account_id) => account_id,
        Err(error) => panic!("account id should parse: {error}"),
    };
    let corrupt_id = match AccountId::new("acct_corrupt") {
        Ok(account_id) => account_id,
        Err(error) => panic!("account id should parse: {error}"),
    };

    if let Err(error) = store
        .upsert_account(&AccountRecord::new(
            codex_router_core::provider::Provider::Openai,
            healthy_id.clone(),
            "healthy",
            AccountStatus::Enabled,
        ))
        .await
    {
        panic!("healthy account should persist: {error}");
    }
    sqlx::query(
        "INSERT INTO accounts (account_id,label,status,provider) VALUES (?1,?2,?3,'openai')",
    )
    .bind(corrupt_id.as_str())
    .bind("refresh-token-canary")
    .bind("not-a-status")
    .execute(&store.pool)
    .await
    .expect("corrupt fixture should persist");

    let corrupt_error = match store.load_account(&corrupt_id).await {
        Ok(account) => panic!("corrupt account should not load: {account:?}"),
        Err(error) => error,
    };

    assert!(matches!(
        corrupt_error,
        StateStoreError::CorruptAccount { .. }
    ));
    assert!(!format!("{corrupt_error:?}").contains("refresh-token-canary"));
    assert_eq!(
        store.load_account(&healthy_id).await,
        Ok(Some(AccountRecord::new(
            codex_router_core::provider::Provider::Openai,
            healthy_id,
            "healthy",
            AccountStatus::Enabled
        )))
    );

    store.close().await.expect("state store should close");
}

#[tokio::test]
async fn unsupported_schema_version_fails_closed_on_open() {
    let temp_dir = TestTempDir::new("unsupported_schema");
    let database_path = temp_dir.path().join("state.sqlite");
    let mut raw = match sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true),
    )
    .await
    {
        Ok(raw) => raw,
        Err(error) => panic!("raw sqlite should open: {error}"),
    };
    if let Err(error) = sqlx::query("PRAGMA user_version = 999")
        .execute(&mut raw)
        .await
    {
        panic!("schema fixture should persist: {error}");
    }
    raw.close().await.expect("unsupported fixture should close");

    let error = match AsyncSqliteStateStore::open(&database_path).await {
        Ok(store) => panic!("unsupported schema should not open: {store:?}"),
        Err(error) => error,
    };

    assert_eq!(
        error,
        StateStoreError::UnsupportedSchemaVersion { version: 999 }
    );
}

#[tokio::test]
async fn async_state_and_legacy_pin_rows_roundtrip() {
    let temp_dir = TestTempDir::new("repository_contracts");
    let database_path = temp_dir.path().join("state.sqlite");
    let store = match AsyncSqliteStateStore::open(&database_path).await {
        Ok(store) => store,
        Err(error) => panic!("state store should open and migrate: {error}"),
    };
    let account_id = account_id("acct_contract");
    let account = AccountRecord::new(
        codex_router_core::provider::Provider::Openai,
        account_id.clone(),
        "contract",
        AccountStatus::Enabled,
    );
    let snapshot =
        PersistedQuotaSnapshot::new(account_id.clone(), QuotaSnapshotSource::MockEndpoint)
            .with_observed_unix_seconds(1_200)
            .with_route_band("responses", 33);
    let affinity_key = AffinityKey::new("response_previous");

    if let Err(error) = store.upsert_account(&account).await {
        panic!("account repository should persist: {error}");
    }
    if let Err(error) = store.upsert_quota_snapshot(&snapshot).await {
        panic!("quota repository should persist: {error}");
    }
    sqlx::query("INSERT INTO affinity_pins (affinity_key,account_id) VALUES (?1,?2)")
        .bind(affinity_key.as_str())
        .bind(account_id.as_str())
        .execute(&store.pool)
        .await
        .expect("legacy affinity pin should persist");

    assert_eq!(store.load_account(&account_id).await, Ok(Some(account)));
    assert_eq!(
        store
            .load_quota_snapshot_for_route_band(&account_id, "responses")
            .await,
        Ok(Some(snapshot))
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT account_id FROM affinity_pins WHERE affinity_key = ?1"
        )
        .bind(affinity_key.as_str())
        .fetch_optional(&store.pool)
        .await
        .expect("legacy affinity pin should load"),
        Some(account_id.as_str().to_owned())
    );

    store.close().await.expect("state store should close");
}
