use super::*;
use sqlx::Connection as _;

#[tokio::test]
async fn account_provider_is_immutable_after_insert() {
    let temp_dir = TestTempDir::new("account_provider_immutable");
    let database_path = temp_dir.path().join("state.sqlite");
    let store = AsyncSqliteStateStore::open(&database_path)
        .await
        .expect("state store should open");
    let account_id = account_id("acct_immutable_provider");
    let openai_account = AccountRecord::new(
        codex_router_core::provider::Provider::Openai,
        account_id.clone(),
        "shared-label",
        AccountStatus::Enabled,
    );
    store
        .upsert_account(&openai_account)
        .await
        .expect("OpenAI account should persist");

    let claude_account = AccountRecord::new(
        Provider::Claude,
        account_id.clone(),
        "shared-label",
        AccountStatus::Enabled,
    );
    assert_eq!(
        store.upsert_account(&claude_account).await,
        Err(StateStoreError::AccountProviderImmutable)
    );

    let persisted_account = store
        .load_account(&account_id)
        .await
        .expect("account should load")
        .expect("account should remain");
    assert_eq!(persisted_account.provider(), Provider::Openai);
    store.close().await.expect("state store should close");
}

#[tokio::test]
async fn native_v13_database_rejects_missing_session_affinity_index() {
    let temp_dir = TestTempDir::new("session_account_affinity_v13_index_upgrade");
    let database_path = temp_dir.path().join("state.sqlite");
    let initial_store = AsyncSqliteStateStore::open(&database_path)
        .await
        .expect("initial v13 store should open");
    initial_store
        .close()
        .await
        .expect("initial store should close");
    let mut existing_v13 = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .foreign_keys(false),
    )
    .await
    .expect("v13 database should open");
    sqlx::query("DROP INDEX session_account_affinities_last_seen_lookup")
        .execute(&mut existing_v13)
        .await
        .map(|result| result.rows_affected())
        .expect("test fixture should remove the post-v13 index");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA user_version")
            .fetch_one(&mut existing_v13)
            .await
            .expect("v13 fixture version should query"),
        13
    );
    existing_v13
        .close()
        .await
        .expect("fixture connection should close");

    let error = AsyncSqliteStateStore::open(&database_path)
        .await
        .expect_err("native current database must reject a missing required index");
    assert_eq!(
        error,
        StateStoreError::Sqlite {
            message: "incompatible account database schema".to_owned()
        }
    );
    let mut inspected = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .foreign_keys(false),
    )
    .await
    .expect("reopened database should inspect");
    let index_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
                   FROM pragma_index_list('session_account_affinities')
                  WHERE name = 'session_account_affinities_last_seen_lookup'",
    )
    .fetch_one(&mut inspected)
    .await
    .expect("session affinity indexes should query");
    assert_eq!(index_count, 0, "rejection must not repair native schema");
    inspected
        .close()
        .await
        .expect("schema inspection should close");
}
