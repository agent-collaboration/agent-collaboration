//! Final bookkeeping shape and injected write times through actual bootstrap transactions.
use super::*;
use crate::fleet_storage_fixture::{FixtureIo, FleetStorageFixture, NOW, TestError};
use fleet_tokio_objects::TokioObject;
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
                let sql = match table {
                    "central_schema_migrations" => "PRAGMA table_info(central_schema_migrations)",
                    "central_schema_migration_failures" => {
                        "PRAGMA table_info(central_schema_migration_failures)"
                    }
                    _ => return Err(StoreError::backend("unexpected bookkeeping table")),
                };
                query::<sqlx_turso::Turso>(sql)
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

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Both fresh bootstrap roles must create the exact owner-approved bookkeeping columns and store injected integer times without an old at alias."
)]
async fn fleet_and_copy_bootstrap_create_final_bookkeeping_with_integer_injected_times()
-> TestResult {
    for schema in [EngineSchema::Fleet, EngineSchema::Copy] {
        let fixture = FleetStorageFixture::open().await?;
        assert!(bootstrap(&fixture, schema, NOW).await? > 0);
        let history = columns(&fixture, "central_schema_migrations").await?;
        assert_eq!(
            history,
            vec![
                ("row_id".into(), "TEXT".into(), 1),
                ("corpus".into(), "TEXT".into(), 0),
                ("version".into(), "INTEGER".into(), 0),
                ("description".into(), "TEXT".into(), 0),
                ("installed_on".into(), "INTEGER".into(), 0),
                ("success".into(), "INTEGER".into(), 0),
                ("checksum".into(), "BLOB".into(), 0),
                ("execution_time".into(), "INTEGER".into(), 0),
                ("created_at".into(), "INTEGER".into(), 0),
                ("updated_at".into(), "INTEGER".into(), 0),
            ],
            "{schema:?}"
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
        let original = read_history(&fixture).await?;
        assert!(!original.is_empty());
        for row in &original {
            assert!(runtime_primitives::uuid_v7(durable_steps::Uuid::parse_str(
                &row.row_id
            )?));
            assert_eq!(
                durable_steps::Uuid::parse_str(&row.row_id)?.to_string(),
                row.row_id
            );
            assert!(!row.corpus.is_empty());
            assert!(row.version > 0);
            assert!(!row.checksum.is_empty());
            assert_eq!(
                (row.created_at, row.updated_at, row.installed_on),
                (NOW.unix_millis(), NOW.unix_millis(), NOW.unix_millis())
            );
            assert_eq!(
                (
                    row.created_class.as_str(),
                    row.updated_class.as_str(),
                    row.installed_class.as_str()
                ),
                ("integer", "integer", "integer")
            );
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
        let repeated = read_history(&reopened).await?;
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
    row_id: String,
    corpus: String,
    version: i64,
    checksum: Vec<u8>,
    created_at: i64,
    updated_at: i64,
    installed_on: i64,
    created_class: String,
    updated_class: String,
    installed_class: String,
}
async fn read_history(fixture: &FleetStorageFixture) -> TestResult<Vec<HistoryRow>> {
    Ok(fixture.object.transaction(|scope|Box::pin(async move {
        let rows=query::<sqlx_turso::Turso>("SELECT row_id,corpus,version,checksum,created_at,updated_at,installed_on,typeof(created_at) AS created_class,typeof(updated_at) AS updated_class,typeof(installed_on) AS installed_class FROM central_schema_migrations ORDER BY corpus,version")
            .fetch_all(scope.executor()).await.map_err(StoreError::backend)?;
        rows.into_iter().map(|row|Ok(HistoryRow {
            row_id:row.try_get("row_id").map_err(StoreError::backend)?,
            corpus:row.try_get("corpus").map_err(StoreError::backend)?,
            version:row.try_get("version").map_err(StoreError::backend)?,
            checksum:row.try_get("checksum").map_err(StoreError::backend)?,
            created_at:row.try_get("created_at").map_err(StoreError::backend)?,
            updated_at:row.try_get("updated_at").map_err(StoreError::backend)?,
            installed_on:row.try_get("installed_on").map_err(StoreError::backend)?,
            created_class:row.try_get("created_class").map_err(StoreError::backend)?,
            updated_class:row.try_get("updated_class").map_err(StoreError::backend)?,
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
    for schema in [EngineSchema::Fleet, EngineSchema::Copy] {
        let fixture = FleetStorageFixture::open().await?;
        bootstrap(&fixture, schema, NOW).await?;

        fixture.object.transaction(move |scope|Box::pin(async move {
            let rows=query::<sqlx_turso::Turso>("SELECT typeof(version) AS version_class,typeof(checksum) AS checksum_class FROM central_schema_migrations WHERE corpus=?1").bind(schema.corpus()).fetch_all(scope.executor()).await.map_err(StoreError::backend)?;
            assert!(!rows.is_empty());
            for row in rows {
                assert_eq!(row.try_get::<String,_>("version_class").map_err(StoreError::backend)?,"integer");
                assert_eq!(row.try_get::<String,_>("checksum_class").map_err(StoreError::backend)?,"blob");
            }
            Ok(())
        })).await?;
        let stored: Vec<(i64, Vec<u8>)> = fixture.object.transaction(move |scope| Box::pin(async move {
            let rows = query::<sqlx_turso::Turso>(
                "SELECT version,typeof(checksum) AS checksum_class,checksum,description,success,typeof(success) AS success_class,installed_on,typeof(installed_on) AS installed_class,execution_time,typeof(execution_time) AS execution_class FROM central_schema_migrations WHERE corpus=?1",
            ).bind(schema.corpus()).fetch_all(scope.executor()).await.map_err(StoreError::backend)?;
            rows.into_iter().map(|row| {
                let class: String = row.try_get("checksum_class").map_err(StoreError::backend)?;
                assert_eq!(class, "blob", "{schema:?} checksum storage class");
                assert!(!row.try_get::<String,_>("description").map_err(StoreError::backend)?.is_empty());
                assert_eq!(row.try_get::<i64,_>("success").map_err(StoreError::backend)?,1);
                assert_eq!(row.try_get::<i64,_>("installed_on").map_err(StoreError::backend)?, NOW.unix_millis());
                assert_eq!(row.try_get::<i64,_>("execution_time").map_err(StoreError::backend)?,0);
                for field in ["success_class","installed_class","execution_class"] {assert_eq!(row.try_get::<String,_>(field).map_err(StoreError::backend)?,"integer");}
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
    reason = "Actual bootstrap and tolerant Copy health must reject stored REAL/TEXT time corruption rather than coerce the cast text to a usable integer."
)]
async fn corrupt_native_bookkeeping_times_refuse_bootstrap_or_make_health_explicitly_unreadable()
-> TestResult {
    for (statement, field) in [
        (
            "UPDATE central_schema_migrations SET created_at=x'31'",
            "central_schema_migrations.created_at",
        ),
        (
            "UPDATE central_schema_migrations SET updated_at='bad'",
            "central_schema_migrations.updated_at",
        ),
        (
            "UPDATE central_schema_migrations SET installed_on=1.5",
            "central_schema_migrations.installed_on",
        ),
    ] {
        let fixture = FleetStorageFixture::open().await?;
        bootstrap(&fixture, EngineSchema::Fleet, NOW).await?;
        sql(&fixture, statement).await?;
        let failure = bootstrap(&fixture, EngineSchema::Fleet, NOW)
            .await
            .err()
            .ok_or("invalid native timestamp was accepted")?;
        assert!(
            matches!(failure.downcast_ref::<StoreError>(),Some(StoreError::Corrupt {field:actual,..}) if actual==field),
            "{failure:?}"
        );
        fixture.close().await?;
    }
    for (statement, field) in [
        (
            "UPDATE central_schema_migration_failures SET created_at=x'31'",
            "central_schema_migration_failures.created_at",
        ),
        (
            "UPDATE central_schema_migration_failures SET updated_at='bad'",
            "central_schema_migration_failures.updated_at",
        ),
        (
            "UPDATE central_schema_migration_failures SET failed_at=1.5",
            "central_schema_migration_failures.failed_at",
        ),
    ] {
        let fixture = FleetStorageFixture::open().await?;
        bootstrap(&fixture, EngineSchema::Copy, NOW).await?;
        record_failure(&fixture, 1, NOW).await?;
        sql(&fixture, statement).await?;
        let health = fixture
            .object
            .transaction(|scope| {
                Box::pin(async move {
                    let (health, _) = <crate::copy_health::ReadCopyHealth as StepCallback<
                        Store,
                        sqlx_turso::Turso,
                    >>::run(
                        crate::copy_health::ReadCopyHealth { storage_bytes: 0 },
                        scope,
                    )
                    .await?;
                    Ok(health)
                })
            })
            .await?;
        let collaboration_central_admin_protocol::CopyHealth::Live(health) = health else {
            return Err("fresh Copy health was not live".into());
        };
        assert!(
            matches!(health.migration_failures,collaboration_central_admin_protocol::CopyHealthRead::Unreadable {detail} if detail.contains(field))
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
                    // The Fleet corpus runs first; central-work follows it in the same step.
                    "SELECT installed_on,execution_time FROM central_schema_migrations WHERE corpus='central-fleet' ORDER BY version",
                )
                .fetch_all(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                assert!(!rows.is_empty());
                for (index, row) in rows.into_iter().enumerate() {
                    assert_eq!(row.try_get::<i64, _>("execution_time").map_err(StoreError::backend)?, 7);
                    let offset = i64::try_from(index).map_err(StoreError::backend)? * 14 + 7;
                    assert_eq!(row.try_get::<i64, _>("installed_on").map_err(StoreError::backend)?, NOW.unix_millis() + offset);
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
    fixture.object.transaction(|scope| Box::pin(async move {
        let (applied, _) = <BootstrapStep as StepCallback<Store, sqlx_turso::Turso>>::run(
            BootstrapStep {
                schema: EngineSchema::Fleet,
                now: NOW,
                clock: Arc::new(BackwardsClock(std::sync::atomic::AtomicBool::new(false))),
            }, scope,
        ).await?;
        let rows = query::<sqlx_turso::Turso>(
            "SELECT installed_on,success,execution_time FROM central_schema_migrations WHERE corpus='central-fleet'",
        ).fetch_all(scope.executor()).await.map_err(StoreError::backend)?;
        assert_eq!(rows.len(), EngineSchema::Fleet.migrations().len());
        // The step also applies the central-work corpus that Fleet carries.
        assert_eq!(applied.count, rows.len() + super::WORK_MIGRATIONS.len());
        for row in rows {
            assert_eq!(row.try_get::<i64,_>("installed_on").map_err(StoreError::backend)?, NOW.unix_millis() - 7);
            assert_eq!(row.try_get::<i64,_>("success").map_err(StoreError::backend)?, 1);
            assert_eq!(row.try_get::<i64,_>("execution_time").map_err(StoreError::backend)?, 0);
        }
        Ok(())
    })).await?;
    // A separate transaction observes the committed rows, not just an uncommitted INSERT.
    assert!(!read_history(&fixture).await?.is_empty());
    fixture.close().await
}
