//! The aggregate install deadline uses the supplied clock and publishes only verified data.
use super::restart::{reader_registry, replicated_head};
use super::*;
use crate::collaboration_runtime::board_composition::central_read_copy::CentralReadCopies;
use router_clock::{ClockInstant, ManualClock};
use std::time::Duration;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; setup errors propagate through the existing real Host fixture."
)]
async fn a_verified_native_snapshot_can_complete_after_the_old_thirty_second_cutoff() -> TestResult
{
    let runtime = Box::pin(Runtime::new()).await?;
    let (state, head) = replicated_head(&runtime).await?;
    *runtime.central.read_copy_entries.lock().await = vec![directory_entry(&state, 9)?];
    let root = runtime.directory.path().join("long-read");
    tokio::fs::create_dir_all(&root).await?;
    let clock = Arc::new(ManualClock::new("2026-11-11T00:00:00Z".parse()?));
    let source = CentralReadCopies::new(
        &root,
        runtime.requests.clone(),
        runtime.central.receiver,
        runtime.readiness.clone(),
        clock.clone(),
    );
    let (source, gate) = source.pause_after_staged_page_for_test();
    let registry = reader_registry(&runtime, &root, Arc::new(source))?;
    let reader = registry.clone();
    let project = runtime.project.clone();
    let mut reading = tokio::spawn(async move { reader.read_project(&project).await });
    let staged = router_clock::timeout(clock.as_ref(), BOUND, gate.staged).await??;
    assert!(staged.receipt.rows > 0);
    assert!(staged.receipt.next.is_some());
    clock.advance(Duration::from_secs(40))?;
    tokio::select! {
        _ = &mut reading => panic!("valid snapshot ended at the old thirty-second cutoff"),
        () = clock.not_due_check_now(ClockInstant::ZERO.after(Duration::from_secs(180))) => {}
    }
    gate.proceed.send(()).map_err(|_| "page gate closed")?;
    let handle = router_clock::timeout(clock.as_ref(), BOUND, reading)
        .await???
        .ok_or("verified read handle")?;
    let mut snapshot = handle.snapshot().await?;
    assert_eq!(snapshot.high_water(), head);
    let record = snapshot.record(&runtime.project.clone().into()).await?;
    assert!(matches!(record, Some(Record::Project(project)) if project.revision == state.revision));
    snapshot.finish().await?;
    drop(handle);
    assert!(!root.join("projects").exists());
    Arc::try_unwrap(registry)
        .map_err(|_| "reader still retained")?
        .close()
        .await?;
    runtime.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; setup errors propagate through the existing real Host fixture."
)]
async fn the_three_minute_whole_pull_bound_never_publishes_a_partial_snapshot() -> TestResult {
    let runtime = Box::pin(Runtime::new()).await?;
    let (state, _) = replicated_head(&runtime).await?;
    let mut entries = runtime.central.read_copy_entries.lock().await;
    *entries = vec![directory_entry(&state, 9)?];
    let previous_requests = runtime.central.read_copy_requests.load(Ordering::SeqCst);
    let root = runtime.directory.path().join("expired-read");
    tokio::fs::create_dir_all(&root).await?;
    let clock = Arc::new(ManualClock::new("2026-11-11T00:00:00Z".parse()?));
    let source = CentralReadCopies::new(
        &root,
        runtime.requests.clone(),
        runtime.central.receiver,
        runtime.readiness.clone(),
        clock.clone(),
    );
    let (source, gate) = source.pause_after_staged_page_for_test();
    let registry = reader_registry(&runtime, &root, Arc::new(source))?;
    let reader = registry.clone();
    let project = runtime.project.clone();
    let mut reading = tokio::spawn(async move { reader.read_project(&project).await });
    router_clock::timeout(clock.as_ref(), BOUND, async {
        while runtime.central.read_copy_requests.load(Ordering::SeqCst) == previous_requests {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    clock.advance(Duration::from_secs(20))?;
    drop(entries);
    let staged = router_clock::timeout(clock.as_ref(), BOUND, gate.staged).await??;
    assert!(staged.receipt.next.is_some());
    clock.advance(Duration::from_secs(159))?;
    tokio::select! {
        _ = &mut reading => panic!("snapshot ended before its whole-pull deadline"),
        () = clock.not_due_check_now(ClockInstant::ZERO.after(Duration::from_secs(180))) => {}
    }
    clock.advance(Duration::from_secs(1))?;
    let result = router_clock::timeout(clock.as_ref(), BOUND, reading).await??;
    assert!(matches!(
        result,
        Err(collaboration_runtime::ProjectRegistryError::Unavailable { .. })
    ));
    drop(gate.proceed);
    assert!(
        registry
            .completed_project_authority(&runtime.project)
            .await?
            .is_none()
    );
    {
        use sqlx::Connection as _;
        let mut target = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&staged.target)
                .read_only(true),
        )
        .await?;
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_schema WHERE name='project_state'")
                .fetch_one(&mut target)
                .await?;
        assert_eq!(count, 0);
        target.close().await?;
    }
    Arc::try_unwrap(registry)
        .map_err(|_| "reader still retained")?
        .close()
        .await?;
    runtime.close().await
}
