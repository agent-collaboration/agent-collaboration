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
                    BootstrapStep { schema, now },
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
                ("version".into(), "TEXT".into(), 0),
                ("checksum".into(), "TEXT".into(), 0),
                ("created_at".into(), "INTEGER".into(), 0),
                ("updated_at".into(), "INTEGER".into(), 0),
                ("applied_at".into(), "INTEGER".into(), 0),
            ],
            "{schema:?}"
        );
        let failures = columns(&fixture, "central_schema_migration_failures").await?;
        assert_eq!(
            failures,
            vec![
                ("row_id".into(), "TEXT".into(), 1),
                ("corpus".into(), "TEXT".into(), 0),
                ("version".into(), "TEXT".into(), 0),
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
            assert_eq!(row.version.parse::<u64>()?.to_string(), row.version);
            assert!(!row.checksum.is_empty());
            assert_eq!(
                (row.created_at, row.updated_at, row.applied_at),
                (NOW.unix_millis(), NOW.unix_millis(), NOW.unix_millis())
            );
            assert_eq!(
                (
                    row.created_class.as_str(),
                    row.updated_class.as_str(),
                    row.applied_class.as_str()
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
    version: String,
    checksum: String,
    created_at: i64,
    updated_at: i64,
    applied_at: i64,
    created_class: String,
    updated_class: String,
    applied_class: String,
}
async fn read_history(fixture: &FleetStorageFixture) -> TestResult<Vec<HistoryRow>> {
    Ok(fixture.object.transaction(|scope|Box::pin(async move {
        let rows=query::<sqlx_turso::Turso>("SELECT row_id,corpus,version,checksum,created_at,updated_at,applied_at,typeof(created_at) AS created_class,typeof(updated_at) AS updated_class,typeof(applied_at) AS applied_class FROM central_schema_migrations ORDER BY corpus,version")
            .fetch_all(scope.executor()).await.map_err(StoreError::backend)?;
        rows.into_iter().map(|row|Ok(HistoryRow {
            row_id:row.try_get("row_id").map_err(StoreError::backend)?,
            corpus:row.try_get("corpus").map_err(StoreError::backend)?,
            version:row.try_get("version").map_err(StoreError::backend)?,
            checksum:row.try_get("checksum").map_err(StoreError::backend)?,
            created_at:row.try_get("created_at").map_err(StoreError::backend)?,
            updated_at:row.try_get("updated_at").map_err(StoreError::backend)?,
            applied_at:row.try_get("applied_at").map_err(StoreError::backend)?,
            created_class:row.try_get("created_class").map_err(StoreError::backend)?,
            updated_class:row.try_get("updated_class").map_err(StoreError::backend)?,
            applied_class:row.try_get("applied_class").map_err(StoreError::backend)?,
        })).collect::<Result<Vec<_>,StoreError>>()
    })).await?)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Fresh owning bootstrap must persist the exact SQLx SHA-384 checksum as lowercase hex, never a JSON byte array."
)]
async fn fresh_bootstrap_stores_sqlx_checksums_as_96_lowercase_hex_characters() -> TestResult {
    for schema in [EngineSchema::Fleet, EngineSchema::Copy] {
        let fixture = FleetStorageFixture::open().await?;
        bootstrap(&fixture, schema, NOW).await?;

        let history = read_history(&fixture).await?;
        let migrations = schema.migrations();
        let stored = history
            .iter()
            .filter(|row| row.corpus == schema.corpus())
            .collect::<Vec<_>>();
        assert_eq!(stored.len(), migrations.len());
        for row in stored {
            let migration = migrations
                .iter()
                .find(|migration| migration.version.to_string() == row.version)
                .ok_or("stored migration is absent from SQLx corpus")?;
            assert_eq!(row.checksum.len(), 96, "{schema:?}: {row:?}");
            assert!(
                row.checksum
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                "{schema:?}: {row:?}"
            );
            let expected = migration
                .checksum
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            assert_eq!(
                row.checksum, expected,
                "{schema:?} migration {}",
                row.version
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
    version: String,
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
        (&"1".to_owned(), &"migration 1 refused".to_owned())
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
        (&"2".to_owned(), &"migration 2 refused".to_owned())
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
            "UPDATE central_schema_migrations SET applied_at=1.5",
            "central_schema_migrations.applied_at",
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
