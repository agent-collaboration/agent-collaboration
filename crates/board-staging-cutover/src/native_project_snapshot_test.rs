use super::*;
use crate::cutover_host_fixture::CutoverHostFixture;
use message_board::{
    ActorOrigin, BoardId, Identity, ProjectFeedEntry, ProjectState, RecordEnvelope,
    RecordPlacement, RecordScope, RecordedActor, RequestId, SubjectRevision, WorkspaceId,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct NativeFixture {
    selected: SelectedWriterFile,
    manifest: SnapshotManifest,
    record_time: RecordTime,
    cursor: FeedCursor,
    path: PathBuf,
}

async fn native_fixture(fixture: &CutoverHostFixture) -> TestResult<NativeFixture> {
    let project = ProjectId::generate();
    let database_id = DatabaseId::try_from(project.as_str().to_owned())?;
    let directory = codex_router_host::router_collaboration_directory(&fixture.root)
        .join("projects")
        .join(project.as_str());
    std::fs::create_dir_all(&directory)?;
    let path = directory.join("e1.sqlite");
    let database = Arc::new(fixture_database(&path, database_id, fixture.peer).await?);
    let store = ProjectStore::from_database(project.clone(), database.clone()).await?;
    let origin = ActorOrigin {
        installation: String::from(fixture.service.clone()).try_into()?,
        machine: "cutover source fixture".to_owned().try_into()?,
    };
    let time = RecordTime::Router(
        chrono::DateTime::from_timestamp(1_577_934_245, 0).ok_or("fixed source time invalid")?,
    );
    let mut write = store.begin_admission(origin.clone()).await?;
    let cursor = write
        .append_record(Record::Project(ProjectState {
            envelope: RecordEnvelope {
                id: project.clone().into(),
                scope: RecordScope {
                    workspace: WorkspaceId::generate(),
                    board: Some(BoardId::generate()),
                    project: Some(project.clone()),
                },
                actor: RecordedActor::declared(Identity::Human {
                    human_id: uuid::Uuid::now_v7().to_string().try_into()?,
                }),
                request: Some(RequestId::generate()),
                position: RecordPlacement::Project(ProjectFeedEntry {
                    cursor: FeedCursor {
                        epoch: EpochNumber::INITIAL,
                        position: FeedPosition::ZERO,
                    },
                    origin,
                }),
                time,
            },
            project_id: project.clone(),
            description: "unchanged current-format source".to_owned().try_into()?,
            revision: SubjectRevision::INITIAL,
            closed_at: None,
        }))
        .await?;
    write.commit().await?;
    let mut before = database.snapshot().await?;
    let manifest = before.manifest().await?;
    before.close().await?;
    drop(store);
    database.close().await;
    drop(database);
    Ok(NativeFixture {
        selected: SelectedWriterFile {
            project,
            peer: fixture.peer,
            file_epoch: Epoch::new(1)?,
        },
        manifest,
        record_time: time,
        cursor,
        path,
    })
}

async fn fixture_database(
    path: &Path,
    database: DatabaseId,
    peer: PeerId,
) -> TestResult<ReplicatedDatabase<BoardProjectSchema>> {
    // Only fixture genesis uses provisional writes. The export API never does.
    Ok(ReplicatedDatabase::open_with(
        path,
        database,
        BoardProjectSchema::new()?,
        LeaseHolder::provisional(database, peer, BOARD_LEASE_POLICY),
        Arc::new(SystemClock),
        WriterOptions {
            allow_provisional_writes: true,
        },
    )
    .await?)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Native facts must match the actual admitted source."
)]
async fn native_source_pins_real_tables_and_keeps_the_source_record_time() -> TestResult {
    // Arrange a real native genesis and project admission, not a fabricated lease.
    let fixture = CutoverHostFixture::new(None)?;
    let original = native_fixture(&fixture).await?;
    let mut offline = fixture.acquire()?;
    // Act through the owning cutover API, including retained-reader handoff.
    let mut source = NativeProjectSnapshot::open(&mut offline, original.selected).await?;
    // Assert actual table hashes, source time and cursor survive unchanged.
    let preflight = source.preflight();
    assert_eq!(preflight.manifest, original.manifest);
    assert_eq!(preflight.high_water, original.cursor);
    assert_eq!(preflight.application_epoch, EpochNumber::INITIAL);
    assert_eq!(preflight.latest_record_at, original.record_time);
    assert!(preflight.line_two.is_none());
    let mut tables = BTreeSet::new();
    while let Some(page) = source.next_page().await? {
        assert_eq!(page.contents.position.epoch, Epoch::new(1)?);
        tables.insert(page.contents.table);
    }
    assert!(tables.contains("project_state"));
    assert!(tables.contains("project_feed"));
    source.close().await?;
    assert!(original.path.is_file());
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Wrong selected peers must refuse before any source lookup."
)]
async fn selected_peer_mismatch_refuses_before_source_path_access() -> TestResult {
    // Arrange no project directory: identity refusal must take precedence over I/O.
    let fixture = CutoverHostFixture::new(None)?;
    let mut offline = fixture.acquire()?;
    let selected = SelectedWriterFile {
        project: ProjectId::generate(),
        peer: PeerId::new(uuid::Uuid::now_v7())?,
        file_epoch: Epoch::new(1)?,
    };
    // Act.
    let error = match NativeProjectSnapshot::open(&mut offline, selected).await {
        Err(error) => error,
        Ok(source) => {
            source.close().await?;
            return Err("wrong peer source was accepted".into());
        }
    };
    // Assert the declared writer cannot be silently replaced by another machine.
    assert!(
        matches!(error, NativeSourceError::PeerMismatch),
        "{error:?}"
    );
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Mislabeled files must refuse while only retained readers exist."
)]
async fn a_mislabeled_native_file_epoch_refuses_before_writer_open() -> TestResult {
    // Arrange a genuine e1 whose filename has been mislabeled as e2.
    let fixture = CutoverHostFixture::new(None)?;
    let mut original = native_fixture(&fixture).await?;
    let mislabeled = original.path.with_file_name("e2.sqlite");
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let from = original.path.with_file_name(format!("e1.sqlite{suffix}"));
        match std::fs::symlink_metadata(&from) {
            Ok(_) => std::fs::rename(
                from,
                mislabeled.with_file_name(format!("e2.sqlite{suffix}")),
            )?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !suffix.is_empty() => {}
            Err(error) => return Err(error.into()),
        }
    }
    original.selected.file_epoch = Epoch::new(2)?;
    let before = std::fs::read(&mislabeled)?;
    let retained = RetainedFile::open_read_only(&mislabeled, BoardProjectSchema::new()?).await?;
    let mut offline = fixture.acquire()?;
    // Act. The external retained reader would prevent any writable reopen.
    let error = match NativeProjectSnapshot::open(&mut offline, original.selected).await {
        Err(error) => error,
        Ok(source) => {
            source.close().await?;
            return Err("mislabeled source was accepted".into());
        }
    };
    // Assert identity is rejected first and no source byte is rewritten.
    assert!(
        matches!(error, NativeSourceError::FileIdentityMismatch),
        "{error:?}"
    );
    assert_eq!(std::fs::read(&mislabeled)?, before);
    drop(retained);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A0 must reject the greatest application epoch before writable admission."
)]
async fn an_empty_epoch_three_line_refuses_before_writer_open() -> TestResult {
    // Arrange deliberately unsupported application metadata in a native file.
    // A genuine L3 holder is a separate real-TestCentral cutover proof.
    let fixture = CutoverHostFixture::new(None)?;
    let original = native_fixture(&fixture).await?;
    let database = fixture_database(
        &original.path,
        DatabaseId::try_from(original.selected.project.as_str().to_owned())?,
        fixture.peer,
    )
    .await?;
    let mut write = database.begin_write().await?;
    sqlx::query("INSERT INTO epoch_lines(line_id,epoch,inherited_through) VALUES (?,3,?)")
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(i64::try_from(original.cursor.position.get())?)
        .execute(&mut write)
        .await?;
    write.commit().await?;
    database.close().await;
    drop(database);
    let before = std::fs::read(&original.path)?;
    let retained = RetainedFile::open_read_only(&original.path, BoardProjectSchema::new()?).await?;
    let mut offline = fixture.acquire()?;
    // Act with another retained reader keeping writable admission unavailable.
    let error = match NativeProjectSnapshot::open(&mut offline, original.selected).await {
        Err(error) => error,
        Ok(source) => {
            source.close().await?;
            return Err("application epoch three was accepted".into());
        }
    };
    // Assert the application epoch, not the e1 filename, caused refusal.
    assert!(
        matches!(error,NativeSourceError::UnsupportedApplicationEpoch {epoch} if epoch.get()==3),
        "{error:?}"
    );
    assert_eq!(std::fs::read(&original.path)?, before);
    drop(retained);
    Ok(())
}
