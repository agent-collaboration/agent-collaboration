//! A start-epoch filename supplies identity, while native restore retains every authority gate.
#[path = "support/accepted_snapshot.rs"]
pub mod accepted_snapshot;
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError};
use collaboration_runtime::NativeProjectStore;
use collaboration_runtime::ProjectRegistryError;
use collaboration_runtime::ProjectStoreRegistry;
use fleet_sync_lease::WriterState;
use fleet_sync_protocol::{
    DatabaseId, Epoch, LeaseRefusal, LeaseState, LeaseStatus, PeerId, RenewAnswer, RenewResult,
};
use fleet_sync_writer::{OpenError, WriteRefusal};
use message_board::*;
use message_board_storage::ProjectStorageError;
use runtime_primitives::{Clock, SyncTime, SystemClock};
use std::sync::Arc;

fn origin() -> Result<ActorOrigin, TestError> {
    Ok(ActorOrigin {
        installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
        machine: MachineLabel::try_from("registry restart fixture".to_owned())?,
    })
}

async fn assert_write_refused(
    store: &NativeProjectStore,
    expected: WriteRefusal,
) -> Result<(), TestError> {
    match store.begin_admission(origin()?).await {
        Err(ProjectStorageError::WriteRefused(actual)) => assert_eq!(actual, expected),
        Err(error) => return Err(format!("unexpected native gate: {error}").into()),
        Ok(write) => {
            write.rollback().await?;
            return Err("filename admitted a gated writer".into());
        }
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn a_pending_seed_stays_unpublished_after_registry_reopen() -> Result<(), TestError> {
    let source = BoardFixture::start().await?;
    let (project, _) = source.project().await?;
    let target = BoardFixture::start().await?;
    let (_, _) = accepted_snapshot::bootstrap_pending_project(&source, &target, &project).await?;
    assert!(matches!(target._registry.existing_project(&project).await,
        Err(ProjectRegistryError::Installing { project: actual }) if actual == project));
    assert!(matches!(target._registry.open_project(&project).await,
        Err(ProjectRegistryError::Installing { project: actual }) if actual == project));
    target.stop().await?;
    source.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn persisted_expiry_is_not_revived_by_the_start_epoch_filename() -> Result<(), TestError> {
    let source = BoardFixture::start().await?;
    let (project, _) = source.project().await?;
    let target = BoardFixture::start().await?;
    let (inherited, epoch) =
        accepted_snapshot::bootstrap_project(&source, &target, &project).await?;
    let started = SyncTime::from_unix_millis(SystemClock.now().unix_millis() - 120_000);
    let expired_at = SyncTime::from_unix_millis(started.unix_millis() + 60_000);
    target
        ._registry
        .database(&project)
        .await?
        .observe_renewal(
            RenewAnswer::Renewed {
                recorded: RenewResult {
                    epoch: Epoch::new(epoch.get())?,
                    expires_at: expired_at,
                },
                current: LeaseStatus {
                    database: DatabaseId::new(uuid::Uuid::parse_str(project.as_str())?)?,
                    epoch: Epoch::new(epoch.get())?,
                    holder: board_fixture::fixture_peer()?,
                    state: LeaseState::Held,
                    expires_at: expired_at,
                    remaining_ms: 60_000,
                },
                transfer_in_progress: None,
                replayed: false,
            },
            started,
        )
        .await?;
    let target = target.restart_read_fixture().await?;
    let store = target
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("expired file")?;
    let epoch = Epoch::new(epoch.get())?;
    assert_eq!(
        store.writer_state().await?,
        WriterState::Paused { epoch, expired_at }
    );
    assert_write_refused(&store, WriteRefusal::LeaseExpired { epoch, expired_at }).await?;
    let read = store.snapshot().await?;
    assert_eq!(read.high_water(), inherited);
    read.finish().await?;
    drop(store);
    target.stop().await?;
    source.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn persisted_supersession_survives_the_registry_reopen() -> Result<(), TestError> {
    let source = BoardFixture::start().await?;
    let (project, _) = source.project().await?;
    let target = BoardFixture::start().await?;
    let (_, epoch) = accepted_snapshot::bootstrap_project(&source, &target, &project).await?;
    let current = Epoch::new(epoch.get() + 1)?;
    target
        ._registry
        .database(&project)
        .await?
        .observe_renewal(
            RenewAnswer::Refused(LeaseRefusal::NotWriter {
                current: Some(current),
            }),
            SystemClock.now(),
        )
        .await?;
    let target = target.restart_read_fixture().await?;
    let path = target
        ._directory
        .path()
        .join("projects")
        .join(project.as_str())
        .join(format!("e{}.sqlite", epoch.get()));
    let bytes = std::fs::read(&path)?;
    assert!(matches!(target._registry.existing_project(&project).await,
        Err(ProjectRegistryError::Storage(ProjectStorageError::WriteRefused(WriteRefusal::Superseded { current: actual })))
            if actual == current));
    assert!(matches!(
        target._registry.read_project(&project).await,
        Err(ProjectRegistryError::Unavailable { .. })
    ));
    assert_eq!(
        target._registry.superseded_current(&project).await,
        Some(Some(EpochNumber::try_from(current.value())?))
    );
    assert_eq!(std::fs::read(path)?, bytes);
    target.stop().await?;
    source.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn a_seeded_file_cannot_be_opened_by_another_enrolled_peer() -> Result<(), TestError> {
    let source = BoardFixture::start().await?;
    let (project, _) = source.project().await?;
    let target = BoardFixture::start().await?;
    accepted_snapshot::bootstrap_project(&source, &target, &project).await?;
    let registry = ProjectStoreRegistry::new(
        target._directory.path(),
        Some(PeerId::new(uuid::Uuid::now_v7())?),
        Arc::new(SystemClock),
    );
    match registry.existing_project(&project).await {
        Err(ProjectRegistryError::Storage(ProjectStorageError::Writer(OpenError::Corrupt {
            field,
        }))) => assert_eq!(field, "writer.identity"),
        Err(error) => return Err(format!("unexpected identity refusal: {error}").into()),
        Ok(_) => return Err("a different enrolled peer opened the writer".into()),
    }
    registry.close().await?;
    target.stop().await?;
    source.stop().await
}
