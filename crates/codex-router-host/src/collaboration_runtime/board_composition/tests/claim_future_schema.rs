//! A real newer native consumer snapshot is refused before local seed construction.
use super::*;
use fleet_sync_protocol::{SnapshotPage, TableName};
use fleet_sync_writer::{Migration, ReplicatedDatabase, ReplicatedSchema, WriterOptions};
use sqlx::{SqlSafeStr, migrate::MigrationType};
use std::sync::LazyLock;

static FUTURE_MIGRATIONS: LazyLock<Vec<Migration>> = LazyLock::new(|| {
    let mut migrations = message_board_storage::PROJECT_MIGRATOR
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    migrations.push(Migration::new(
        999,
        "newer board fixture schema".into(),
        MigrationType::Simple,
        "CREATE TABLE claim_future_records(record_key TEXT NOT NULL PRIMARY KEY)".into_sql_str(),
        false,
    ));
    migrations
});

struct FutureBoardSchema {
    board: board_fleet_sync::BoardProjectSchema,
    tables: &'static [TableName],
}
static FUTURE_TABLES: LazyLock<Option<Vec<TableName>>> = LazyLock::new(|| {
    let board = board_fleet_sync::BoardProjectSchema::new().ok()?;
    let mut tables = board.replicated_tables().to_vec();
    tables.push(TableName::new("claim_future_records").ok()?);
    Some(tables)
});
impl FutureBoardSchema {
    fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let board = board_fleet_sync::BoardProjectSchema::new()?;
        let tables = FUTURE_TABLES
            .as_deref()
            .ok_or("future board table catalog")?;
        Ok(Self { board, tables })
    }
}
impl ReplicatedSchema for FutureBoardSchema {
    fn migrations(&self) -> &'static [Migration] {
        &FUTURE_MIGRATIONS
    }
    fn local_migrations(&self) -> &'static [Migration] {
        self.board.local_migrations()
    }
    fn replicated_tables(&self) -> &'static [TableName] {
        self.tables
    }
}

#[tokio::test]
#[cfg(feature = "runtime-test-support")]
async fn future_schema_public_posts_keep_the_typed_refusal_without_forwarding() -> TestResult {
    use collaboration_service::{CollaborationApplication, ServiceIdentity};
    use message_board::public_replies as public;

    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let project_directory = directory.path().join("projects").join(project.as_str());
    std::fs::create_dir_all(&project_directory)?;
    let path = project_directory.join("e1.sqlite");
    let database = fleet_sync_protocol::DatabaseId::try_from(project.as_str().to_owned())?;
    let peer = fleet_sync_protocol::PeerId::new(durable_steps::Uuid::now_v7())?;
    let source = ReplicatedDatabase::open_with(
        &path,
        database,
        FutureBoardSchema::new()?,
        fleet_sync_lease::LeaseHolder::provisional(
            database,
            peer,
            board_fleet_sync::BOARD_LEASE_POLICY,
        ),
        Arc::new(SystemClock),
        WriterOptions {
            allow_provisional_writes: true,
        },
    )
    .await?;
    source.close().await;

    // An enrolled local identity may be offline. No Session or project handle
    // exists, so a mistaken forward cannot supply this native refusal.
    let registry = Arc::new(ProjectStoreRegistry::new(
        directory.path(),
        Some(peer),
        Arc::new(SystemClock),
    ));
    let fleet = Arc::new(
        FleetClient::open(
            directory.path(),
            Arc::new(
                crate::collaboration_runtime::board_composition::UnattachedProjectRegistration,
            ),
            None,
        )
        .await?,
    );
    let admission = Arc::new(ProjectAdmission::new(
        collaboration_service::ProjectAdmissionProps {
            registry,
            fleet,
            identity: identity()?,
        },
    )?);
    let application = CollaborationApplication::new(
        ServiceIdentity::new(
            origin()?.installation.as_str(),
            "00000000-0000-4000-8000-000000000002",
        )?
        .with_project_admission(admission.clone()),
    );
    let before = durable_file_hashes(&path)?;
    let modified = std::fs::metadata(&path)?.modified()?;
    let proof = async {
        let supported = board_fleet_sync::BoardProjectSchema::new()?
            .supported_schemas()
            .iter()
            .map(|schema| schema.version)
            .max()
            .ok_or("current supported schema")?;
        assert!(supported < 999);
        let expected = public::RequestResult::SchemaNotReady(SchemaRefusal::TooNew {
            store: 999,
            supported,
        });
        let actor = Identity::Human {
            human_id: "future-schema author".to_owned().try_into()?,
        };
        let root_post = PostId::generate();
        let created = application
            .board()
            .thread_create(ThreadCreateRequest {
                request_id: RequestId::generate(),
                protocol_version: ProtocolVersion::CURRENT,
                project_id: project.clone(),
                post_id: root_post.clone(),
                about: ThreadSubject::Project {
                    line: "future schema".to_owned().try_into()?,
                },
                actor: actor.clone(),
                acting_for: None,
                text: "must not forward".to_owned().try_into()?,
                references: serde_json::from_value(serde_json::json!([]))?,
                addressees: Vec::new(),
                role: None,
                watch: false,
            })
            .await;
        let posted = application
            .board()
            .thread_post(ThreadPostRequest {
                request_id: RequestId::generate(),
                protocol_version: ProtocolVersion::CURRENT,
                post_id: PostId::generate(),
                placement: PostTarget::Thread {
                    thread: ThreadRef {
                        project: Some(project.clone()),
                        thread: root_post.into(),
                    },
                },
                actor,
                acting_for: None,
                text: "must not forward either".to_owned().try_into()?,
                references: serde_json::from_value(serde_json::json!([]))?,
                addressees: Vec::new(),
            })
            .await;
        for reply in [created, posted] {
            let reply = reply?;
            assert_eq!(
                reply.result, expected,
                "cold future-schema posts must reach native admission"
            );
            assert!(reply.reader.is_none());
        }
        assert!(
            admission
                .runtime_test_context()
                .fleet
                .pending_requests()
                .await?
                .is_empty(),
            "schema-refused posts allocated sender custody"
        );
        assert_eq!(durable_file_hashes(&path)?, before);
        assert_eq!(std::fs::metadata(&path)?.modified()?, modified);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    drop(application);
    let closed = Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("future-schema admission retained"))?
        .close()
        .await;
    proof?;
    closed?;
    Ok(())
}

#[tokio::test]
async fn claim_schema_too_new_preserves_the_actual_copy_identity_and_supported_version()
-> TestResult {
    let fixture = ClaimFixture::start().await?;
    let proof = async {
        let project = ProjectId::generate();
        let source_path = fixture.directory.path().join("future.sqlite");
        // Capture a genuine current board first; then its own migration owner
        // advances this historical source to the unsupported schema.
        project_claim::captured_board_snapshot(&fixture.central, &source_path, &project).await?;
        let database = DatabaseId::try_from(project.as_str().to_owned())?;
        let peer = fixture.projects.peer;
        let source = ReplicatedDatabase::open_with(
            &source_path,
            database,
            FutureBoardSchema::new()?,
            fleet_sync_lease::LeaseHolder::provisional(
                database,
                peer,
                board_fleet_sync::BOARD_LEASE_POLICY,
            ),
            Arc::new(SystemClock),
            WriterOptions {
                allow_provisional_writes: true,
            },
        )
        .await?;
        let mut snapshot = source.snapshot().await?;
        let manifest = snapshot.manifest().await?;
        assert_eq!(manifest.schema.version, 999);
        let mut pages: Vec<SnapshotPage> = Vec::new();
        let mut cursor = None;
        loop {
            let page = snapshot.page(cursor.clone()).await?;
            cursor = page.next.clone();
            pages.push(page);
            if cursor.is_none() {
                break;
            }
        }
        snapshot.close().await?;
        source.close().await;
        fixture
            .central
            .import_board_snapshot(pages, manifest.clone())
            .await?;
        let supported = board_fleet_sync::BoardProjectSchema::new()?
            .supported_schemas()
            .iter()
            .map(|schema| schema.version)
            .max();
        assert!(supported.is_some_and(|version| version < 999));
        assert_eq!(
            fixture.claim(&project).await?,
            ProjectClaimReply::Refused(ProjectClaimRefusal::SchemaTooNew {
                store: manifest.schema,
                supported
            })
        );
        assert!(fixture.registry.existing_project(&project).await?.is_none());
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let closed = fixture.shutdown().await;
    proof?;
    closed?;
    Ok(())
}
