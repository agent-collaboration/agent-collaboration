//! Real native genesis shared by cutover component tests.
use crate::SelectedWriterFile;
use crate::cutover_host_fixture::CutoverHostFixture;
use board_fleet_sync::{BOARD_LEASE_POLICY, BoardProjectSchema};
use fleet_project_lease::LeaseHolder;
use fleet_project_writer::{ReplicatedDatabase, WriterOptions};
use fleet_protocol::{DatabaseId, Epoch, PeerId, SnapshotManifest};
use message_board::{
    ActorOrigin, BoardId, Identity, ProjectFeedEntry, ProjectState, RecordEnvelope,
    RecordPlacement, RecordScope, RecordedActor, RequestId, SubjectRevision, WorkspaceId,
};
use message_board::{EpochNumber, FeedCursor, FeedPosition, ProjectId, Record, RecordTime};
use message_board_storage::ProjectStore;
use runtime_primitives::SystemClock;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub(crate) struct NativeFixture {
    pub(crate) selected: SelectedWriterFile,
    pub(crate) manifest: SnapshotManifest,
    pub(crate) record_time: RecordTime,
    pub(crate) cursor: FeedCursor,
    pub(crate) path: PathBuf,
    pub(crate) scope: RecordScope,
}

pub(crate) async fn native_fixture(fixture: &CutoverHostFixture) -> TestResult<NativeFixture> {
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
    let scope = RecordScope {
        workspace: WorkspaceId::generate(),
        board: Some(BoardId::generate()),
        project: Some(project.clone()),
    };
    let mut write = store.begin_admission(origin.clone()).await?;
    let cursor = write
        .append_record(Record::Project(ProjectState {
            envelope: RecordEnvelope {
                id: project.clone().into(),
                scope: scope.clone(),
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
        scope,
    })
}

pub(crate) async fn fixture_database(
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
