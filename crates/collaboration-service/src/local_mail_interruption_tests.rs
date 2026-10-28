//! Abandon the actual SDK drive after native receipt, then physically reopen its owner.
use super::*;
use durable_steps::OperationFailure;
use fleet_sync_protocol::RecipientOutcome;
use std::sync::atomic::Ordering;

#[tokio::test]
async fn interrupted_sdk_dispatch_reopens_as_uncertain_without_repeating_the_native_call() {
    // The endpoint is scripted; the Inbox, operation journal, push and producer replay are real.
    let runtime = Runtime::new().await;
    let draft = runtime.fixture.draft();
    let item = runtime.fixture.item(&draft).id();
    let push = draft.push_id.clone();
    runtime.client.script(item, vec![accepted()]);
    assert!(runtime.fixture.admit_local(draft.clone()).await.unwrap());
    let witness = runtime.fixture.pause_after_external_answer();
    let engine = runtime.fixture.engine();
    let worker = tokio::spawn(async move { engine.drive_due().await });
    tokio::time::timeout(std::time::Duration::from_secs(10), witness.entered)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(runtime.client.calls(item), 1);
    assert_eq!(
        runtime
            .fixture
            .owner
            .get_push_record(&push)
            .await
            .unwrap()
            .unwrap()
            .delivery_state,
        collaboration_protocol::PushDeliveryState::Attempted
    );
    assert!(runtime.fixture.local_outcome(item).await.is_none());

    // Cancellation drops the drive at its durable call intent; no answer is supplied to the SDK.
    worker.abort();
    assert!(worker.await.unwrap_err().is_cancelled());
    drop(witness.resume);
    let runtime = runtime.reopen().await;
    runtime
        .fixture
        .host
        .clock
        .fetch_add(1_000_000, Ordering::SeqCst);
    let engine = runtime.fixture.engine();
    let recovered = tokio::time::timeout(std::time::Duration::from_secs(10), engine.drive_due())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recovered.failed.len(), 1, "{recovered:?}");
    assert!(matches!(
        recovered.failed.first(),
        Some((_, OperationFailure::Uncertain { .. }))
    ));
    assert_eq!(
        runtime.client.calls(item),
        1,
        "an interrupted non-deduplicating call was resent"
    );
    assert_eq!(
        runtime.fixture.local_outcome(item).await,
        Some(RecipientOutcome::Uncertain)
    );
    let original = runtime
        .fixture
        .owner
        .get_push_record(&push)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        original.delivery_state,
        collaboration_protocol::PushDeliveryState::OutcomeUnknown
    );
    let settled = original.settled_at.unwrap();
    assert_eq!(
        original.last_outcome.as_ref().unwrap().outcome,
        DeliveryOutcome::Unknown
    );
    drop(engine);
    let mail = runtime.local_mail_runtime();
    let replay = mail
        .send_local(draft.clone(), "original interrupted target".into())
        .await
        .unwrap();
    assert_eq!(replay.receipt.outcome, DeliveryOutcome::Unknown);
    assert_eq!(runtime.client.calls(item), 1);
    assert_eq!(
        runtime
            .fixture
            .owner
            .prune_push_records(settled + chrono::Duration::days(30), 500)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        runtime
            .fixture
            .owner
            .prune_push_records(
                settled + chrono::Duration::days(30) + chrono::Duration::nanoseconds(1),
                500
            )
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        serde_json::to_value(
            mail.send_local(draft, "renamed target".into())
                .await
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(replay).unwrap()
    );
    assert_eq!(runtime.client.calls(item), 1);
    assert!(
        runtime
            .fixture
            .owner
            .get_push_record(&push)
            .await
            .unwrap()
            .is_none()
    );
    drop(mail);
    runtime.close().await;
}
