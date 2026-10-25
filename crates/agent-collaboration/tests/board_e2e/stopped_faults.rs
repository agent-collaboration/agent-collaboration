//! Named faults touch only an owned native fixture after its Host has exited.
use super::fixture::BoardFixture;
use codex_router_test_support::FixtureResult;
use serde_json::Value;
use sqlx_turso::sqlx::{ConnectOptions, Connection};

async fn open(fixture: &BoardFixture, project: &str) -> FixtureResult<sqlx_turso::TursoConnection> {
    Ok(sqlx_turso::TursoConnectOptions::new()
        .filename(super::records_reopen::database_for(fixture, project)?)
        .create_if_missing(false)
        .connect()
        .await?)
}

pub async fn newer_schema(fixture: &BoardFixture, project: &str) -> FixtureResult<i64> {
    let mut connection = open(fixture, project).await?;
    let result = async {
        let previous = sqlx_turso::sqlx::query_scalar::<sqlx_turso::Turso, Option<i64>>(
            "SELECT MAX(version) FROM _sqlx_migrations",
        )
        .fetch_one(&mut connection)
        .await?
        .ok_or("owned fixture has no migration history")?;
        let changed = sqlx_turso::sqlx::query::<sqlx_turso::Turso>(
            "UPDATE _sqlx_migrations SET version=? WHERE version=?",
        )
        .bind(previous.checked_add(1).ok_or("schema version overflow")?)
        .bind(previous)
        .execute(&mut connection)
        .await?;
        assert_eq!(changed.rows_affected(), 1);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(previous)
    }
    .await;
    let closed = connection.close().await;
    finish_stopped_access(result, closed.map_err(Into::into))
}

pub async fn restore_schema(fixture: &BoardFixture, project: &str, previous: i64) -> FixtureResult {
    let mut connection = open(fixture, project).await?;
    let changed = sqlx_turso::sqlx::query::<sqlx_turso::Turso>(
        "UPDATE _sqlx_migrations SET version=? WHERE version=?",
    )
    .bind(previous)
    .bind(previous.checked_add(1).ok_or("schema version overflow")?)
    .execute(&mut connection)
    .await;
    let closed = connection.close().await;
    let changed = finish_stopped_access(changed.map_err(Into::into), closed.map_err(Into::into))?;
    assert_eq!(changed.rows_affected(), 1);
    Ok(())
}

pub async fn corrupt_watch(
    fixture: &BoardFixture,
    project: &str,
    thread: &str,
    reader: &Value,
) -> FixtureResult {
    let mut connection = open(fixture, project).await?;
    // This is the deliberate stored-boolean fault; no production constraint changes.
    let changed = async {
        sqlx_turso::sqlx::query::<sqlx_turso::Turso>("PRAGMA ignore_check_constraints=ON")
            .execute(&mut connection)
            .await?;
        sqlx_turso::sqlx::query::<sqlx_turso::Turso>(
            "UPDATE thread_watches SET active=2 WHERE thread_id=? AND reader_actor=(SELECT actor_id FROM actors WHERE canonical_identity=?)",
        )
        .bind(thread)
        .bind(serde_json::to_string(reader)?)
        .execute(&mut connection)
        .await
        .map_err(Into::<Box<dyn std::error::Error + Send + Sync>>::into)
    }
    .await;
    let closed = connection.close().await;
    let changed = finish_stopped_access(changed, closed.map_err(Into::into))?;
    assert_eq!(changed.rows_affected(), 1);
    Ok(())
}

pub(super) fn finish_stopped_access<Value>(
    operation: FixtureResult<Value>,
    closed: FixtureResult,
) -> FixtureResult<Value> {
    match (operation, closed) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(operation), Err(close)) => {
            Err(format!("stopped fixture access: {operation}; close: {close}").into())
        }
    }
}
