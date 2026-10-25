//! Moving a native file checkpoints WAL data and retains the source on a busy checkpoint.
use crate::project_store_tests::project_record;
use crate::{ProjectLedgerReader, ProjectStore};
use message_board::{FeedPosition, ProjectId};
use sqlx::{ConnectOptions, Connection};
type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
async fn moving_a_populated_native_store_preserves_tables_still_in_the_wal() -> TestResult {
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let source = directory.path().join("project.sqlite");
    let target = directory.path().join("project.orphaned.sqlite");
    let store = ProjectStore::open(&source, project.clone()).await?;
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    write.append_record(project_record(project.clone())).await?;
    write.commit().await?;
    store
        .database
        .reject_registration(crate::project_test_support::rejection()?)
        .await?;
    assert!(
        tokio::fs::metadata(source.with_extension("sqlite-wal"))
            .await?
            .len()
            > 0,
        "test must move committed WAL data"
    );
    store.database.move_aside(&target).await?;
    assert!(!source.exists());
    assert!(target.exists());
    assert!(store.snapshot().await.is_err());
    let reader = ProjectLedgerReader::open(
        &target,
        project,
        board_fleet_sync::BoardProjectSchema::new()?,
    )
    .await?;
    assert_eq!(
        reader.high_water().await?.position,
        FeedPosition::try_from(1)?
    );
    reader.close().await;
    Ok(())
}

#[tokio::test]
async fn a_busy_move_checkpoint_keeps_the_source_and_pauses_new_admission() -> TestResult {
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let source = directory.path().join("busy.sqlite");
    let target = directory.path().join("busy.orphaned.sqlite");
    let store = ProjectStore::open(&source, project.clone()).await?;
    let mut first = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    first.append_record(project_record(project.clone())).await?;
    first.commit().await?;
    let mut external = sqlx_turso::TursoConnectOptions::new()
        .filename(&source)
        .create_if_missing(false)
        .connect()
        .await?;
    let mut held = external.begin().await?;
    sqlx::query::<sqlx_turso::Turso>("SELECT project_id FROM project_state")
        .fetch_one(&mut *held)
        .await?;
    let mut second = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    second.append_record(project_record(project)).await?;
    second.commit().await?;
    store
        .database
        .reject_registration(crate::project_test_support::rejection()?)
        .await?;
    let result = store.database.move_aside(&target).await;
    assert!(
        matches!(
            result,
            Err(fleet_sync_writer::MoveAsideError::Publish(
                fleet_sync_writer::native_file::PublishError::CheckpointBusy { .. }
            ))
        ),
        "{result:?}"
    );
    assert!(source.exists());
    assert!(!target.exists());
    assert!(store.snapshot().await.is_err());
    assert!(
        store
            .begin_admission(crate::project_test_support::origin())
            .await
            .is_err()
    );
    held.rollback().await?;
    external.close().await?;
    store.close().await;
    Ok(())
}
