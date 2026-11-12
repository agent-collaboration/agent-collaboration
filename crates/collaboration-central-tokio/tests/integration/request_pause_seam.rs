//! The test-support pause points and write turns on the Tokio host: a forced race and
//! a crash at a held point, with rows read back from the workspace store's file.

use crate::storage_support::{HeldClock, open_platform, read_store, single_integer, workspace};
use collaboration_central_core::storage::{
    AlarmUpdate, BatchOutcome, RowExpectation, StatementBatch, StoreCall, StoreReply,
};
use collaboration_central_core::storage_conformance::{
    count_owners, insert_owner, widgets_migrator,
};
use collaboration_central_core::{CentralPlatform, PausePoint, PausePoints, WorkspaceTurns};
use collaboration_central_tokio::TokioCentralPlatform;
use std::sync::Arc;

/// A stand-in rule with the shape central's rules will have: take the turn, read what
/// the rule needs, then write. Rule: a workspace has at most one owner.
async fn register_first_owner(
    platform: TokioCentralPlatform,
    turns: Arc<WorkspaceTurns>,
    pauses: PausePoints,
    owner_id: &'static str,
) -> bool {
    let _turn = turns.take(&workspace()).await;
    let owners = single_integer(&read_store(&platform, count_owners()).await);
    pauses.reached(&workspace(), PausePoint::AfterReads).await;
    if owners > 0 {
        return false;
    }
    let reply = platform
        .call_store(
            workspace(),
            StoreCall::Batch(StatementBatch {
                statements: vec![insert_owner(owner_id, RowExpectation::Exactly(1))],
                alarm: AlarmUpdate::Unchanged,
            }),
        )
        .await
        .unwrap();
    assert!(matches!(
        reply,
        StoreReply::Batch(BatchOutcome::Applied { .. })
    ));
    pauses
        .reached(&workspace(), PausePoint::AfterStoreApplied)
        .await;
    true
}

#[tokio::test]
async fn a_second_request_waits_for_the_turn_and_reads_the_first_one_write() {
    let root = tempfile::tempdir().unwrap();
    let platform = open_platform(
        root.path(),
        HeldClock::at(1_700_000_000_000),
        widgets_migrator(),
    )
    .await;
    let turns = Arc::new(WorkspaceTurns::default());
    let pauses = PausePoints::default();

    let mut held = pauses.hold(&workspace(), PausePoint::AfterReads);
    let first = tokio::spawn(register_first_owner(
        platform.clone(),
        turns.clone(),
        pauses.clone(),
        "owner-1",
    ));
    held.until_held().await;
    assert_eq!(pauses.held(&workspace()), vec![PausePoint::AfterReads]);

    let second = tokio::spawn(register_first_owner(
        platform.clone(),
        turns.clone(),
        pauses.clone(),
        "owner-2",
    ));
    turns.until_waiting(&workspace(), 1).await;
    assert!(!second.is_finished());

    held.release();
    assert!(first.await.unwrap());
    assert!(!second.await.unwrap());
    assert_eq!(
        single_integer(&read_store(&platform, count_owners()).await),
        1
    );
    platform.shutdown().await.unwrap();
}

async fn crash_at(point: PausePoint) -> i64 {
    let root = tempfile::tempdir().unwrap();
    let clock = HeldClock::at(1_700_000_000_000);
    let platform = open_platform(root.path(), clock.clone(), widgets_migrator()).await;
    let pauses = PausePoints::default();
    let mut held = pauses.hold(&workspace(), point);
    let request = tokio::spawn(register_first_owner(
        platform.clone(),
        Arc::new(WorkspaceTurns::default()),
        pauses.clone(),
        "owner-1",
    ));
    held.until_held().await;
    assert_eq!(pauses.held(&workspace()), vec![point]);

    // The crash: the held request dies with its object, then the object restarts.
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    platform.shutdown().await.unwrap();
    drop(held);
    let restarted = open_platform(root.path(), clock, widgets_migrator()).await;
    let owners = single_integer(&read_store(&restarted, count_owners()).await);
    restarted.shutdown().await.unwrap();
    owners
}

#[tokio::test]
async fn a_crash_after_the_reads_leaves_nothing_and_after_the_store_applied_keeps_the_write() {
    assert_eq!(crash_at(PausePoint::AfterReads).await, 0);
    assert_eq!(crash_at(PausePoint::AfterStoreApplied).await, 1);
}
