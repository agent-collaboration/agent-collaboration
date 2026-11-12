//! Final bookkeeping shape and injected write times through actual bootstrap transactions.
use super::*;
use crate::fleet_storage_fixture::{FixtureIo, FleetStorageFixture, NOW, TestError};
use fleet_sync_tokio_runner::TokioObject;
use sqlx_core::{query::query, row::Row};

type Store = TokioObject<crate::CoreHost<FixtureIo>>;
type TestResult<T = ()> = Result<T, TestError>;

async fn bootstrap(
    fixture: &FleetStorageFixture,
    schema: EngineSchema,
    now: runtime_primitives::SyncTime,
) -> TestResult<usize> {
    Ok(fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                let (applied, _) = <BootstrapStep as StepCallback<Store, sqlx_turso::Turso>>::run(
                    BootstrapStep {
                        schema,
                        now,
                        clock: Arc::new(FixedBootstrapClock(now)),
                    },
                    scope,
                )
                .await?;
                Ok(applied.count)
            })
        })
        .await?)
}

async fn columns(
    fixture: &FleetStorageFixture,
    table: &'static str,
) -> TestResult<Vec<(String, String, i64)>> {
    Ok(fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>(sqlx_core::sql_str::AssertSqlSafe(format!(
                    "PRAGMA table_info({table})"
                )))
                .fetch_all(scope.executor())
                .await
                .map_err(StoreError::backend)?
                .into_iter()
                .map(|row| {
                    Ok((
                        row.try_get("name").map_err(StoreError::backend)?,
                        row.try_get("type").map_err(StoreError::backend)?,
                        row.try_get("pk").map_err(StoreError::backend)?,
                    ))
                })
                .collect()
            })
        })
        .await?)
}

/// The record table of `schema`'s own migration set.
fn record_table(schema: EngineSchema) -> TestResult<&'static str> {
    Ok(crate::migration_records::MigrationSet::named(schema.corpus())?.table())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Fresh Fleet bootstrap must create sqlx's exact record table for its set, store the injected install time, and leave the failure table's shape alone."
)]
async fn fleet_bootstrap_creates_one_sqlx_record_table_per_set_with_injected_install_times()
-> TestResult {
    for schema in [EngineSchema::Fleet] {
        let fixture = FleetStorageFixture::open().await?;
        assert!(bootstrap(&fixture, schema, NOW).await? > 0);
        let table = record_table(schema)?;
        // sqlx's own DDL (sqlx-sqlite-0.9.0 src/migrate.rs:93–100), under the set's name.
        assert_eq!(
            columns(&fixture, table).await?,
            vec![
                ("version".into(), "BIGINT".into(), 1),
                ("description".into(), "TEXT".into(), 0),
                ("installed_on".into(), "TIMESTAMP".into(), 0),
                ("success".into(), "BOOLEAN".into(), 0),
                ("checksum".into(), "BLOB".into(), 0),
                ("execution_time".into(), "BIGINT".into(), 0),
            ],
            "{schema:?}"
        );
        assert!(
            columns(&fixture, "central_schema_migrations")
                .await?
                .is_empty(),
            "{schema:?} kept the custom record"
        );
        let failures = columns(&fixture, "central_schema_migration_failures").await?;
        assert_eq!(
            failures,
            vec![
                ("row_id".into(), "TEXT".into(), 1),
                ("corpus".into(), "TEXT".into(), 0),
                ("version".into(), "INTEGER".into(), 0),
                ("reason".into(), "TEXT".into(), 0),
                ("created_at".into(), "INTEGER".into(), 0),
                ("updated_at".into(), "INTEGER".into(), 0),
                ("failed_at".into(), "INTEGER".into(), 0),
            ],
            "{schema:?}"
        );
        let original = read_history(&fixture, table).await?;
        assert_eq!(original.len(), schema.migrations().len());
        for row in &original {
            assert!(row.version > 0);
            assert!(!row.checksum.is_empty());
            assert_eq!(
                row.installed_on,
                crate::migration_records::installed_on_text(NOW)?
            );
            assert_eq!(row.installed_class, "text");
        }
        let reopened = fixture.reopen().await?;
        assert_eq!(
            bootstrap(
                &reopened,
                schema,
                NOW.after(std::time::Duration::from_secs(1))
            )
            .await?,
            0
        );
        let repeated = read_history(&reopened, table).await?;
        assert_eq!(
            repeated, original,
            "owning reopen or no-op bootstrap rewrote bookkeeping"
        );
        reopened.close().await?;
    }
    Ok(())
}

#[derive(Debug, PartialEq)]
struct HistoryRow {
    version: i64,
    checksum: Vec<u8>,
    installed_on: String,
    installed_class: String,
}
async fn read_history(
    fixture: &FleetStorageFixture,
    table: &'static str,
) -> TestResult<Vec<HistoryRow>> {
    Ok(fixture.object.transaction(move |scope|Box::pin(async move {
        let rows=query::<sqlx_turso::Turso>(sqlx_core::sql_str::AssertSqlSafe(format!("SELECT CAST(version AS INTEGER) AS version,checksum,CAST(installed_on AS TEXT) AS installed_on,typeof(installed_on) AS installed_class FROM {table} ORDER BY version")))
            .fetch_all(scope.executor()).await.map_err(StoreError::backend)?;
        rows.into_iter().map(|row|Ok(HistoryRow {
            version:row.try_get("version").map_err(StoreError::backend)?,
            checksum:row.try_get("checksum").map_err(StoreError::backend)?,
            installed_on:row.try_get("installed_on").map_err(StoreError::backend)?,
            installed_class:row.try_get("installed_class").map_err(StoreError::backend)?,
        })).collect::<Result<Vec<_>,StoreError>>()
    })).await?)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Fresh owning bootstrap must persist the exact 48 SQLx SHA-384 bytes as a SQLite blob."
)]
async fn fresh_bootstrap_matches_sqlx_migration_column_types_and_checksum_bytes() -> TestResult {
    for schema in [EngineSchema::Fleet] {
        let fixture = FleetStorageFixture::open().await?;
        bootstrap(&fixture, schema, NOW).await?;

        let table = record_table(schema)?;
        fixture.object.transaction(move |scope|Box::pin(async move {
            let rows=query::<sqlx_turso::Turso>(sqlx_core::sql_str::AssertSqlSafe(format!("SELECT typeof(version) AS version_class,typeof(checksum) AS checksum_class FROM {table}"))).fetch_all(scope.executor()).await.map_err(StoreError::backend)?;
            assert!(!rows.is_empty());
            for row in rows {
                assert_eq!(row.try_get::<String,_>("version_class").map_err(StoreError::backend)?,"integer");
                assert_eq!(row.try_get::<String,_>("checksum_class").map_err(StoreError::backend)?,"blob");
            }
            Ok(())
        })).await?;
        let stored: Vec<(i64, Vec<u8>)> = fixture.object.transaction(move |scope| Box::pin(async move {
            let rows = query::<sqlx_turso::Turso>(sqlx_core::sql_str::AssertSqlSafe(format!(
                "SELECT CAST(version AS INTEGER) AS version,typeof(checksum) AS checksum_class,checksum,description,CAST(success AS INTEGER) AS success,typeof(success) AS success_class,CAST(installed_on AS TEXT) AS installed_on,typeof(installed_on) AS installed_class,CAST(execution_time AS INTEGER) AS execution_time,typeof(execution_time) AS execution_class FROM {table}",
            ))).fetch_all(scope.executor()).await.map_err(StoreError::backend)?;
            rows.into_iter().map(|row| {
                let class: String = row.try_get("checksum_class").map_err(StoreError::backend)?;
                assert_eq!(class, "blob", "{schema:?} checksum storage class");
                assert!(!row.try_get::<String,_>("description").map_err(StoreError::backend)?.is_empty());
                assert_eq!(row.try_get::<i64,_>("success").map_err(StoreError::backend)?,1);
                assert_eq!(row.try_get::<String,_>("installed_on").map_err(StoreError::backend)?, crate::migration_records::installed_on_text(NOW)?);
                assert_eq!(row.try_get::<String,_>("installed_class").map_err(StoreError::backend)?,"text");
                assert_eq!(row.try_get::<i64,_>("execution_time").map_err(StoreError::backend)?,0);
                for field in ["success_class","execution_class"] {assert_eq!(row.try_get::<String,_>(field).map_err(StoreError::backend)?,"integer");}
                Ok((row.try_get("version").map_err(StoreError::backend)?,row.try_get("checksum").map_err(StoreError::backend)?))
            }).collect::<Result<Vec<_>,StoreError>>()
        })).await?;
        let migrations = schema.migrations();
        assert_eq!(stored.len(), migrations.len());
        for (version, checksum) in stored {
            let migration = migrations
                .iter()
                .find(|migration| migration.version == version)
                .ok_or("stored migration is absent from SQLx corpus")?;
            assert_eq!(checksum.len(), 48, "{schema:?} migration {version}");
            assert_eq!(
                checksum,
                migration.checksum.as_ref(),
                "{schema:?} migration {version}"
            );
        }
        fixture.close().await?;
    }
    Ok(())
}

async fn record_failure(
    fixture: &FleetStorageFixture,
    version: u64,
    at: runtime_primitives::SyncTime,
) -> TestResult {
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                crate::migration_failure::record(
                    scope,
                    crate::migration_failure::MigrationFailure {
                        corpus: "bookkeeping-test".into(),
                        version,
                        reason: format!("migration {version} refused"),
                        at,
                        cause: StoreError::Aborted {
                            reason: "injected migration refusal".into(),
                        },
                    },
                )
                .await
            })
        })
        .await?;
    Ok(())
}

#[derive(Debug, PartialEq)]
struct FailureRow {
    row_id: String,
    version: i64,
    reason: String,
    created_at: i64,
    updated_at: i64,
    failed_at: i64,
    classes: (String, String, String),
}
async fn read_failure(fixture: &FleetStorageFixture) -> TestResult<FailureRow> {
    Ok(fixture.object.transaction(|scope|Box::pin(async move {
        let rows=query::<sqlx_turso::Turso>("SELECT row_id,version,reason,created_at,updated_at,failed_at,typeof(created_at) AS created_class,typeof(updated_at) AS updated_class,typeof(failed_at) AS failed_class FROM central_schema_migration_failures")
            .fetch_all(scope.executor()).await.map_err(StoreError::backend)?;
        let [row]=rows.as_slice() else {return Err(StoreError::backend("expected one latest failure per corpus"));};
        Ok(FailureRow {
            row_id:row.try_get("row_id").map_err(StoreError::backend)?,version:row.try_get("version").map_err(StoreError::backend)?,reason:row.try_get("reason").map_err(StoreError::backend)?,
            created_at:row.try_get("created_at").map_err(StoreError::backend)?,updated_at:row.try_get("updated_at").map_err(StoreError::backend)?,failed_at:row.try_get("failed_at").map_err(StoreError::backend)?,
            classes:(row.try_get("created_class").map_err(StoreError::backend)?,row.try_get("updated_class").map_err(StoreError::backend)?,row.try_get("failed_class").map_err(StoreError::backend)?),
        })
    })).await?)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Latest-per-corpus failure updates must preserve the original UUIDv7/creation time, use the supplied failure instant, and survive owning reopen."
)]
async fn latest_failure_keeps_identity_and_creation_but_advances_its_injected_failure_time()
-> TestResult {
    let fixture = FleetStorageFixture::open().await?;
    record_failure(&fixture, 1, NOW).await?;
    let first = read_failure(&fixture).await?;
    assert!(runtime_primitives::uuid_v7(durable_steps::Uuid::parse_str(
        &first.row_id
    )?));
    assert_eq!(
        (&first.version, &first.reason),
        (&1, &"migration 1 refused".to_owned())
    );
    assert_eq!(
        (first.created_at, first.updated_at, first.failed_at),
        (NOW.unix_millis(), NOW.unix_millis(), NOW.unix_millis())
    );
    assert_eq!(
        first.classes,
        ("integer".into(), "integer".into(), "integer".into())
    );
    let fixture = fixture.reopen().await?;
    assert_eq!(read_failure(&fixture).await?, first);
    let later = NOW.after(std::time::Duration::from_millis(1_234));
    record_failure(&fixture, 2, later).await?;
    let second = read_failure(&fixture).await?;
    assert_eq!(second.row_id, first.row_id);
    assert_eq!(second.created_at, first.created_at);
    assert_eq!(
        (second.updated_at, second.failed_at),
        (later.unix_millis(), later.unix_millis())
    );
    assert_eq!(
        (&second.version, &second.reason),
        (&2, &"migration 2 refused".to_owned())
    );
    assert_eq!(second.classes, first.classes);
    let fixture = fixture.reopen().await?;
    assert_eq!(read_failure(&fixture).await?, second);
    fixture.close().await
}

async fn sql(fixture: &FleetStorageFixture, statement: &'static str) -> TestResult {
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                scope
                    .executor()
                    .execute(statement)
                    .await
                    .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Actual bootstrap must refuse a record outside sqlx's shape."
)]
async fn corrupt_migration_records_refuse_bootstrap() -> TestResult {
    for (statement, field) in [
        (
            "UPDATE _sqlx_migrations_central_fleet SET installed_on=1.5",
            "_sqlx_migrations_central_fleet.installed_on",
        ),
        (
            "UPDATE _sqlx_migrations_central_fleet SET installed_on='yesterday'",
            "_sqlx_migrations_central_fleet.installed_on",
        ),
        (
            "UPDATE _sqlx_migrations_central_fleet SET success=0",
            "_sqlx_migrations_central_fleet.success",
        ),
        (
            "UPDATE _sqlx_migrations_central_fleet SET execution_time=-1",
            "_sqlx_migrations_central_fleet.execution_time",
        ),
    ] {
        let fixture = FleetStorageFixture::open().await?;
        bootstrap(&fixture, EngineSchema::Fleet, NOW).await?;
        sql(&fixture, statement).await?;
        let failure = bootstrap(&fixture, EngineSchema::Fleet, NOW)
            .await
            .err()
            .ok_or("a record outside sqlx's shape was accepted")?;
        assert!(
            matches!(failure.downcast_ref::<StoreError>(),Some(StoreError::Corrupt {field:actual,..}) if actual==field),
            "{failure:?}"
        );
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Migration execution time must be the actual difference between two injected clock samples around SQL execution."
)]
async fn migration_execution_time_uses_the_injected_clock_around_each_execute() -> TestResult {
    struct TickingClock(std::sync::atomic::AtomicI64);
    impl runtime_primitives::Clock for TickingClock {
        fn now(&self) -> runtime_primitives::SyncTime {
            runtime_primitives::SyncTime::from_unix_millis(
                self.0.fetch_add(7, std::sync::atomic::Ordering::SeqCst),
            )
        }
    }
    let fixture = FleetStorageFixture::open().await?;
    fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                <BootstrapStep as StepCallback<Store, sqlx_turso::Turso>>::run(
                    BootstrapStep {
                        schema: EngineSchema::Fleet,
                        now: NOW,
                        clock: Arc::new(TickingClock(std::sync::atomic::AtomicI64::new(
                            NOW.unix_millis(),
                        ))),
                    },
                    scope,
                )
                .await?;
                let rows = query::<sqlx_turso::Turso>(
                    "SELECT CAST(installed_on AS TEXT) AS installed_on,CAST(execution_time AS INTEGER) AS execution_time FROM _sqlx_migrations_central_fleet ORDER BY version",
                )
                .fetch_all(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                assert!(!rows.is_empty());
                for (index, row) in rows.into_iter().enumerate() {
                    assert_eq!(row.try_get::<i64, _>("execution_time").map_err(StoreError::backend)?, 7);
                    let offset = i64::try_from(index).map_err(StoreError::backend)? * 14 + 7;
                    let installed = crate::migration_records::installed_on_text(
                        runtime_primitives::SyncTime::from_unix_millis(NOW.unix_millis() + offset),
                    )?;
                    assert_eq!(row.try_get::<String, _>("installed_on").map_err(StoreError::backend)?, installed);
                }
                Ok(())
            })
        })
        .await?;
    fixture.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A backwards injected wall-clock step must record zero elapsed without aborting a successfully executed migration."
)]
async fn backwards_clock_records_zero_execution_time_and_commits_history() -> TestResult {
    struct BackwardsClock(std::sync::atomic::AtomicBool);
    impl runtime_primitives::Clock for BackwardsClock {
        fn now(&self) -> runtime_primitives::SyncTime {
            let stepped = self.0.swap(true, std::sync::atomic::Ordering::SeqCst);
            runtime_primitives::SyncTime::from_unix_millis(
                NOW.unix_millis() - i64::from(stepped) * 7,
            )
        }
    }
    let fixture = FleetStorageFixture::open().await?;
    fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                let (applied, _) = <BootstrapStep as StepCallback<Store, sqlx_turso::Turso>>::run(
                    BootstrapStep {
                        schema: EngineSchema::Fleet,
                        now: NOW,
                        clock: Arc::new(BackwardsClock(std::sync::atomic::AtomicBool::new(false))),
                    },
                    scope,
                )
                .await?;
                let rows = query::<sqlx_turso::Turso>(
            "SELECT CAST(installed_on AS TEXT) AS installed_on,CAST(success AS INTEGER) AS success,CAST(execution_time AS INTEGER) AS execution_time FROM _sqlx_migrations_central_fleet",
        ).fetch_all(scope.executor()).await.map_err(StoreError::backend)?;
                assert_eq!(rows.len(), EngineSchema::Fleet.migrations().len());
                assert_eq!(applied.count, rows.len());
                for row in rows {
                    assert_eq!(
                        row.try_get::<String, _>("installed_on")
                            .map_err(StoreError::backend)?,
                        crate::migration_records::installed_on_text(
                            runtime_primitives::SyncTime::from_unix_millis(NOW.unix_millis() - 7)
                        )?
                    );
                    assert_eq!(
                        row.try_get::<i64, _>("success")
                            .map_err(StoreError::backend)?,
                        1
                    );
                    assert_eq!(
                        row.try_get::<i64, _>("execution_time")
                            .map_err(StoreError::backend)?,
                        0
                    );
                }
                Ok(())
            })
        })
        .await?;
    // A separate transaction observes the committed rows, not just an uncommitted INSERT.
    assert!(
        !read_history(&fixture, "_sqlx_migrations_central_fleet")
            .await?
            .is_empty()
    );
    fixture.close().await
}
