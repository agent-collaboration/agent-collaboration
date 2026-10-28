//! Controlled SQLITE_FULL at the summary executor, after actual schema writes.
//! This checks typed error propagation, not physical disk exhaustion.
use super::*;
use durable_steps::storage::TransactionScope;
use fleet_sync_protocol::{CopyUnavailableReason, PeerId};
use futures_util::{
    future::BoxFuture,
    stream::{self, BoxStream},
};
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    database::Database,
    error::{DatabaseError, ErrorKind},
    executor::{Execute, Executor},
    sql_str::SqlStr,
};
use sqlx_turso::{Turso, TursoConnection};
use std::{borrow::Cow, error::Error};

#[derive(Debug, thiserror::Error)]
#[error("controlled summary storage capacity failure")]
struct CapacityError;
impl DatabaseError for CapacityError {
    fn message(&self) -> &str {
        "controlled summary storage capacity failure"
    }
    fn code(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed("SQLITE_FULL"))
    }
    fn as_error(&self) -> &(dyn Error + Send + Sync + 'static) {
        self
    }
    fn as_error_mut(&mut self) -> &mut (dyn Error + Send + Sync + 'static) {
        self
    }
    fn into_error(self: Box<Self>) -> Box<dyn Error + Send + Sync + 'static> {
        self
    }
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}
struct CapturedQuery {
    sql: SqlStr,
    arguments: Option<<Turso as Database>::Arguments>,
    persistent: bool,
}
impl<'q> Execute<'q, Turso> for CapturedQuery {
    fn sql(self) -> SqlStr {
        self.sql
    }
    fn statement(&self) -> Option<&<Turso as Database>::Statement> {
        None
    }
    fn take_arguments(
        &mut self,
    ) -> Result<Option<<Turso as Database>::Arguments>, sqlx_core::error::BoxDynError> {
        Ok(self.arguments.take())
    }
    fn persistent(&self) -> bool {
        self.persistent
    }
}
#[derive(Debug)]
struct CapacityExecutor<'c>(&'c mut TursoConnection);
impl<'c> Executor<'c> for CapacityExecutor<'c> {
    type Database = Turso;
    fn fetch_many<'e, 'q: 'e, E>(
        self,
        mut query: E,
    ) -> BoxStream<
        'e,
        Result<
            sqlx_turso::sqlx::Either<<Turso as Database>::QueryResult, <Turso as Database>::Row>,
            sqlx_core::Error,
        >,
    >
    where
        'c: 'e,
        E: 'q + Execute<'q, Turso>,
    {
        let arguments = query.take_arguments().map_err(sqlx_core::Error::Encode);
        let persistent = query.persistent();
        let sql = query.sql();
        if sql
            .as_str()
            .trim_start()
            .starts_with("INSERT INTO _copy_summary")
        {
            return Box::pin(stream::once(async {
                Err(sqlx_core::Error::Database(Box::new(CapacityError)))
            }));
        }
        match arguments {
            Ok(arguments) => self.0.fetch_many(CapturedQuery {
                sql,
                arguments,
                persistent,
            }),
            Err(error) => Box::pin(stream::once(async move { Err(error) })),
        }
    }
    fn fetch_optional<'e, 'q: 'e, E>(
        self,
        query: E,
    ) -> BoxFuture<'e, Result<Option<<Turso as Database>::Row>, sqlx_core::Error>>
    where
        'c: 'e,
        E: 'q + Execute<'q, Turso>,
    {
        self.0.fetch_optional(query)
    }
    fn prepare_with<'e>(
        self,
        sql: SqlStr,
        parameters: &'e [<Turso as Database>::TypeInfo],
    ) -> BoxFuture<'e, Result<<Turso as Database>::Statement, sqlx_core::Error>>
    where
        'c: 'e,
    {
        self.0.prepare_with(sql, parameters)
    }
    fn describe<'e>(
        self,
        sql: SqlStr,
    ) -> BoxFuture<'e, Result<sqlx_core::describe::Describe<Turso>, sqlx_core::Error>>
    where
        'c: 'e,
    {
        self.0.describe(sql)
    }
}
struct CapacityScope<'c>(&'c mut TursoConnection);
impl TransactionScope for CapacityScope<'_> {
    type Database = Turso;
    type Executor<'q>
        = CapacityExecutor<'q>
    where
        Self: 'q;
    fn executor(&mut self) -> Self::Executor<'_> {
        CapacityExecutor(self.0)
    }
}
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn summary_storage_full_is_preserved_as_copy_unavailable_after_real_rollback() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    fixture.create(false).await?;
    let piece = schema_piece(fixture.database)?;
    fixture.objects.close().await?;
    drop(fixture);
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.path().join("copy.db"))
        .connect()
        .await?;
    let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
    let rules = collaboration_central_core::CopyReceiverRules::new(
        runtime_primitives::SyncTime::from_unix_millis(1_792_281_600_000),
        None,
    )?;
    let error = fleet_sync_receiver::apply_piece(
        &mut CapacityScope(&mut transaction),
        &piece,
        &PeerId::try_from("019f0000-0000-7000-8000-000000000901".to_owned())?,
        &rules,
        runtime_primitives::SyncTime::from_unix_millis(1_792_281_600_000),
    )
    .await
    .err()
    .ok_or("the summary executor must fail")?;
    let marker = fleet_sync_receiver::apply_rollback(error);
    transaction.rollback().await?;
    let answer = fleet_sync_receiver::classify_copy_store_error(marker).answer();
    let row = sqlx_core::query::query::<Turso>(
        "SELECT count(*) AS tables FROM sqlite_schema WHERE name='board_rows'",
    )
    .fetch_one(&mut connection)
    .await?;
    use sqlx_core::row::Row;
    assert_eq!(row.try_get::<i64, _>("tables")?, 0);
    connection.close().await?;
    assert_eq!(
        answer,
        ReplicationAnswer::CopyUnavailable {
            reason: CopyUnavailableReason::StorageFull
        }
    );
    // Capacity is restored by removing only the executor's controlled failure.
    // Retry the identical piece through the normal Copy runtime and journal.
    let restored = Fixture::open(root.path()).await?;
    let proof = async {
        assert_eq!(restored.counts().await?, (0, 0, 0));
        assert_eq!(restored.runtime.copy_summary().await?, None);
        assert_eq!(
            restored.apply(piece.clone()).await?,
            ReplicationAnswer::Applied {
                through: position(1)?
            }
        );
        assert_eq!(restored.counts().await?, (1, 1, 1));
        let summary = restored.runtime.copy_summary().await?;
        assert!(summary.is_some());
        assert_eq!(
            restored.apply(piece).await?,
            ReplicationAnswer::AlreadyApplied {
                through: position(1)?
            }
        );
        assert_eq!(restored.counts().await?, (1, 1, 1));
        assert_eq!(restored.runtime.copy_summary().await?, summary);
        restored.runtime.on_alarm().await?;
        assert_eq!(
            restored.summary(true).await?,
            restored.summary(false).await?
        );
        assert_eq!(
            restored
                .io
                .calls
                .lock()
                .map_err(|_| "calls poisoned")?
                .len(),
            1
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    restored.objects.close().await?;
    proof
}
