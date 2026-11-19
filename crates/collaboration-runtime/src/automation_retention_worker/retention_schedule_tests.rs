//! The automation retention worker on a real automation store: a protected row holds no timer
//! and goes when the commit that removes its protection wakes the worker.
//!
//! Real store operations remain asynchronous; only the manual clock advances deadlines.
use super::retention_test_harness::*;
use crate::LoopSnapshot;
use agent_automation::{InstructionText, OperationId};
use automation_storage::InstructionUpdate;

#[tokio::test]
async fn a_protected_old_revision_holds_no_timer_and_goes_when_its_protection_is_removed() {
    let directory = tempfile::tempdir().unwrap();
    let store = open_store(directory.path()).await;
    let wall = at("2026-06-15T12:00:00.000Z");
    // The instruction's first revision is long past retention, but it is the current one.
    let instruction = store
        .lock()
        .await
        .create_instruction(
            &OperationId::generate(),
            &InstructionText::try_from("first revision".to_owned()).unwrap(),
            at("2026-01-01T00:00:00.000Z").timestamp_millis(),
        )
        .await
        .unwrap();
    let mut writer = fixture_writer(directory.path()).await;
    insert_event(&mut writer, "later", at("2026-04-15T13:00:00.000Z")).await;

    let worker = start(&store, wall).await;

    let parked = worker.snapshot();
    assert_eq!(
        revisions(&mut writer).await,
        1,
        "the current revision stays"
    );
    assert_eq!(
        armed(&parked),
        Some(at("2026-06-15T13:00:00.001Z")),
        "the protected revision does not arm the timer; the later event does"
    );
    let passes = parked.passes;

    // Replacing the revision is the commit that removes its protection.
    store
        .lock()
        .await
        .update_instruction(&InstructionUpdate {
            operation_id: OperationId::generate(),
            instruction_id: instruction.instruction_id.clone(),
            expected_revision_id: instruction.revision_id.clone(),
            text: InstructionText::try_from("second revision".to_owned()).unwrap(),
            now_ms: wall.timestamp_millis(),
        })
        .await
        .unwrap();
    worker.observation.asleep_at_least(parked.sleeps + 1).await;

    let woke = worker.snapshot();
    assert!(woke.passes > passes, "the commit woke the worker");
    let since_park = LoopSnapshot {
        reads: woke.reads[parked.reads.len()..].to_vec(),
        ..LoopSnapshot::default()
    };
    assert_eq!(
        prunes(&since_park),
        vec!["retention.instruction_revisions.prune"],
        "the old revision went at its due time, which had already passed"
    );
    assert_eq!(
        revisions(&mut writer).await,
        1,
        "only the new revision is left"
    );
    assert_eq!(armed(&woke), Some(at("2026-06-15T13:00:00.001Z")));
    worker.stop().await;
}
