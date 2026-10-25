//! Real registry/library scope proof. The enrolled PeerId is B5's explicit fixture stand-in.
#![allow(clippy::panic_in_result_fn)]
use board_fleet_sync::BOARD_LEASE_POLICY;
use collaboration_service::{ProjectRegistryError, ProjectStoreRegistry};
use fleet_sync_lease::WriterState;
use fleet_sync_protocol::{
    DatabaseId, Epoch, LeaseState, LeaseStatus, PeerId, RenewAnswer, RenewResult,
};
use fleet_sync_writer::WriteRefusal;
use message_board::*;
use message_board_storage::ProjectStorageError;
use runtime_primitives::{Clock, SyncTime};
use std::sync::Arc;
#[path = "support/controlled_clock.rs"]
mod controlled_clock;
use controlled_clock::ControlledClock;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
const START: i64 = 1_792_344_000_000;

fn origin() -> Result<ActorOrigin, Box<dyn std::error::Error + Send + Sync>> {
    Ok(ActorOrigin {
        installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
        machine: MachineLabel::try_from("registry fixture".to_owned())?,
    })
}
fn project_record(project: &ProjectId) -> Result<Record, Box<dyn std::error::Error + Send + Sync>> {
    Ok(Record::Project(ProjectState {
        envelope: RecordEnvelope {
            id: project.clone().into(),
            scope: RecordScope {
                workspace: WorkspaceId::generate(),
                board: Some(BoardId::generate()),
                project: Some(project.clone()),
            },
            actor: RecordedActor::declared(Identity::Human {
                human_id: HumanId::try_from("owner".to_owned())?,
            }),
            request: None,
            position: RecordPlacement::Project(ProjectFeedEntry {
                cursor: FeedCursor {
                    epoch: EpochNumber::INITIAL,
                    position: FeedPosition::ZERO,
                },
                origin: origin()?,
            }),
            time: RecordTime::Router(
                chrono::DateTime::from_timestamp_millis(START).ok_or("fixture time")?,
            ),
        },
        project_id: project.clone(),
        revision: SubjectRevision::INITIAL,
        closed_at: None,
        description: Description::try_from(String::new())?,
    }))
}
fn peer() -> Result<PeerId, Box<dyn std::error::Error + Send + Sync>> {
    Ok(PeerId::new(uuid::Uuid::now_v7())?)
}

#[tokio::test]
async fn never_enrolled_writer_is_refused_before_any_file_is_created() -> TestResult {
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    let registry = ProjectStoreRegistry::new(
        &root,
        None,
        Arc::new(ControlledClock::at(SyncTime::from_unix_millis(START))),
    );
    assert!(matches!(
        registry.open_project(&ProjectId::generate()).await,
        Err(ProjectRegistryError::NotEnrolled)
    ));
    assert!(!root.exists());
    registry.close().await?;
    Ok(())
}

#[tokio::test]
async fn an_unregistered_native_file_remains_unwritable_after_reopen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let peer = peer()?;
    let clock = Arc::new(ControlledClock::at(SyncTime::from_unix_millis(START)));
    // No transport, session or Welcome is supplied. Only the persisted enrollment input is present.
    let registry = ProjectStoreRegistry::new(directory.path(), Some(peer), clock.clone());
    let project = ProjectId::generate();
    let store = registry.open_project(&project).await?;
    assert!(matches!(
        store.writer_state().await?,
        WriterState::Provisional
    ));
    assert!(matches!(
        store.begin_admission(origin()?).await,
        Err(ProjectStorageError::WriteRefused(WriteRefusal::Provisional))
    ));
    assert!(
        directory
            .path()
            .join("projects")
            .join(project.as_str())
            .join("e1.sqlite")
            .is_file()
    );
    drop(store);
    registry.close().await?;

    let reopened = ProjectStoreRegistry::new(directory.path(), Some(peer), clock);
    assert_eq!(reopened.project_ids().await?, vec![project.clone()]);
    let store = reopened
        .existing_project(&project)
        .await?
        .ok_or("reopened project")?;
    let mut read = store.snapshot().await?;
    assert_eq!(read.high_water().position, FeedPosition::ZERO);
    assert!(read.record(&project.clone().into()).await?.is_none());
    read.finish().await?;
    assert!(matches!(
        store.begin_admission(origin()?).await,
        Err(ProjectStorageError::WriteRefused(WriteRefusal::Provisional))
    ));
    drop(store);
    reopened.close().await?;
    Ok(())
}

#[tokio::test]
async fn real_clock_expiry_refuses_an_open_write_keeps_reads_and_renewal_resumes_the_same_epoch()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let peer = peer()?;
    let clock = Arc::new(ControlledClock::at(SyncTime::from_unix_millis(START)));
    let registry = ProjectStoreRegistry::new(directory.path(), Some(peer), clock.clone());
    let project = ProjectId::generate();
    let store = registry.open_project(&project).await?;
    let database = registry.database(&project).await?;
    let initial = clock.now();
    let expires = initial.after(BOARD_LEASE_POLICY.lease_duration);
    let lease = LeaseStatus {
        database: DatabaseId::new(uuid::Uuid::parse_str(project.as_str())?)?,
        epoch: Epoch::new(1)?,
        holder: peer,
        state: LeaseState::Held,
        expires_at: expires,
        remaining_ms: u64::try_from(BOARD_LEASE_POLICY.lease_duration.as_millis())?,
    };
    // This typed registration answer is the fixture input; it is persisted by the real owner.
    database
        .observe_registration(lease.clone(), initial)
        .await?;
    let base = project_record(&project)?;
    let mut write = store.begin_admission(origin()?).await?;
    let first = write.append_record(base.clone()).await?;
    write.commit().await?;

    let mut write = store.begin_admission(origin()?).await?;
    assert_eq!(write.admitted_epoch()?, EpochNumber::INITIAL);
    clock.set(expires);
    let Record::Project(mut changed) = base else {
        return Err("project fixture".into());
    };
    changed.revision = SubjectRevision::try_from(2)?;
    assert!(matches!(
        write.append_record(Record::Project(changed.clone())).await,
        Err(ProjectStorageError::WriteRefused(
            WriteRefusal::LeaseExpired { .. }
        ))
    ));
    write.rollback().await?;
    // Status is observed outside the write scope; reads continue while authority is paused.
    assert!(
        matches!(store.writer_state().await?, WriterState::Paused { expired_at, .. } if expired_at == expires)
    );
    assert!(matches!(
        store.begin_admission(origin()?).await,
        Err(ProjectStorageError::WriteRefused(
            WriteRefusal::LeaseExpired { .. }
        ))
    ));
    let mut read = store.snapshot().await?;
    assert_eq!(read.high_water(), first);
    assert!(
        matches!(read.record(&project.clone().into()).await?, Some(Record::Project(row)) if row.revision == SubjectRevision::INITIAL)
    );
    read.finish().await?;

    let renewed_until = clock.now().after(BOARD_LEASE_POLICY.lease_duration);
    database
        .observe_renewal(
            RenewAnswer::Renewed {
                recorded: RenewResult {
                    epoch: lease.epoch,
                    expires_at: renewed_until,
                },
                current: LeaseStatus {
                    expires_at: renewed_until,
                    ..lease
                },
                transfer_in_progress: None,
                replayed: false,
            },
            clock.now(),
        )
        .await?;
    let mut write = store.begin_admission(origin()?).await?;
    assert_eq!(write.admitted_epoch()?, EpochNumber::INITIAL);
    let second = write.append_record(Record::Project(changed)).await?;
    write.commit().await?;
    assert_eq!(second.position.get(), first.position.get() + 1);
    drop(database);
    drop(store);
    registry.close().await?;
    Ok(())
}
