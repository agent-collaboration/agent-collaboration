use super::*;
use sqlx::Connection as _;

#[tokio::test]
async fn mutation_only_store_requires_v12_and_roundtrips_set_then_delete() {
    let temp_dir = TestTempDir::new("weekly_floor_mutation");
    let database_path = temp_dir.path().join("state.sqlite");
    let store = AsyncSqliteStateStore::open(&database_path)
        .await
        .unwrap_or_else(|error| panic!("state should open: {error}"));
    let account_id = account_id("acct_weekly_floor");
    store
        .upsert_account(&AccountRecord::new(
            codex_router_core::provider::Provider::Openai,
            account_id.clone(),
            "weekly-floor",
            AccountStatus::Enabled,
        ))
        .await
        .unwrap_or_else(|error| panic!("account should persist: {error}"));
    store.close().await.expect("state should close");

    let mutation = AsyncWeeklyQuotaFloorMutationStore::open(&database_path)
        .await
        .unwrap_or_else(|error| panic!("v12 mutation store should open: {error}"));
    let floor = WeeklyQuotaFloorBasisPoints::new(500)
        .unwrap_or_else(|error| panic!("floor should validate: {error}"));
    assert_eq!(
        mutation
            .set_weekly_quota_floor_by_account_id(&account_id, Some(floor))
            .await,
        Ok(WeeklyQuotaFloorMutationResult::Enabled(
            AccountRoutingPolicy::new(account_id.clone(), floor)
        ))
    );
    assert_eq!(
        mutation
            .set_weekly_quota_floor_by_account_id(&account_id, None)
            .await,
        Ok(WeeklyQuotaFloorMutationResult::Disabled)
    );
    let missing_account_id =
        AccountId::new("acct_weekly_floor_missing").expect("valid missing account id");
    assert_eq!(
        mutation
            .set_weekly_quota_floor_by_account_id(&missing_account_id, Some(floor))
            .await,
        Err(StateStoreError::WeeklyQuotaFloorAccountNotFound)
    );
    mutation.close().await;

    let read_only = AsyncSqliteStateStore::open_read_only(&database_path)
        .await
        .unwrap_or_else(|error| panic!("state should reopen: {error}"));
    assert_eq!(
        read_only.list_account_routing_policies().await,
        Ok(Vec::new())
    );
}

#[tokio::test]
async fn mutation_only_store_rejects_v10_and_v11_without_migrating() {
    let temp_dir = TestTempDir::new("weekly_floor_requires_v12");
    let database_path = temp_dir.path().join("state.sqlite");
    let mut raw = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .foreign_keys(false),
    )
    .await
    .unwrap_or_else(|error| panic!("v10 fixture should open: {error}"));
    sqlx::raw_sql(
        "CREATE TABLE accounts (
                account_id TEXT PRIMARY KEY NOT NULL,
                label TEXT NOT NULL,
                status TEXT NOT NULL,
                active_credential_generation INTEGER
             );
             PRAGMA user_version = 10;",
    )
    .execute(&mut raw)
    .await
    .unwrap_or_else(|error| panic!("v10 fixture should initialize: {error}"));
    raw.close().await.expect("fixture connection should close");

    let error = AsyncWeeklyQuotaFloorMutationStore::open(&database_path)
        .await
        .expect_err("mutation open must reject v10");
    assert_eq!(
        error,
        StateStoreError::WeeklyQuotaFloorSchemaUpgradeRequired
    );

    let mut raw = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .foreign_keys(false),
    )
    .await
    .unwrap_or_else(|error| panic!("v10 fixture should reopen: {error}"));
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut raw)
        .await
        .unwrap_or_else(|error| panic!("version should read: {error}"));
    assert_eq!(version, 10);
    assert!(
        sqlx::query("SELECT 1 FROM account_routing_policies")
            .fetch_optional(&mut raw)
            .await
            .is_err()
    );
    sqlx::raw_sql(
        "CREATE TABLE account_routing_policies (
                account_id TEXT PRIMARY KEY NOT NULL,
                weekly_quota_floor_basis_points INTEGER NOT NULL
                    CHECK (
                        weekly_quota_floor_basis_points BETWEEN 100 AND 1000
                        AND weekly_quota_floor_basis_points % 100 = 0
                    )
             );
             PRAGMA user_version = 11;",
    )
    .execute(&mut raw)
    .await
    .expect("v11 policy schema should initialize");
    raw.close().await.expect("fixture connection should close");

    let error = AsyncWeeklyQuotaFloorMutationStore::open(&database_path)
        .await
        .expect_err("mutation open must reject v11");
    assert_eq!(
        error,
        StateStoreError::WeeklyQuotaFloorSchemaUpgradeRequired
    );
    let mut raw = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .foreign_keys(false),
    )
    .await
    .unwrap_or_else(|error| panic!("v11 fixture should reopen: {error}"));
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut raw)
        .await
        .unwrap_or_else(|error| panic!("version should read: {error}"));
    assert_eq!(version, 11);
    raw.close().await.expect("v11 inspection should close");
}

#[tokio::test]
async fn read_only_open_rejects_v11_without_migrating_policy_state() {
    let temp_dir = TestTempDir::new("read_only_v11_no_migration");
    let database_path = temp_dir.path().join("state.sqlite");
    let store = AsyncSqliteStateStore::open(&database_path)
        .await
        .expect("current state should open");
    let account_id = account_id("acct_read_only_v11");
    store
        .upsert_account(&AccountRecord::new(
            codex_router_core::provider::Provider::Openai,
            account_id.clone(),
            "read-only-v11",
            AccountStatus::Enabled,
        ))
        .await
        .expect("account should persist");
    store.close().await.expect("state should close");
    let mutation = AsyncWeeklyQuotaFloorMutationStore::open(&database_path)
        .await
        .expect("current mutation store should open");
    mutation
        .set_weekly_quota_floor_by_account_id(
            &account_id,
            Some(WeeklyQuotaFloorBasisPoints::new(800).expect("valid floor")),
        )
        .await
        .expect("policy should persist");
    mutation.close().await;
    convert_current_fixture_to_v11(&database_path).await;

    assert_eq!(
        AsyncSqliteStateStore::open_read_only(&database_path)
            .await
            .expect_err("read-only open must reject v11 without migrating"),
        StateStoreError::UnsupportedSchemaVersion { version: 11 }
    );
    assert_intact_v11_policy_database(&database_path, account_id.as_str(), 800).await;
}

#[tokio::test]
async fn every_v12_open_rejects_malformed_policy_columns() {
    let temp_dir = TestTempDir::new("v12_malformed_policy_columns");
    let database_path = temp_dir.path().join("state.sqlite");
    let store = AsyncSqliteStateStore::open(&database_path)
        .await
        .expect("valid v12 should open");
    store.close().await.expect("valid v12 should close");
    let mut raw = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .foreign_keys(false),
    )
    .await
    .expect("fixture should reopen");
    sqlx::raw_sql(
        "DROP TABLE account_routing_policies;
             CREATE TABLE account_routing_policies (account_id TEXT PRIMARY KEY NOT NULL);",
    )
    .execute(&mut raw)
    .await
    .expect("malformed policy table should install");
    raw.close().await.expect("fixture connection should close");

    assert_eq!(
        AsyncSqliteStateStore::open(&database_path)
            .await
            .expect_err("async writable open must reject malformed policy schema"),
        StateStoreError::Sqlite {
            message: "incompatible account database schema".to_owned()
        }
    );
    assert_eq!(
        AsyncSqliteStateStore::open_read_only(&database_path)
            .await
            .expect_err("read-only open must reject malformed policy schema"),
        StateStoreError::MissingReadOnlySchemaObject {
            object_kind: "column",
            object_name: "weekly_quota_floor_basis_points",
        }
    );
    assert_eq!(
        AsyncWeeklyQuotaFloorMutationStore::open(&database_path)
            .await
            .expect_err("mutation open must reject malformed policy schema"),
        StateStoreError::WeeklyQuotaFloorSchemaUpgradeRequired
    );

    let mut raw = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .foreign_keys(false),
    )
    .await
    .expect("fixture should reopen");
    sqlx::raw_sql("DROP TABLE account_routing_policies;")
        .execute(&mut raw)
        .await
        .expect("policy table should drop");
    raw.close().await.expect("fixture connection should close");
    assert_eq!(
        AsyncSqliteStateStore::open(&database_path)
            .await
            .expect_err("async writable open must reject missing policy table"),
        StateStoreError::Sqlite {
            message: "incompatible account database schema".to_owned()
        }
    );
    assert_eq!(
        AsyncSqliteStateStore::open_read_only(&database_path)
            .await
            .expect_err("read-only open must reject missing policy table"),
        StateStoreError::MissingReadOnlySchemaObject {
            object_kind: "table",
            object_name: "account_routing_policies",
        }
    );
    assert_eq!(
        AsyncWeeklyQuotaFloorMutationStore::open(&database_path)
            .await
            .expect_err("mutation open must reject missing policy table"),
        StateStoreError::WeeklyQuotaFloorSchemaUpgradeRequired
    );
}

#[tokio::test]
async fn production_async_v12_migration_contention_is_atomic_and_retryable() {
    let temp_dir = TestTempDir::new("v12_production_migration_contention");
    let database_path = temp_dir.path().join("state.sqlite");
    let account = AccountRecord::new(
        codex_router_core::provider::Provider::Openai,
        account_id("acct_migration_contention"),
        "migration-contention",
        AccountStatus::Enabled,
    );
    let store = AsyncSqliteStateStore::open(&database_path)
        .await
        .expect("fixture should open");
    store
        .upsert_account(&account)
        .await
        .expect("account should persist");
    store.close().await.expect("seed store should close");
    let mut raw = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .foreign_keys(false)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Delete),
    )
    .await
    .expect("fixture should reopen");
    let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut raw)
        .await
        .expect("fixture journal mode should read");
    assert_eq!(
        journal_mode, "delete",
        "keep the original held-writer fixture mode"
    );
    sqlx::query("INSERT INTO account_routing_policies VALUES (?1, 800)")
        .bind(account.account_id().as_str())
        .execute(&mut raw)
        .await
        .map(|result| result.rows_affected())
        .expect("v11-compatible policy should persist");
    raw.close().await.expect("fixture connection should close");
    convert_current_fixture_to_v11(&database_path).await;

    let mut lock = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .foreign_keys(false),
    )
    .await
    .expect("lock connection should open");
    sqlx::raw_sql("PRAGMA busy_timeout = 0; BEGIN IMMEDIATE;")
        .execute(&mut lock)
        .await
        .expect("writer lock should be held");
    let migration_attempt = tokio::time::timeout(
        std::time::Duration::from_millis(250),
        AsyncSqliteStateStore::open(&database_path),
    )
    .await;
    match migration_attempt {
        Ok(Err(StateStoreError::Sqlite { message })) => assert!(
            message.contains("locked") || message.contains("busy"),
            "held-writer migration should return a lock error, got: {message}"
        ),
        Ok(Err(error)) => panic!("held-writer migration returned the wrong error: {error}"),
        Ok(Ok(_)) => panic!("migration must not commit beneath a held writer"),
        Err(_) => panic!("production migration must return before the test timeout"),
    }
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut lock)
        .await
        .expect("version should read under lock");
    assert_eq!(version, 11);
    let preserved_floor: i64 = sqlx::query_scalar("SELECT weekly_quota_floor_basis_points FROM account_routing_policies WHERE account_id = ?1").bind(account.account_id().as_str()).fetch_one(&mut lock).await
            .expect("v11 policy should remain readable");
    assert_eq!(preserved_floor, 800);
    let replacement_exists: i64 = sqlx::query_scalar(
        "SELECT EXISTS(
                    SELECT 1 FROM sqlite_master
                     WHERE type = 'table' AND name = 'account_routing_policies_v12'
                 )",
    )
    .fetch_one(&mut lock)
    .await
    .expect("replacement-table state should read");
    assert_eq!(replacement_exists, 0);
    let preserved_accounts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
        .fetch_one(&mut lock)
        .await
        .expect("preserved account count should read");
    assert_eq!(preserved_accounts, 1);
    sqlx::raw_sql("ROLLBACK;")
        .execute(&mut lock)
        .await
        .expect("lock should release");
    lock.close().await.expect("lock connection should close");

    let migrated = AsyncSqliteStateStore::open(&database_path)
        .await
        .expect("production migration retry should succeed");
    assert_eq!(migrated.schema_version().await, Ok(13));
    assert_eq!(migrated.list_accounts().await, Ok(vec![account.clone()]));
    assert_eq!(
        migrated.list_account_routing_policies().await,
        Ok(vec![AccountRoutingPolicy::new(
            account.account_id().clone(),
            WeeklyQuotaFloorBasisPoints::new(800).expect("valid floor")
        )])
    );
    migrated.close().await.expect("migrated store should close");
    let mut raw = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .foreign_keys(false),
    )
    .await
    .expect("migrated DB should inspect");
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&mut raw)
        .await
        .expect("integrity check should run");
    assert_eq!(integrity, "ok");
    raw.close().await.expect("migrated inspection should close");
}
