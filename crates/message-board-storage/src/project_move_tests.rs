//! Moving a native file checkpoints WAL data and retains the source on a busy checkpoint.
use crate::project_store_tests::project_record;
use crate::{ProjectLedgerReader, ProjectStorageError, ProjectStore};
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
    let mut write = store.begin_admission().await?;
    write.append_record(project_record(project.clone())).await?;
    let committed = write.commit_retaining_writer().await?;
    assert!(
        tokio::fs::metadata(source.with_extension("sqlite-wal"))
            .await?
            .len()
            > 0,
        "test must move committed WAL data"
    );
    store.move_aside(&committed, &target).await?;
    assert!(!source.exists());
    assert!(target.exists());
    assert!(store.snapshot().await.is_err());
    let reader = ProjectLedgerReader::open(&target, project).await?;
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
    let mut first = store.begin_admission().await?;
    first.append_record(project_record(project.clone())).await?;
    first.commit().await?;
    let mut external = sqlx_turso::TursoConnectOptions::new()
        .filename(&source)
        .create_if_missing(false)
        .connect()
        .await?;
    let mut held = external.begin().await?;
    sqlx::query::<sqlx_turso::Turso>("SELECT singleton FROM feed_checkpoint WHERE singleton = 1")
        .fetch_one(&mut *held)
        .await?;
    let mut second = store.begin_admission().await?;
    second.append_record(project_record(project)).await?;
    let committed = second.commit_retaining_writer().await?;
    let result = store.move_aside(&committed, &target).await;
    assert!(
        matches!(result, Err(ProjectStorageError::MoveAsidePending { .. })),
        "{result:?}"
    );
    assert!(source.exists());
    assert!(!target.exists());
    assert!(store.snapshot().await.is_err());
    drop(committed);
    assert!(store.begin_admission().await.is_err());
    held.rollback().await?;
    external.close().await?;
    store.close().await;
    Ok(())
}
