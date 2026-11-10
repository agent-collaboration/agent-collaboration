use super::*;
use sqlx::Connection as _;

#[tokio::test]
async fn v10_v11_v12_native_upgrades_create_credit_tables_and_preserve_account_policy() {
    for version in ["v10", "v11", "v12"] {
        let temp_dir = TestTempDir::new(&format!("credit_tables_{version}_upgrade"));
        let database_path = temp_dir.path().join("state.sqlite");
        let account_id = account_id(&format!("acct_credit_tables_{version}"));
        let account = AccountRecord::new(
            Provider::Openai,
            account_id.clone(),
            format!("credit-tables-{version}"),
            AccountStatus::Enabled,
        )
        .with_active_credential_generation(1);
        let current = AsyncSqliteStateStore::open(&database_path)
            .await
            .unwrap_or_else(|error| panic!("current fixture should open for {version}: {error}"));
        current
            .upsert_account(&account)
            .await
            .unwrap_or_else(|error| panic!("{version} account should persist: {error}"));
        current
            .close()
            .await
            .unwrap_or_else(|error| panic!("current fixture should close for {version}: {error}"));

        let preserved_floor = WeeklyQuotaFloorBasisPoints::new(800)
            .expect("800 basis points should be a valid floor");
        if version != "v10" {
            let mutation = AsyncWeeklyQuotaFloorMutationStore::open(&database_path)
                .await
                .unwrap_or_else(|error| {
                    panic!("current floor mutation store should open: {error}")
                });
            mutation
                .set_weekly_quota_floor_by_account_id(&account_id, Some(preserved_floor))
                .await
                .unwrap_or_else(|error| panic!("{version} floor should persist: {error}"));
            mutation.close().await;
        }

        match version {
            "v10" => convert_current_fixture_to_v10(&database_path).await,
            "v11" => convert_current_fixture_to_v11(&database_path).await,
            "v12" => convert_current_fixture_to_v12(&database_path).await,
            _ => panic!("unknown legacy fixture version"),
        }
        assert_credit_migration_tables(&database_path, false).await;

        let migrated = AsyncSqliteStateStore::open(&database_path)
            .await
            .unwrap_or_else(|error| panic!("{version} native database should upgrade: {error}"));
        assert_eq!(migrated.schema_version().await, Ok(13));
        assert_eq!(migrated.list_accounts().await, Ok(vec![account]));
        let expected_policies = if version == "v10" {
            Vec::new()
        } else {
            vec![AccountRoutingPolicy::new(account_id, preserved_floor)]
        };
        assert_eq!(
            migrated.list_account_routing_policies().await,
            Ok(expected_policies),
            "{version} account policy should survive its native upgrade"
        );
        migrated
            .close()
            .await
            .unwrap_or_else(|error| panic!("upgraded {version} database should close: {error}"));
        assert_credit_migration_tables(&database_path, true).await;
    }
}

#[test]
fn weekly_floor_accepts_only_integer_percent_basis_points() {
    for basis_points in [100_u16, 200, 500, 1_000, 1_500] {
        assert_eq!(
            WeeklyQuotaFloorBasisPoints::new(basis_points)
                .map(WeeklyQuotaFloorBasisPoints::basis_points),
            Ok(basis_points)
        );
    }
    for basis_points in [0_u16, 1, 99, 101, 999, 1_501, 1_600] {
        assert!(WeeklyQuotaFloorBasisPoints::new(basis_points).is_err());
    }
}

#[tokio::test]
async fn v10_migrates_through_v11_to_v12_and_exposes_policy_read_only() {
    let temp_dir = TestTempDir::new("v10_to_v12_policy_migration");
    let database_path = temp_dir.path().join("state.sqlite");
    let v10 = AsyncSqliteStateStore::open(&database_path)
        .await
        .unwrap_or_else(|error| panic!("v10 fixture should open: {error}"));
    let account = AccountRecord::new(
        codex_router_core::provider::Provider::Openai,
        account_id("acct_v11_preserved"),
        "preserved",
        AccountStatus::Enabled,
    );
    v10.upsert_account(&account)
        .await
        .unwrap_or_else(|error| panic!("v10 account should persist: {error}"));
    v10.close().await.expect("current fixture should close");
    let mut raw = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .foreign_keys(false),
    )
    .await
    .expect("fixture should reopen for seeding");
    sqlx::raw_sql(
        "INSERT INTO quota_snapshots VALUES (
                'acct_v11_preserved', 'mock_endpoint', 100, 'responses', 5000,
                1000, NULL, 0
             );
             INSERT INTO selector_quota_windows VALUES (
                'acct_v11_preserved', 'responses', 604800, 'eligible', 5000,
                1000, 1, 100
             );
             INSERT INTO active_client_leases VALUES (
                'responses', 'process', 'reservation', 'acct_v11_preserved', 100, 0
             );
             INSERT INTO active_session_events (
                account_id, route_band, process_run_id, logical_session_id,
                reservation_id, event_kind, event_unix_seconds,
                session_started_unix_seconds, session_ended_unix_seconds,
                transport_kind
             ) VALUES (
                'acct_v11_preserved', 'responses', 'process', 'session',
                'reservation', 'acquired', 100, 100, NULL, 'websocket'
             );
             INSERT INTO active_session_rollups VALUES (
                'acct_v11_preserved', 'responses', 0, 300, 100, 1, 0, 0
             );
             CREATE TABLE unrelated_v10_sentinel (value TEXT NOT NULL);
             INSERT INTO unrelated_v10_sentinel VALUES ('preserved');",
    )
    .execute(&mut raw)
    .await
    .expect("canonical v10 rows should seed");
    raw.close().await.expect("fixture connection should close");
    convert_current_fixture_to_v10(&database_path).await;

    let migrated = AsyncSqliteStateStore::open(&database_path)
        .await
        .unwrap_or_else(|error| panic!("v10 database should migrate: {error}"));
    assert_eq!(migrated.schema_version().await, Ok(13));
    assert_eq!(migrated.list_accounts().await, Ok(vec![account]));
    assert_eq!(
        migrated.list_account_routing_policies().await,
        Ok(Vec::new())
    );
    migrated.close().await.expect("migrated store should close");

    let mut raw = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .foreign_keys(false),
    )
    .await
    .expect("migrated database should inspect");
    for table_name in [
        "quota_snapshots",
        "selector_quota_windows",
        "active_client_leases",
        "active_session_events",
        "active_session_rollups",
        "unrelated_v10_sentinel",
    ] {
        let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {table_name}"
        )))
        .fetch_one(&mut raw)
        .await
        .unwrap_or_else(|error| panic!("{table_name} should survive: {error}"));
        assert_eq!(count, 1, "{table_name} row should survive migration");
    }
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&mut raw)
        .await
        .expect("integrity check should run");
    assert_eq!(integrity, "ok");
    assert!(
        sqlx::query("INSERT INTO account_routing_policies VALUES ('acct_v11_preserved', 50)")
            .execute(&mut raw)
            .await
            .map(|result| result.rows_affected())
            .is_err()
    );
    assert!(
        sqlx::query("INSERT INTO account_routing_policies VALUES ('acct_v11_preserved', 1501)")
            .execute(&mut raw)
            .await
            .map(|result| result.rows_affected())
            .is_err()
    );
    raw.close().await.expect("fixture connection should close");

    let read_only = AsyncSqliteStateStore::open_read_only(&database_path)
        .await
        .unwrap_or_else(|error| panic!("v12 should reopen read-only: {error}"));
    assert_eq!(
        read_only.list_account_routing_policies().await,
        Ok(Vec::new())
    );
    read_only.close().await.expect("read_only should close");
}

#[tokio::test]
async fn v12_migration_preserves_v11_policy_and_expands_integer_percent_check() {
    let temp_dir = TestTempDir::new("v11_to_v12_policy_migration");
    let database_path = temp_dir.path().join("state.sqlite");
    let store = AsyncSqliteStateStore::open(&database_path)
        .await
        .expect("current fixture should open");
    let account_id = account_id("acct_v12_preserved");
    store
        .upsert_account(&AccountRecord::new(
            codex_router_core::provider::Provider::Openai,
            account_id.clone(),
            "v12-preserved",
            AccountStatus::Enabled,
        ))
        .await
        .expect("account should persist");
    store.close().await.expect("fixture should close");
    let mutation = AsyncWeeklyQuotaFloorMutationStore::open(&database_path)
        .await
        .expect("current mutation store should open");
    let preserved_floor = WeeklyQuotaFloorBasisPoints::new(1_000).expect("valid v11 floor");
    mutation
        .set_weekly_quota_floor_by_label("v12-preserved", Some(preserved_floor))
        .await
        .expect("v11-compatible floor should persist");
    mutation.close().await;
    convert_current_fixture_to_v11(&database_path).await;

    let migrated = AsyncSqliteStateStore::open(&database_path)
        .await
        .expect("v11 database should migrate to v12");
    assert_eq!(migrated.schema_version().await, Ok(13));
    assert_eq!(
        migrated.list_account_routing_policies().await,
        Ok(vec![AccountRoutingPolicy::new(
            account_id.clone(),
            preserved_floor
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
    .expect("v12 database should inspect");
    let table_sql: String = sqlx::query_scalar(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'account_routing_policies'",
    )
    .fetch_one(&mut raw)
    .await
    .expect("policy table SQL should read");
    assert!(table_sql.contains("BETWEEN 100 AND 1500"));
    assert_eq!(
        sqlx::query("INSERT INTO account_routing_policies VALUES ('acct_v12_fifteen', 1500)")
            .execute(&mut raw)
            .await
            .expect("valid fifteen-percent floor should insert")
            .rows_affected(),
        1
    );
    for invalid_basis_points in [1_501, 1_550, 1_600] {
        assert!(
            sqlx::query("INSERT INTO account_routing_policies VALUES (?1, ?2)")
                .bind(format!("acct_invalid_{invalid_basis_points}"))
                .bind(invalid_basis_points)
                .execute(&mut raw)
                .await
                .map(|result| result.rows_affected())
                .is_err(),
            "{invalid_basis_points} basis points must be rejected"
        );
    }
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&mut raw)
        .await
        .expect("integrity check should run");
    assert_eq!(integrity, "ok");
    raw.close().await.expect("raw should close");
}

#[tokio::test]
async fn v10_native_migration_rolls_back_atomically_and_retries() {
    let temp_dir = TestTempDir::new("v11_sync_rollback_retry");
    let database_path = temp_dir.path().join("state.sqlite");
    let store = AsyncSqliteStateStore::open(&database_path)
        .await
        .expect("fixture should open");
    store.close().await.expect("state store should close");
    convert_current_fixture_to_v10(&database_path).await;

    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&database_path)
                .create_if_missing(false),
        )
        .await
        .expect("migration rollback pool should open");
    crate::account_migrations::migrate_and_rollback_for_test(&pool)
        .await
        .expect("native migration transaction should roll back");
    pool.close().await;
    let mut connection = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .foreign_keys(false),
    )
    .await
    .expect("fixture should reopen");
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut connection)
        .await
        .expect("version should read");
    assert_eq!(version, 10);
    assert!(
        sqlx::query("SELECT 1 FROM account_routing_policies")
            .fetch_optional(&mut connection)
            .await
            .is_err()
    );
    connection
        .close()
        .await
        .expect("fixture connection should close");

    let migrated = AsyncSqliteStateStore::open(&database_path)
        .await
        .expect("native migration retry should succeed");
    assert_eq!(migrated.schema_version().await, Ok(13));
    migrated.close().await.expect("migrated should close");
}

#[tokio::test]
async fn v11_native_migration_rolls_back_to_intact_policy_and_retries() {
    let temp_dir = TestTempDir::new("v12_sync_rollback_retry");
    let database_path = temp_dir.path().join("state.sqlite");
    let store = AsyncSqliteStateStore::open(&database_path)
        .await
        .expect("current fixture should open");
    let account_id = account_id("acct_v12_sync_rollback");
    store
        .upsert_account(&AccountRecord::new(
            codex_router_core::provider::Provider::Openai,
            account_id.clone(),
            "v12-sync-rollback",
            AccountStatus::Enabled,
        ))
        .await
        .expect("account should persist");
    store.close().await.expect("state store should close");
    let mut raw = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .foreign_keys(false),
    )
    .await
    .expect("fixture should reopen");
    sqlx::query("INSERT INTO account_routing_policies VALUES (?1, 900)")
        .bind(account_id.as_str())
        .execute(&mut raw)
        .await
        .map(|result| result.rows_affected())
        .expect("policy should persist");
    raw.close().await.expect("fixture connection should close");
    convert_current_fixture_to_v11(&database_path).await;

    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&database_path)
                .create_if_missing(false),
        )
        .await
        .expect("migration rollback pool should open");
    crate::account_migrations::migrate_and_rollback_for_test(&pool)
        .await
        .expect("native migration transaction should roll back");
    pool.close().await;
    assert_intact_v11_policy_database(&database_path, account_id.as_str(), 900).await;

    let migrated = AsyncSqliteStateStore::open(&database_path)
        .await
        .expect("native v12 migration retry should succeed");
    assert_eq!(migrated.schema_version().await, Ok(13));
    migrated.close().await.expect("state store should close");
    let mut raw = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .foreign_keys(false),
    )
    .await
    .expect("migrated database should reopen");
    let preserved_floor: i64 = sqlx::query_scalar("SELECT weekly_quota_floor_basis_points FROM account_routing_policies WHERE account_id = ?1").bind(account_id.as_str()).fetch_one(&mut raw).await
            .expect("native migration should preserve policy");
    assert_eq!(preserved_floor, 900);
    raw.close().await.expect("raw should close");
}
