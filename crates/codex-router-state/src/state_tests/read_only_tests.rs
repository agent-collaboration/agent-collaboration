use super::*;
use sqlx::Connection as _;

#[tokio::test]
async fn async_read_only_store_reads_without_allowing_writes() {
    let temp_dir = TestTempDir::new("async_read_only_store");
    let database_path = temp_dir.path().join("state.sqlite");
    let writable_store = match AsyncSqliteStateStore::open(&database_path).await {
        Ok(store) => store,
        Err(error) => panic!("writable async state store should open and migrate: {error}"),
    };
    let account = AccountRecord::new(
        codex_router_core::provider::Provider::Openai,
        account_id("acct_async_read_only"),
        "read-only",
        AccountStatus::Enabled,
    );
    if let Err(error) = writable_store.upsert_account(&account).await {
        panic!("writable async store should persist account: {error}");
    }
    if let Err(error) = writable_store.close().await {
        panic!("writable async store should close cleanly: {error}");
    }
    drop(writable_store);

    let read_only_store = match AsyncSqliteStateStore::open_read_only(&database_path).await {
        Ok(store) => store,
        Err(error) => panic!("read-only async state store should open: {error}"),
    };
    let accounts = match read_only_store.list_accounts().await {
        Ok(accounts) => accounts,
        Err(error) => panic!("read-only async store should list accounts: {error}"),
    };
    assert_eq!(accounts, vec![account]);

    let write_error = read_only_store
        .upsert_account(&AccountRecord::new(
            codex_router_core::provider::Provider::Openai,
            account_id("acct_async_read_only_blocked"),
            "blocked",
            AccountStatus::Enabled,
        ))
        .await;
    assert!(
        write_error.is_err(),
        "read-only async store must reject writes"
    );
    read_only_store
        .close()
        .await
        .expect("read-only store should close");
}

#[tokio::test]
async fn async_read_only_store_reports_missing_current_schema_without_writes() {
    let temp_dir = TestTempDir::new("async_read_only_missing_schema");
    let database_path = temp_dir.path().join("state.sqlite");
    create_v10_database_missing_async_projection_tables(&database_path).await;
    let mut raw = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .foreign_keys(false),
    )
    .await
    .expect("fixture should reopen");
    sqlx::query("PRAGMA user_version = 13")
        .execute(&mut raw)
        .await
        .expect("fixture should claim current schema");
    raw.close().await.expect("fixture connection should close");

    let error = expect_error(
        AsyncSqliteStateStore::open_read_only(&database_path).await,
        "read-only open should fail before status queries hit missing tables",
    );

    assert_eq!(
        error,
        StateStoreError::MissingReadOnlySchemaObject {
            object_kind: "table",
            object_name: "quota_history_observations",
        }
    );
}

#[tokio::test]
async fn async_read_only_store_requires_both_credit_tables_in_native_schema() {
    for table_name in ["account_credit_policies", "account_credit_observations"] {
        let temp_dir = TestTempDir::new("async_read_only_missing_credit_schema");
        let database_path = temp_dir.path().join("state.sqlite");
        let store = AsyncSqliteStateStore::open(&database_path)
            .await
            .expect("current native state should initialize");
        store.close().await.expect("state should close");

        let mut raw = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&database_path)
                .create_if_missing(true)
                .foreign_keys(false),
        )
        .await
        .expect("native fixture should reopen");
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP TABLE {table_name};")))
            .execute(&mut raw)
            .await
            .expect("test should remove one required credit table");
        raw.close().await.expect("fixture connection should close");

        assert_eq!(
            AsyncSqliteStateStore::open_read_only(&database_path)
                .await
                .expect_err("read-only open must reject a missing credit table"),
            StateStoreError::MissingReadOnlySchemaObject {
                object_kind: "table",
                object_name: table_name,
            }
        );
    }

    let temp_dir = TestTempDir::new("async_read_only_missing_credit_column");
    let database_path = temp_dir.path().join("state.sqlite");
    let store = AsyncSqliteStateStore::open(&database_path)
        .await
        .expect("current native state should initialize");
    store.close().await.expect("state should close");
    let mut raw = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .foreign_keys(false),
    )
    .await
    .expect("native fixture should reopen");
    sqlx::raw_sql(
        "DROP TABLE account_credit_observations;
             CREATE TABLE account_credit_observations (
                 account_id TEXT PRIMARY KEY NOT NULL,
                 credential_generation INTEGER NOT NULL
             );",
    )
    .execute(&mut raw)
    .await
    .expect("test should replace the credit table with a missing column");
    raw.close().await.expect("fixture connection should close");

    assert_eq!(
        AsyncSqliteStateStore::open_read_only(&database_path)
            .await
            .expect_err("read-only open must reject a missing credit column"),
        StateStoreError::MissingReadOnlySchemaObject {
            object_kind: "column",
            object_name: "latest_started_attempt",
        }
    );
}

#[tokio::test]
async fn legacy_seeded_state_store_is_accepted_by_quota_status_read_only_schema() {
    let temp_dir = TestTempDir::new("sync_seeded_read_only_quota_status");
    let database_path = temp_dir.path().join("state.sqlite");
    let seed_store = match AsyncSqliteStateStore::open(&database_path).await {
        Ok(store) => store,
        Err(error) => panic!("seed state store should open and migrate: {error}"),
    };
    let account_id = account_id("acct_sync_seeded_quota_status");
    let account = AccountRecord::new(
        codex_router_core::provider::Provider::Openai,
        account_id.clone(),
        "legacy-seeded",
        AccountStatus::Enabled,
    )
    .with_active_credential_generation(1);
    if let Err(error) = seed_store.upsert_account(&account).await {
        panic!("legacy seed should persist account: {error}");
    }
    let short_window = PersistedSelectorQuotaWindow::new(
        account_id.clone(),
        "responses",
        18_000,
        SelectorQuotaWindowStatus::Eligible,
    )
    .with_remaining_headroom(91)
    .with_reset_unix_seconds(18_000)
    .with_effective(true)
    .with_observed_unix_seconds(1_000);
    let weekly_window = PersistedSelectorQuotaWindow::new(
        account_id.clone(),
        "responses",
        604_800,
        SelectorQuotaWindowStatus::Eligible,
    )
    .with_remaining_headroom(54)
    .with_reset_unix_seconds(604_800)
    .with_observed_unix_seconds(1_000);
    if let Err(error) = seed_store.upsert_selector_quota_window(&short_window).await {
        panic!("legacy seed should persist short selector window: {error}");
    }
    if let Err(error) = seed_store
        .upsert_selector_quota_window(&weekly_window)
        .await
    {
        panic!("legacy seed should persist weekly selector window: {error}");
    }
    seed_store.close().await.expect("seed store should close");

    convert_current_fixture_to_v12(&database_path).await;
    let mut legacy = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(false)
            .foreign_keys(false),
    )
    .await
    .expect("legacy seed should reopen");
    sqlx::raw_sql(
        "CREATE TABLE session_account_affinities (
        session_id TEXT PRIMARY KEY NOT NULL,
        account_id TEXT NOT NULL,
        last_seen_unix_seconds INTEGER NOT NULL
    ); PRAGMA user_version = 13;",
    )
    .execute(&mut legacy)
    .await
    .expect("legacy v13 session schema should seed");
    let history_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE name = '_sqlx_migrations'")
            .fetch_one(&mut legacy)
            .await
            .expect("legacy history absence should query");
    assert_eq!(history_count, 0);
    assert!(
        sqlx::query("SELECT provider FROM accounts")
            .fetch_optional(&mut legacy)
            .await
            .is_err()
    );
    legacy.close().await.expect("legacy seed should close");

    let migrated_store = AsyncSqliteStateStore::open(&database_path)
        .await
        .expect("legacy-seeded legacy state should migrate to provider schema");
    migrated_store
        .close()
        .await
        .expect("migrated legacy-seeded state should close");
    let read_only_store = match AsyncSqliteStateStore::open_read_only(&database_path).await {
        Ok(store) => store,
        Err(error) => {
            panic!(
                "legacy-seeded state database should satisfy read-only quota status schema: {error}"
            )
        }
    };
    let observations = match AsyncQuotaHistoryRepository::quota_history_observations_for_window(
        &read_only_store,
        &account_id,
        "responses",
        604_800,
        0,
        2_000,
    )
    .await
    {
        Ok(observations) => observations,
        Err(error) => {
            panic!("quota status read-only quota history query should find current schema: {error}")
        }
    };

    assert!(
        observations.is_empty(),
        "legacy-seeded state should start with no quota history observations"
    );
    read_only_store
        .close()
        .await
        .expect("read-only quota store should close");
}

#[tokio::test]
async fn async_writable_store_enables_wal_journal_mode() {
    let temp_dir = TestTempDir::new("async_wal_journal_mode");
    let database_path = temp_dir.path().join("state.sqlite");
    let store = match AsyncSqliteStateStore::open(&database_path).await {
        Ok(store) => store,
        Err(error) => panic!("async state store should open and migrate: {error}"),
    };
    if let Err(error) = store.close().await {
        panic!("async state store should close cleanly: {error}");
    }
    drop(store);

    let mut connection = match sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database_path)
            .read_only(true),
    )
    .await
    {
        Ok(connection) => connection,
        Err(error) => panic!("sqlite database should open for journal inspection: {error}"),
    };
    let journal_mode: String = match sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut connection)
        .await
    {
        Ok(journal_mode) => journal_mode,
        Err(error) => panic!("journal mode should be readable: {error}"),
    };
    assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
    connection
        .close()
        .await
        .expect("journal inspection should close");
}

#[tokio::test]
async fn existing_delete_mode_state_file_reopens_in_wal_for_each_writable_store() {
    let temp_dir = TestTempDir::new("async_existing_delete_wal_journal_mode");
    let database_path = temp_dir.path().join("state.sqlite");
    let store = AsyncSqliteStateStore::open(&database_path)
        .await
        .expect("state store should create the current schema");
    store
        .close()
        .await
        .expect("state store should close before switching the fixture");

    set_journal_mode(&database_path, "DELETE").await;
    let store = AsyncSqliteStateStore::open(&database_path)
        .await
        .expect("state store should reopen a DELETE database");
    store.close().await.expect("state store should close");
    assert_eq!(journal_mode(&database_path).await.as_deref(), Some("wal"));

    set_journal_mode(&database_path, "DELETE").await;
    let weekly_floor = AsyncWeeklyQuotaFloorMutationStore::open(&database_path)
        .await
        .expect("weekly-floor mutation store should reopen a DELETE database");
    weekly_floor.close().await;
    assert_eq!(journal_mode(&database_path).await.as_deref(), Some("wal"));

    set_journal_mode(&database_path, "DELETE").await;
    let credit_policy =
        crate::credit_store::AsyncCreditUsagePolicyMutationStore::open(&database_path)
            .await
            .expect("credit-policy mutation store should reopen a DELETE database");
    credit_policy.close().await;
    assert_eq!(journal_mode(&database_path).await.as_deref(), Some("wal"));
}

async fn set_journal_mode(database_path: &Path, requested_mode: &str) {
    use sqlx::Connection as _;
    let mut connection = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(database_path)
            .create_if_missing(false)
            .pragma("journal_mode", requested_mode.to_owned()),
    )
    .await
    .expect("state database should open in requested journal mode");
    let actual_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut connection)
        .await
        .expect("fixture journal mode should read");
    assert_eq!(
        actual_mode.to_ascii_lowercase(),
        requested_mode.to_ascii_lowercase()
    );
    connection
        .close()
        .await
        .expect("journal fixture should close");
}

async fn journal_mode(database_path: &Path) -> Option<String> {
    use sqlx::Connection as _;
    let mut connection = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(database_path)
            .read_only(true),
    )
    .await
    .ok()?;
    let mode = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut connection)
        .await
        .ok();
    connection.close().await.ok()?;
    mode
}
