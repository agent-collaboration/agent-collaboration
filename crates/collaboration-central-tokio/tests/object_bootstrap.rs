#![cfg(feature = "conformance")]
#![allow(clippy::panic_in_result_fn)]

use collaboration_central_core::{
    BootstrapCommand, BootstrapProbe, BootstrapProbeReply, BootstrapReply, CopyKind, CoreHost,
    EngineSchema, ObjectAddress, ObjectBootstrap, ObjectRuntime, PlatformIo,
};
use collaboration_central_tokio::TokioStorageHost;
use durable_steps::{
    AtomicStore, CallFailure, NotSentReason, StoreError, storage::TransactionScope,
};
use fleet_sync_protocol::{
    CopyStatusAnswer, CopyUnavailableReason, DatabaseId, ReceiverId, ReplicationAnswer,
};
use fleet_sync_tokio_runner::{ObjectKey, ObjectStore, TokioObject};
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use sqlx_core::{executor::Executor, query::query, row::Row};
use std::{path::Path, sync::Arc};

type TestError = Box<dyn std::error::Error + Send + Sync>;
type NativeObject = TokioObject<CoreHost<ProbeIo>>;
type NativeHost = TokioStorageHost<CoreHost<ProbeIo>>;

struct ProbeIo;
impl Clock for ProbeIo {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(1_792_281_600_000)
    }
}
impl IdSource for ProbeIo {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::now_v7()
    }
}
impl WakeScheduler for ProbeIo {
    async fn wake_at(&self, _at: SyncTime) -> Result<(), WakeFailure> {
        Ok(())
    }
}
impl PlatformIo for ProbeIo {
    async fn object_call(
        &self,
        _to: ObjectAddress,
        _body: Vec<u8>,
    ) -> Result<Vec<u8>, CallFailure> {
        Err(CallFailure::NotSent {
            reason: NotSentReason::Overloaded,
        })
    }
}
struct Fixture {
    objects: ObjectStore<CoreHost<ProbeIo>>,
    host: Arc<NativeHost>,
    object: Arc<NativeObject>,
}
impl Fixture {
    async fn open(path: &Path) -> Result<Self, TestError> {
        let objects = ObjectStore::open(path).await?;
        let shared = Arc::new(CoreHost::conformance_registry(
            ProbeIo,
            ReceiverId::new(durable_steps::Uuid::now_v7())?,
        ));
        let object = objects
            .object(ObjectKey::new("bootstrap")?, Default::default(), shared)
            .await?;
        let host = Arc::new(TokioStorageHost::new(object.clone()));
        Ok(Self {
            objects,
            host,
            object,
        })
    }
    async fn tables(&self) -> Result<Vec<String>, StoreError> {
        self.object
            .transaction(|scope| {
                Box::pin(async move {
                    query::<sqlx_turso::Turso>(
                        "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
                    )
                    .fetch_all(scope.executor())
                    .await
                    .map_err(StoreError::backend)?
                    .into_iter()
                    .map(|row| row.try_get("name").map_err(StoreError::backend))
                    .collect()
                })
            })
            .await
    }
}

#[tokio::test]
async fn exported_engine_corpora_initialize_once_and_reopen_without_reapplying_the_alter()
-> Result<(), TestError> {
    for (schema, expected) in [
        (EngineSchema::Registry, 8),
        (EngineSchema::Copy, 3),
        (EngineSchema::Session, 4),
        (EngineSchema::Mailbox, 3),
        (EngineSchema::Fleet, 10),
    ] {
        let directory = tempfile::tempdir()?;
        let fixture = Fixture::open(directory.path()).await?;
        let bootstrap = ObjectBootstrap::new(fixture.host.clone(), ProbeIo.now());
        assert_eq!(bootstrap.initialize(schema).await?, expected);
        assert_eq!(bootstrap.initialize(schema).await?, 0);
        let probe = match schema {
            EngineSchema::Registry => Some(BootstrapProbe::Registry),
            EngineSchema::Copy => Some(BootstrapProbe::Copy),
            EngineSchema::Mailbox => Some(BootstrapProbe::Mailbox),
            EngineSchema::Session | EngineSchema::Fleet => None,
        };
        if let Some(schema) = probe {
            let BootstrapReply::Probed(reply) = bootstrap
                .execute(BootstrapCommand::Probe { schema })
                .await?
            else {
                return Err("library readback missing".into());
            };
            match *reply {
                BootstrapProbeReply::Registry(report) => {
                    assert_eq!(
                        report.peer.to_string(),
                        "019f0000-0000-7000-8000-000000000601"
                    );
                    assert_eq!(report.attach_seq.value(), 1);
                    assert_eq!(report.support.max_version, 7);
                }
                BootstrapProbeReply::Copy(answer) => assert!(
                    matches!(*answer, CopyStatusAnswer::Status(status) if status.fence.value() == 1 && status.acked.sequence.value() == 0)
                ),
                BootstrapProbeReply::Mailbox(query) => assert!(
                    query.statuses.iter().all(|status| matches!(
                        status.state,
                        fleet_sync_protocol::ItemStatusState::Unknown
                    )) && query.statuses.len() == 1
                ),
            }
        }
        let count: String = fixture
            .object
            .transaction(|scope| {
                Box::pin(async move {
                    query::<sqlx_turso::Turso>(
                        "SELECT CAST(COUNT(*) AS TEXT) AS count FROM central_schema_migrations",
                    )
                    .fetch_one(scope.executor())
                    .await
                    .map_err(StoreError::backend)?
                    .try_get("count")
                    .map_err(StoreError::backend)
                })
            })
            .await?;
        assert_eq!(count, expected.to_string());
        fixture.objects.close().await?;
        drop(bootstrap);
        drop(fixture);

        let reopened = Fixture::open(directory.path()).await?;
        assert_eq!(
            ObjectBootstrap::new(reopened.host.clone(), ProbeIo.now())
                .initialize(schema)
                .await?,
            0
        );
        reopened.objects.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn a_real_late_sql_error_rolls_back_all_earlier_migrations_and_history()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path()).await?;
    // A declared public migration creates the conflicting tables. Bootstrap's
    // first two copy migrations then run before the third hits that conflict.
    fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                let conflict = fleet_sync_receiver::RECEIVER_MIGRATOR
                    .iter()
                    .find(|migration| migration.version == 3)
                    .ok_or_else(|| StoreError::Aborted {
                        reason: "snapshot-install migration absent".into(),
                    })?;
                scope
                    .executor()
                    .execute(conflict.sql.as_ref())
                    .await
                    .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await?;
    let before = fixture.tables().await?;

    let result = ObjectBootstrap::new(fixture.host.clone(), ProbeIo.now())
        .initialize(EngineSchema::Copy)
        .await;
    let after = fixture.tables().await?;
    let failures = fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>(
                    "SELECT corpus,version,reason,at FROM central_schema_migration_failures",
                )
                .fetch_all(scope.executor())
                .await
                .map_err(StoreError::backend)?
                .into_iter()
                .map(|row| {
                    Ok((
                        row.try_get::<String, _>("corpus")
                            .map_err(StoreError::backend)?,
                        row.try_get::<String, _>("version")
                            .map_err(StoreError::backend)?,
                        row.try_get::<String, _>("reason")
                            .map_err(StoreError::backend)?,
                        row.try_get::<String, _>("at")
                            .map_err(StoreError::backend)?,
                    ))
                })
                .collect::<Result<Vec<_>, StoreError>>()
            })
        })
        .await?;
    fixture.objects.close().await?;

    assert!(
        result.is_err(),
        "an unregistered existing engine table was adopted"
    );
    let mut expected = before;
    expected.push("central_schema_migration_failures".into());
    expected.sort();
    assert_eq!(
        after, expected,
        "late failure leaked earlier schema or migration history"
    );
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].0, "fleet-sync-receiver");
    assert_eq!(failures[0].1, "3");
    assert!(failures[0].2.contains("_fleet_sync_install"));
    assert_eq!(failures[0].3, ProbeIo.now().unix_millis().to_string());
    Ok(())
}

#[tokio::test]
async fn a_changed_migration_checksum_is_refused_on_reopen() -> Result<(), TestError> {
    corrupt_history("UPDATE central_schema_migrations SET checksum='[]' WHERE version='1'").await
}
#[tokio::test]
async fn a_noncanonical_stored_version_is_refused_on_reopen() -> Result<(), TestError> {
    corrupt_history("UPDATE central_schema_migrations SET version='01' WHERE version='1'").await
}
#[tokio::test]
async fn an_unknown_stored_version_is_refused_on_reopen() -> Result<(), TestError> {
    corrupt_history("UPDATE central_schema_migrations SET version='999' WHERE version='1'").await
}
async fn corrupt_history(sql: &'static str) -> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path()).await?;
    ObjectBootstrap::new(fixture.host.clone(), ProbeIo.now())
        .initialize(EngineSchema::Registry)
        .await?;
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                scope
                    .executor()
                    .execute(sql)
                    .await
                    .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await?;
    fixture.objects.close().await?;
    drop(fixture);
    let fixture = Fixture::open(directory.path()).await?;

    let result = ObjectBootstrap::new(fixture.host.clone(), ProbeIo.now())
        .initialize(EngineSchema::Registry)
        .await;
    fixture.objects.close().await?;

    assert!(
        matches!(result, Err(StoreError::Corrupt { field, .. }) if field == "schema.migration")
    );
    Ok(())
}

#[tokio::test]
async fn receiver_rollback_is_unavailable_but_a_failed_store_boundary_is_unknown()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path()).await?;
    let database = DatabaseId::new(durable_steps::Uuid::now_v7())?;
    let runtime = ObjectRuntime::<CopyKind, _, ProbeIo>::copy(
        fixture.host.clone(),
        ProbeIo,
        ReceiverId::new(durable_steps::Uuid::now_v7())?,
        database,
        std::time::Duration::from_secs(2),
    );

    // No receiver schema is installed: the real receiver query fails and the
    // real store rolls it back. No receiver or SQL statement is mocked.
    let failed = runtime
        .read_copy_status()
        .await
        .err()
        .ok_or("missing receiver schema was accepted")?
        .answer();
    assert!(matches!(
        failed,
        ReplicationAnswer::CopyUnavailable {
            reason: CopyUnavailableReason::Damaged { .. }
        }
    ));
    ObjectBootstrap::new(fixture.host.clone(), ProbeIo.now())
        .initialize(EngineSchema::Copy)
        .await?;
    assert_eq!(
        runtime
            .read_copy_status()
            .await
            .map_err(|error| format!("{error:?}"))?,
        CopyStatusAnswer::NotInitialized
    );

    fixture.objects.close().await?;
    let closed = runtime
        .read_copy_status()
        .await
        .err()
        .ok_or("closed store was accepted")?
        .answer();
    assert!(
        matches!(closed, ReplicationAnswer::OutcomeUnknown { .. }),
        "failed begin was promoted to no-effect: {closed:?}"
    );
    Ok(())
}

#[path = "object_bootstrap/owner_migrate.rs"]
mod owner_migrate;

#[tokio::test]
async fn central_migration_metadata_has_physical_uuid_primary_keys() -> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    ObjectBootstrap::new(fixture.host.clone(), ProbeIo.now())
        .initialize(EngineSchema::Copy)
        .await?;
    let identities = history_ids(&fixture).await?;
    for identity in &identities {
        assert_eq!(
            durable_steps::Uuid::parse_str(identity)?.get_version_num(),
            7
        );
    }
    assert!(!identities.is_empty());
    assert_eq!(
        ObjectBootstrap::new(fixture.host.clone(), ProbeIo.now())
            .initialize(EngineSchema::Copy)
            .await?,
        0
    );
    assert_eq!(history_ids(&fixture).await?, identities);
    let keys=fixture.object.transaction(|scope|Box::pin(async move {
        let history=sqlx_core::query::query::<sqlx_turso::Turso>("SELECT name FROM pragma_table_info('central_schema_migrations') WHERE pk>0 ORDER BY pk").fetch_all(scope.executor()).await.map_err(StoreError::backend)?;
        let failures=sqlx_core::query::query::<sqlx_turso::Turso>("SELECT name FROM pragma_table_info('central_schema_migration_failures') WHERE pk>0 ORDER BY pk").fetch_all(scope.executor()).await.map_err(StoreError::backend)?;
        Ok((history.into_iter().map(|row|row.try_get::<String,_>("name")).collect::<Result<Vec<_>,_>>().map_err(StoreError::backend)?, failures.into_iter().map(|row|row.try_get::<String,_>("name")).collect::<Result<Vec<_>,_>>().map_err(StoreError::backend)?))
    })).await?;
    fixture.objects.close().await?;
    assert_eq!(
        keys,
        (vec!["row_id".to_owned()], vec!["row_id".to_owned()]),
        "natural metadata keys replaced physical UUID identities"
    );
    drop(fixture);
    let reopened = Fixture::open(root.path()).await?;
    assert_eq!(
        ObjectBootstrap::new(reopened.host.clone(), ProbeIo.now())
            .initialize(EngineSchema::Copy)
            .await?,
        0
    );
    assert_eq!(history_ids(&reopened).await?, identities);
    reopened.objects.close().await?;
    Ok(())
}

async fn history_ids(fixture: &Fixture) -> Result<Vec<String>, TestError> {
    Ok(fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>(
                    "SELECT row_id FROM central_schema_migrations ORDER BY corpus,version",
                )
                .fetch_all(scope.executor())
                .await
                .map_err(StoreError::backend)?
                .into_iter()
                .map(|row| row.try_get("row_id").map_err(StoreError::backend))
                .collect()
            })
        })
        .await?)
}
async fn failure_id(fixture: &Fixture) -> Result<String, TestError> {
    Ok(fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>("SELECT row_id FROM central_schema_migration_failures")
                    .fetch_one(scope.executor())
                    .await
                    .map_err(StoreError::backend)?
                    .try_get("row_id")
                    .map_err(StoreError::backend)
            })
        })
        .await?)
}
#[tokio::test]
async fn repeated_migration_failure_upsert_preserves_its_uuid_across_reopen()
-> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                let collision = fleet_sync_receiver::RECEIVER_MIGRATOR
                    .iter()
                    .find(|migration| migration.version == 3)
                    .ok_or_else(|| StoreError::backend("receiver v3 missing"))?;
                scope
                    .executor()
                    .execute(collision.sql.as_ref())
                    .await
                    .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await?;
    let bootstrap = ObjectBootstrap::new(fixture.host.clone(), ProbeIo.now());
    assert!(bootstrap.initialize(EngineSchema::Copy).await.is_err());
    let original = failure_id(&fixture).await?;
    assert_eq!(
        durable_steps::Uuid::parse_str(&original)?.get_version_num(),
        7
    );
    assert!(bootstrap.initialize(EngineSchema::Copy).await.is_err());
    assert_eq!(failure_id(&fixture).await?, original);
    fixture.objects.close().await?;
    drop(bootstrap);
    drop(fixture);
    let reopened = Fixture::open(root.path()).await?;
    assert!(
        ObjectBootstrap::new(reopened.host.clone(), ProbeIo.now())
            .initialize(EngineSchema::Copy)
            .await
            .is_err()
    );
    assert_eq!(failure_id(&reopened).await?, original);
    reopened.objects.close().await?;
    Ok(())
}
