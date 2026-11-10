use std::path::Path;

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection, SqliteConnection};

pub(super) async fn create_v2_database_with_quota_snapshot(database_path: &Path) {
    let mut connection = match SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(database_path)
            .create_if_missing(true)
            .foreign_keys(false),
    )
    .await
    {
        Ok(connection) => connection,
        Err(error) => panic!("raw v2 database should open: {error}"),
    };
    if let Err(error) = sqlx::raw_sql(
        "
            CREATE TABLE accounts (
                account_id TEXT PRIMARY KEY NOT NULL,
                label TEXT NOT NULL,
                status TEXT NOT NULL,
                active_credential_generation INTEGER
            );

            CREATE TABLE quota_snapshots (
                account_id TEXT NOT NULL,
                source TEXT NOT NULL,
                observed_unix_seconds INTEGER NOT NULL,
                route_band TEXT NOT NULL,
                remaining_headroom INTEGER NOT NULL,
                reset_unix_seconds INTEGER,
                stale_penalty INTEGER NOT NULL,
                PRIMARY KEY (account_id, route_band)
            );

            CREATE TABLE affinity_pins (
                affinity_key TEXT PRIMARY KEY NOT NULL,
                account_id TEXT NOT NULL
            );

            INSERT INTO accounts (
                account_id, label, status, active_credential_generation
            ) VALUES (
                'acct_v2_backfill', 'v2-backfill', 'enabled', 1
            );

            INSERT INTO quota_snapshots (
                account_id, source, observed_unix_seconds, route_band,
                remaining_headroom, reset_unix_seconds, stale_penalty
            ) VALUES (
                'acct_v2_backfill', 'mock_endpoint', 1000, 'responses',
                64, 2000, 0
            );

            INSERT INTO quota_snapshots (
                account_id, source, observed_unix_seconds, route_band,
                remaining_headroom, reset_unix_seconds, stale_penalty
            ) VALUES (
                'acct_v2_backfill', 'mock_endpoint', 1000, 'code_review',
                64, 2000, 0
            );

            PRAGMA user_version = 2;
            ",
    )
    .execute(&mut connection)
    .await
    {
        panic!("raw v2 database should initialize: {error}");
    }
    connection
        .close()
        .await
        .expect("legacy fixture connection should close");
}

pub(super) async fn create_v3_database_with_legacy_code_review_selector_window(
    database_path: &Path,
) {
    let mut connection = match SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(database_path)
            .create_if_missing(true)
            .foreign_keys(false),
    )
    .await
    {
        Ok(connection) => connection,
        Err(error) => panic!("raw v3 database should open: {error}"),
    };
    if let Err(error) = sqlx::raw_sql(
        "
            CREATE TABLE accounts (
                account_id TEXT PRIMARY KEY NOT NULL,
                label TEXT NOT NULL,
                status TEXT NOT NULL,
                active_credential_generation INTEGER
            );

            CREATE TABLE quota_snapshots (
                account_id TEXT NOT NULL,
                source TEXT NOT NULL,
                observed_unix_seconds INTEGER NOT NULL,
                route_band TEXT NOT NULL,
                remaining_headroom INTEGER NOT NULL,
                reset_unix_seconds INTEGER,
                stale_penalty INTEGER NOT NULL,
                PRIMARY KEY (account_id, route_band)
            );

            CREATE TABLE affinity_pins (
                affinity_key TEXT PRIMARY KEY NOT NULL,
                account_id TEXT NOT NULL
            );

            CREATE TABLE selector_quota_windows (
                account_id TEXT NOT NULL,
                route_band TEXT NOT NULL,
                limit_window_seconds INTEGER NOT NULL,
                status TEXT NOT NULL,
                remaining_headroom INTEGER NOT NULL,
                reset_unix_seconds INTEGER,
                effective INTEGER NOT NULL,
                observed_unix_seconds INTEGER NOT NULL,
                PRIMARY KEY (account_id, route_band, limit_window_seconds)
            );

            INSERT INTO accounts (
                account_id, label, status, active_credential_generation
            ) VALUES (
                'acct_v3_cleanup', 'v3-cleanup', 'enabled', 1
            );

            INSERT INTO selector_quota_windows (
                account_id, route_band, limit_window_seconds, status,
                remaining_headroom, reset_unix_seconds, effective,
                observed_unix_seconds
            ) VALUES (
                'acct_v3_cleanup', 'responses', 18000, 'eligible',
                64, 2000, 1, 1000
            );

            INSERT INTO selector_quota_windows (
                account_id, route_band, limit_window_seconds, status,
                remaining_headroom, reset_unix_seconds, effective,
                observed_unix_seconds
            ) VALUES (
                'acct_v3_cleanup', 'code_review', 604800, 'eligible',
                77, 999999, 1, 12345
            );

            PRAGMA user_version = 3;
            ",
    )
    .execute(&mut connection)
    .await
    {
        panic!("raw v3 database should initialize: {error}");
    }
    connection
        .close()
        .await
        .expect("legacy fixture connection should close");
}

pub(super) async fn create_v6_database_without_reset_credits(database_path: &Path) {
    let mut connection = match SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(database_path)
            .create_if_missing(true)
            .foreign_keys(false),
    )
    .await
    {
        Ok(connection) => connection,
        Err(error) => panic!("raw v6 database should open: {error}"),
    };
    if let Err(error) = sqlx::raw_sql(
        "
            CREATE TABLE accounts (
                account_id TEXT PRIMARY KEY NOT NULL,
                label TEXT NOT NULL,
                status TEXT NOT NULL,
                active_credential_generation INTEGER
            );

            CREATE TABLE quota_snapshots (
                account_id TEXT NOT NULL,
                source TEXT NOT NULL,
                observed_unix_seconds INTEGER NOT NULL,
                route_band TEXT NOT NULL,
                remaining_headroom INTEGER NOT NULL,
                reset_unix_seconds INTEGER,
                stale_penalty INTEGER NOT NULL,
                PRIMARY KEY (account_id, route_band)
            );

            CREATE TABLE affinity_pins (
                affinity_key TEXT PRIMARY KEY NOT NULL,
                account_id TEXT NOT NULL
            );

            CREATE TABLE selector_quota_windows (
                account_id TEXT NOT NULL,
                route_band TEXT NOT NULL,
                limit_window_seconds INTEGER NOT NULL,
                status TEXT NOT NULL,
                remaining_headroom INTEGER NOT NULL,
                reset_unix_seconds INTEGER,
                effective INTEGER NOT NULL,
                observed_unix_seconds INTEGER NOT NULL,
                PRIMARY KEY (account_id, route_band, limit_window_seconds)
            );

            CREATE TABLE quota_refresh_status (
                account_id TEXT NOT NULL,
                route_band TEXT NOT NULL,
                last_success_unix_seconds INTEGER,
                last_attempt_unix_seconds INTEGER,
                last_error_class TEXT,
                stale_after_unix_seconds INTEGER,
                PRIMARY KEY (account_id, route_band)
            );

            CREATE TABLE previous_response_affinity_owners (
                affinity_key_hash TEXT NOT NULL,
                route_band TEXT NOT NULL,
                account_id TEXT NOT NULL,
                credential_generation INTEGER NOT NULL,
                source_transport TEXT NOT NULL,
                created_unix_seconds INTEGER NOT NULL,
                PRIMARY KEY (affinity_key_hash, route_band, account_id)
            );

            INSERT INTO accounts (
                account_id, label, status, active_credential_generation
            ) VALUES (
                'acct_v6_reset_credits', 'v6-reset-credits', 'enabled', 1
            );

            INSERT INTO quota_snapshots (
                account_id, source, observed_unix_seconds, route_band,
                remaining_headroom, reset_unix_seconds, stale_penalty
            ) VALUES (
                'acct_v6_reset_credits', 'mock_endpoint', 1_000, 'responses',
                42, 2_000, 0
            );

            PRAGMA user_version = 6;
            ",
    )
    .execute(&mut connection)
    .await
    {
        panic!("raw v6 database should initialize: {error}");
    }
    connection
        .close()
        .await
        .expect("legacy fixture connection should close");
}

pub(super) async fn assert_unsupported_quota_fixture_is_preserved(
    database_path: &Path,
    expected_version: i64,
    expected_rows: &[(&str, &[&str])],
) {
    let options = SqliteConnectOptions::new()
        .filename(database_path)
        .read_only(true)
        .pragma("query_only", "ON");
    let mut before = SqliteConnection::connect_with(&options)
        .await
        .expect("historical fixture should open read-only");
    let schema_query =
        "SELECT name,sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY name";
    let expected_schema: Vec<(String, Option<String>)> = sqlx::query_as(schema_query)
        .fetch_all(&mut before)
        .await
        .expect("historical schema should read before refusal");
    for (query, expected) in expected_rows {
        let actual: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(*query))
            .fetch_all(&mut before)
            .await
            .expect("original historical rows should read before refusal");
        assert_eq!(
            actual, *expected,
            "historical fixture must reach its intended input"
        );
    }
    before
        .close()
        .await
        .expect("historical inspection should close");

    assert_eq!(
        crate::sqlite::AsyncSqliteStateStore::open(database_path)
            .await
            .expect_err("unsupported historical version must be refused"),
        crate::sqlite::StateStoreError::UnsupportedSchemaVersion {
            version: expected_version,
        },
    );

    let mut after = SqliteConnection::connect_with(&options)
        .await
        .expect("refused fixture should reopen read-only");
    let actual_schema: Vec<(String, Option<String>)> = sqlx::query_as(schema_query)
        .fetch_all(&mut after)
        .await
        .expect("historical schema should survive refusal");
    assert_eq!(actual_schema, expected_schema);
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut after)
        .await
        .expect("historical version should remain readable");
    assert_eq!(version, expected_version);
    let history_tables: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE name = '_sqlx_migrations'")
            .fetch_one(&mut after)
            .await
            .expect("migration history absence should query");
    assert_eq!(history_tables, 0);
    for (query, expected) in expected_rows {
        let actual: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(*query))
            .fetch_all(&mut after)
            .await
            .expect("historical rows should survive refusal");
        assert_eq!(actual, *expected, "unsupported input must remain intact");
    }
    after
        .close()
        .await
        .expect("refused inspection should close");
}
