//! The board adapter rejects genuinely verified follower files for other schemas.
use super::*;
use durable_steps::{
    AtomicStore, CallFailure, CallTarget, Engine, StoreError, storage::TransactionScope,
};
use fleet_sync_protocol::*;
use fleet_sync_tokio_runner::{
    CallEnvelope, CallRouter, InstallCoordinator, InstallFile, InstallFileRequest,
    InstallFileResult, InstallRefusal, InstallRequest, InstallSnapshot, InstallSource,
    InstallStatus, ObjectKey, ObjectOptions, ObjectStore, StagingStore, TokioHost, TokioObject,
};
use fleet_sync_writer::{Migration, ReplicatedDatabase, ReplicatedSchema, WriterOptions};
use sqlx::{SqlSafeStr, migrate::MigrationType};
use std::sync::{LazyLock, OnceLock, Weak};

static MIGRATIONS: LazyLock<Vec<Migration>> = LazyLock::new(|| {
    vec![
        Migration::new(
            1,
            "different application".into(),
            MigrationType::Simple,
            "CREATE TABLE records(id TEXT NOT NULL PRIMARY KEY,value TEXT) STRICT".into_sql_str(),
            false,
        ),
        Migration::new(
            2,
            "newer application".into(),
            MigrationType::Simple,
            "CREATE TABLE later_records(id TEXT NOT NULL PRIMARY KEY,value TEXT) STRICT"
                .into_sql_str(),
            false,
        ),
    ]
});
static TABLES: LazyLock<Result<Vec<TableName>, TextValueError>> = LazyLock::new(|| {
    ["records", "later_records"]
        .into_iter()
        .map(TableName::new)
        .collect()
});

#[derive(Clone, Copy)]
enum SourceSchema {
    DifferentLayout,
    NewerVersion,
}
struct TestSchema {
    kind: SourceSchema,
    tables: &'static [TableName],
}
impl ReplicatedSchema for TestSchema {
    fn migrations(&self) -> &'static [Migration] {
        match self.kind {
            SourceSchema::DifferentLayout => MIGRATIONS.get(..1).unwrap(),
            SourceSchema::NewerVersion => &MIGRATIONS,
        }
    }
    fn replicated_tables(&self) -> &'static [TableName] {
        self.tables
    }
}
type SchemaHost = TokioHost<SchemaCopyCalls>;
struct SchemaInstallParts {
    coordinator: Arc<InstallCoordinator<SchemaHost>>,
    target: Arc<TokioObject<SchemaHost>>,
}
struct SchemaCopyCalls {
    snapshot: tokio::sync::Mutex<Option<fleet_sync_writer::WriterSnapshot>>,
    source: CopyStatus,
    parts: OnceLock<Weak<SchemaInstallParts>>,
}
impl CallRouter for SchemaCopyCalls {
    async fn dispatch(&self, call: CallEnvelope) -> Result<serde_json::Value, CallFailure> {
        if call.kind == InstallSource::KIND {
            return serde_json::to_value(Ok::<_, InstallRefusal>(self.source.clone()))
                .map_err(|_| CallFailure::AnswerLost);
        }
        if call.kind != InstallFile::KIND {
            return Err(CallFailure::AnswerLost);
        }
        let file: InstallFile =
            serde_json::from_value(call.target).map_err(|_| CallFailure::AnswerLost)?;
        let request: InstallFileRequest =
            serde_json::from_value(call.request).map_err(|_| CallFailure::AnswerLost)?;
        let parts = self
            .parts
            .get()
            .and_then(Weak::upgrade)
            .ok_or(CallFailure::AnswerLost)?;
        let result = match request {
            InstallFileRequest::Prepare { source } => {
                parts.coordinator.prepare(file.install_id, &source).await
            }
            InstallFileRequest::Page { page, cursor } => {
                let contents = self
                    .snapshot
                    .lock()
                    .await
                    .as_mut()
                    .ok_or(CallFailure::AnswerLost)?
                    .page(cursor.clone())
                    .await
                    .map_err(|_| CallFailure::AnswerLost)?;
                parts
                    .coordinator
                    .fetch_page(file.install_id, page, cursor.as_ref(), move || async move {
                        Ok(SnapshotRead::Page(Box::new(contents)))
                    })
                    .await
            }
            InstallFileRequest::Verify => {
                let manifest = self
                    .snapshot
                    .lock()
                    .await
                    .as_mut()
                    .ok_or(CallFailure::AnswerLost)?
                    .manifest()
                    .await
                    .map_err(|_| CallFailure::AnswerLost)?;
                parts
                    .coordinator
                    .verify_from(file.install_id, move || async move {
                        Ok(SnapshotManifestAnswer::Manifest(manifest))
                    })
                    .await
            }
            InstallFileRequest::Publish => parts
                .coordinator
                .publish(file.install_id, &parts.target)
                .await
                .map(InstallFileResult::Outcome),
            InstallFileRequest::Ready => parts
                .coordinator
                .mark_read_copy_ready(file.install_id, &parts.target)
                .await
                .map(InstallFileResult::Outcome),
        }
        .map_err(|error| InstallRefusal {
            code: "schema_fixture_install".to_owned(),
            detail: error.to_string(),
        });
        serde_json::to_value(result).map_err(|_| CallFailure::AnswerLost)
    }
}

async fn verified_schema_copy(
    root: &Path,
    kind: SourceSchema,
) -> Result<(ProjectId, fleet_sync_writer::ReadCopy), Box<dyn std::error::Error + Send + Sync>> {
    let tables = TABLES.as_ref().map_err(Clone::clone)?;
    let tables = match kind {
        SourceSchema::DifferentLayout => tables.get(..1).unwrap(),
        SourceSchema::NewerVersion => tables.as_slice(),
    };
    let peer = PeerId::new(durable_steps::Uuid::now_v7())?;
    let database = ReplicatedDatabase::create(
        &root.join("source.sqlite"),
        TestSchema { kind, tables },
        peer,
        board_fleet_sync::BOARD_LEASE_POLICY,
        WriterOptions::default(),
    )
    .await?;
    let now = SystemClock.now();
    database
        .observe_registration(
            LeaseStatus {
                database: database.database(),
                epoch: Epoch::new(1)?,
                holder: peer,
                state: LeaseState::Held,
                expires_at: now.after(board_fleet_sync::BOARD_LEASE_POLICY.lease_duration),
                remaining_ms: u64::try_from(
                    board_fleet_sync::BOARD_LEASE_POLICY
                        .lease_duration
                        .as_millis(),
                )?,
            },
            now,
        )
        .await?;
    database.migrate().await?;
    let project = ProjectId::try_from(database.database().as_uuid().to_string())?;
    let snapshot = database.snapshot().await?;
    let schema = snapshot.schema().clone();
    let position = snapshot.position();
    let calls = Arc::new(SchemaCopyCalls {
        snapshot: tokio::sync::Mutex::new(Some(snapshot)),
        source: CopyStatus {
            database: database.database(),
            role: CopyRole::Authoritative,
            fence: position.epoch,
            holder: Some(peer),
            start: ReplicationPosition::new(position.epoch, CommitSequence::new(0)),
            acked: position,
            schema: schema.clone(),
        },
        parts: OnceLock::new(),
    });
    let host = Arc::new(TokioHost::new(calls.clone(), SystemClock));
    let objects = ObjectStore::open(root.join("objects")).await?;
    let control = objects
        .object(
            ObjectKey::new("control")?,
            ObjectOptions::default(),
            host.clone(),
        )
        .await?;
    let target = objects
        .object(
            ObjectKey::new("copy")?,
            ObjectOptions {
                foreign_keys: false,
            },
            host.clone(),
        )
        .await?;
    target
        .transaction(|scope| {
            Box::pin(async move {
                let mut migrator = sqlx::migrate::Migrator::with_migrations(
                    fleet_sync_receiver::RECEIVER_MIGRATOR
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>(),
                );
                migrator.dangerous_set_table_name("_fleet_sync_copy_migrations");
                migrator
                    .run_direct(None, scope.executor(), false)
                    .await
                    .map_err(StoreError::backend)
            })
        })
        .await?;
    let coordinator = Arc::new(
        InstallCoordinator::open(
            control.clone(),
            Arc::new(StagingStore::open(root.join("staging")).await?),
        )
        .await?,
    );
    let parts = Arc::new(SchemaInstallParts {
        coordinator: coordinator.clone(),
        target: target.clone(),
    });
    calls
        .parts
        .set(Arc::downgrade(&parts))
        .map_err(|_| "duplicate schema installer")?;
    let mut engine = Engine::new(control, host.clone());
    engine.register(InstallSnapshot::new(host))?;
    let handle = coordinator
        .start(
            InstallRequest {
                attempt: InstallAttemptId::new(durable_steps::Uuid::now_v7())?,
                target: target.key().clone(),
                database: database.database(),
                source: ReceiverId::new(durable_steps::Uuid::now_v7())?,
            },
            &target,
        )
        .await?;
    timeout(BOUND, engine.drive_due()).await??;
    let record = handle.status().await?;
    let InstallStatus::Ready { snapshot } = record.status else {
        return Err(format!("native schema fixture did not verify: {:?}", record.status).into());
    };
    let copy = coordinator.read_copy(snapshot.install_id, &target).await?;
    assert_eq!(copy.installed().schema, schema);
    calls
        .snapshot
        .lock()
        .await
        .take()
        .ok_or("source snapshot closed early")?
        .close()
        .await?;

    database.close().await;
    objects.close().await?;
    Ok((project, copy))
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn a_verified_different_application_layout_is_refused_by_the_board_catalog() -> TestResult {
    let root = tempfile::tempdir()?;
    let (project, copy) = verified_schema_copy(root.path(), SourceSchema::DifferentLayout).await?;
    assert_eq!(copy.installed().schema.version, 1);
    let result = message_board_storage::ProjectReadCopy::from_verified(project, copy).await;
    assert!(
        matches!(result, Err(message_board_storage::ProjectStorageError::SchemaNotReady(
        SchemaRefusal::Mismatch { version: 1, expected, actual })) if expected != actual)
    );
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn a_verified_newer_application_version_is_refused_without_running_board_migrations()
-> TestResult {
    let root = tempfile::tempdir()?;
    let (project, copy) = verified_schema_copy(root.path(), SourceSchema::NewerVersion).await?;
    assert_eq!(copy.installed().schema.version, 2);
    let result = message_board_storage::ProjectReadCopy::from_verified(project, copy).await;
    assert!(matches!(
        result,
        Err(message_board_storage::ProjectStorageError::SchemaNotReady(
            SchemaRefusal::TooNew {
                store: 2,
                supported: 1
            }
        ))
    ));
    Ok(())
}
