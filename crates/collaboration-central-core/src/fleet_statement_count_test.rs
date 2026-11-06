use super::StatementCountScope;
use crate::fleet_storage_fixture::{FleetStorageFixture, TestError};
use durable_steps::{AtomicStore, storage::TransactionScope};
use sqlx_core::{executor::Executor, query::query, row::Row, sql_str::SqlSafeStr};
use sqlx_turso::Turso;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The real executor must preserve rows, errors and exact execution counts."
)]
async fn counts_polled_executions_including_cached_queries_but_not_preparation()
-> Result<(), TestError> {
    // Arrange: a real store, without a tracing dispatcher or shared counter.
    let fixture = FleetStorageFixture::open().await?;
    fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                let mut counted = StatementCountScope::new(scope);
                drop(query::<Turso>("SELECT 7 AS value").fetch(counted.executor()));
                drop(query::<Turso>("SELECT 7 AS value").fetch_optional(counted.executor()));
                let _prepared = counted
                    .executor()
                    .prepare("SELECT 7 AS value".into_sql_str())
                    .await?;
                assert_eq!(counted.statement_count(), 0);

                // Act/Assert: cached executions count once each; results stay real.
                for expected_count in 1..=2 {
                    let rows = query::<Turso>("SELECT 7 AS value")
                        .fetch_all(counted.executor())
                        .await?;
                    assert_eq!(rows.len(), 1);
                    assert_eq!(rows[0].try_get::<i64, _>("value")?, 7);
                    assert_eq!(counted.statement_count(), expected_count);
                }
                let row = query::<Turso>("SELECT 11 AS value")
                    .fetch_one(counted.executor())
                    .await?;
                assert_eq!(row.try_get::<i64, _>("value")?, 11);
                let absent = query::<Turso>("SELECT 13 AS value WHERE 0")
                    .fetch_optional(counted.executor())
                    .await?;
                assert!(absent.is_none());
                query::<Turso>("SELECT 17 AS value")
                    .execute(counted.executor())
                    .await?;
                let failed = query::<Turso>("SELECT 19 AS value WHERE 0")
                    .fetch_one(counted.executor())
                    .await;
                assert!(matches!(failed, Err(sqlx_core::Error::RowNotFound)));
                assert_eq!(counted.statement_count(), 6);
                Ok(())
            })
        })
        .await?;
    fixture.close().await?;
    Ok(())
}
