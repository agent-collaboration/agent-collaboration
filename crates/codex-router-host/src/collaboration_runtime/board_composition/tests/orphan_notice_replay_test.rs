//! Re-handling after orphan progress loss preserves real cache and native lifecycle owners.
use super::*;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn orphan_progress_replays_cache_and_lifecycle_notices_idempotently() -> TestResult {
    // Arrange already-realized project/cache effects, and only an orphan progress file.
    let runtime = Box::pin(Runtime::new()).await?;
    let home = runtime.progress_directory().join("machine-session");
    tokio::fs::create_dir_all(&home).await?;
    let orphan = home.join("machine-notice-progress.json");
    tokio::fs::write(&orphan, br#"{"handledThrough":73}"#).await?;
    let before = runtime.fleet.records().await?;
    let writer = runtime.projects.ensure(&runtime.project).await?;
    let mut reactions = runtime.reactions().await?;
    assert_eq!(runtime.progress().await?, NoticePosition::new(0));
    assert!(!orphan.exists());
    runtime.central.script_notice_answers(Vec::new()).await;

    // Act: central's current-session prefix replays both kinds of previously
    // realized effect; distinct positions force the real handlers to run twice.
    for position in 1..=4 {
        let kind = if position % 2 == 1 {
            BoardNotice::ProjectListChanged {}
        } else {
            runtime.notice(position).kind
        };
        reactions
            .handle_pushed_notice(
                PositionedBoardNotice {
                    position: NoticePosition::new(position),
                    kind,
                },
                &runtime.projects,
                &Arc::downgrade(&runtime.admission),
                &Arc::downgrade(&runtime.fleet),
                &std::sync::Weak::new(),
                &runtime.requests,
            )
            .await?;
    }

    // Assert cache upserts did not duplicate/change records, and native lifecycle
    // prompts kept the same actual writer and epoch instead of installing again.
    assert_eq!(runtime.fleet.records().await?, before);
    assert!(Arc::ptr_eq(
        &runtime.projects.ensure(&runtime.project).await?,
        &writer
    ));
    assert!(
        matches!(writer.store.writer_state().await?, fleet_sync_lease::WriterState::Writing { epoch } if epoch == runtime.epoch)
    );
    assert_eq!(runtime.progress().await?, NoticePosition::new(4));
    drop(writer);
    drop(reactions);
    runtime.close().await
}
