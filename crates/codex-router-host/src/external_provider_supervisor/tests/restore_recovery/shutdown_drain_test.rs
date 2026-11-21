use super::*;
use std::{task::Poll, time::Duration};
use tokio::sync::oneshot;

const DRAIN_ELAPSED: &str = "external provider operation drain timed out";

#[tokio::test(flavor = "current_thread")]
async fn shutdown_completes_when_a_reserved_admission_drops_without_a_result() {
    let rig = RestoreRig::new();
    let host = rig.open_host("restored-in-auto").await;
    let runtime = &host
        .inner
        .bindings
        .get(&rig.target.endpoint)
        .expect("binding")
        .runtime;
    runtime.shutdown().await;
    let (entered, reserving) = oneshot::channel();
    let (_release, release) = oneshot::channel();
    *host
        .inner
        .admission_test_pause
        .lock()
        .expect("admission pause") = Some(AdmissionTestPause { entered, release });
    let operation = OperationId::generate();
    let admission = host.prepare_operation(
        host.admission_deadline(),
        operation.clone(),
        ProviderOperationKind::ConversationLoad,
        binding(),
        Some(&rig.target),
    );
    tokio::pin!(admission);
    tokio::select! {
        entered = reserving => entered.expect("reservation is held"),
        _ = &mut admission => panic!("admission returned before the reservation was held"),
    }
    let reservation = host
        .held_result_for_test(&operation)
        .expect("reserved operation")
        .0;
    let changed = reservation.changed.notified();
    rig.clock
        .advance(Duration::from_secs(1))
        .expect("separate admission and drain deadlines");
    let shutdown = host.shutdown();
    tokio::pin!(shutdown);
    assert!(futures_util::poll!(&mut shutdown).is_pending());
    rig.clock
        .advance(Duration::from_secs(9))
        .expect("expire the admission");
    assert!(admission.await.is_err());
    changed.await;
    assert!(matches!(
        reservation.record(),
        crate::operation_machines::operation_record_machine::OperationRecord::Dropped
    ));
    assert!(host.held_result_for_test(&operation).is_none());

    let outcome = match futures_util::poll!(&mut shutdown) {
        Poll::Ready(outcome) => outcome,
        Poll::Pending => {
            rig.clock
                .advance(Duration::from_secs(1))
                .expect("reach shutdown's drain deadline");
            shutdown.await
        }
    };
    assert_eq!(
        outcome,
        Ok(()),
        "a dropped reservation has no result to await"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn shutdown_spends_one_budget_on_results_and_the_task_join() {
    let rig = RestoreRig::new();
    let host = rig.open_host("restored-in-auto").await;
    host.inner
        .bindings
        .get(&rig.target.endpoint)
        .expect("binding")
        .runtime
        .shutdown()
        .await;
    let operation = OperationId::generate();
    host.hold_live_operation(operation.clone());
    let (release_task, hold_task) = oneshot::channel();
    let task = host.inner.operation_tasks.spawn(async move {
        hold_task.await.expect("release the tracked task");
    });
    let shutdown = host.shutdown();
    tokio::pin!(shutdown);
    assert!(futures_util::poll!(&mut shutdown).is_pending());

    rig.clock
        .advance(Duration::from_secs(6))
        .expect("spend six seconds waiting for the result");
    assert!(host.publish_held_result(&operation).await);
    assert!(futures_util::poll!(&mut shutdown).is_pending());
    rig.clock
        .advance(Duration::from_secs(4))
        .expect("reach the one drain deadline");
    let outcome_at_deadline = futures_util::poll!(&mut shutdown);
    release_task.send(()).expect("release the tracked task");
    task.await.expect("tracked task finishes");
    assert_eq!(outcome_at_deadline, Poll::Ready(Err(DRAIN_ELAPSED)));
}

#[tokio::test(flavor = "current_thread")]
async fn shutdown_deadline_releases_a_late_dispatch_writer_with_no_acknowledgement() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("restored-in-auto").await;
    let (entered, marking) = oneshot::channel();
    let (release_mark, release) = oneshot::channel();
    *host
        .inner
        .dispatch_mark_test_pause
        .lock()
        .expect("dispatch pause") = Some(AdmissionTestPause { entered, release });
    let operation = OperationId::generate();
    host.load(rig.load_request(operation.clone()))
        .await
        .expect("admitted");
    marking
        .await
        .expect("dispatch mark waits before taking the store");
    let mut commit = host
        .inner
        .store
        .lock()
        .await
        .pause_next_admission_after_commit_for_test();
    release_mark
        .send(())
        .expect("mark before the acknowledgement hold");
    commit.reached().await;

    let wait = host.wait(ConversationOperationWaitRequest {
        operation_id: operation.clone(),
        timeout_seconds: PositiveSeconds::try_from(30).expect("wait cutoff"),
    });
    tokio::pin!(wait);
    assert!(futures_util::poll!(&mut wait).is_pending());
    rig.clock
        .advance(Duration::from_secs(10))
        .expect("expire the mark");
    let refusal = wait
        .await
        .expect_err("the caller gets a known-unsent answer");
    assert_eq!(refusal.effect, ProviderOperationEffect::None);
    assert_eq!(refusal.stage, ConversationOperationFailureStage::Dispatch);
    assert!(
        host.inner.store.try_lock().is_err(),
        "the late writer holds the store"
    );
    host.inner
        .bindings
        .get(&rig.target.endpoint)
        .expect("binding")
        .runtime
        .shutdown()
        .await;

    let shutdown = host.shutdown();
    tokio::pin!(shutdown);
    assert!(futures_util::poll!(&mut shutdown).is_pending());
    rig.clock
        .advance(Duration::from_secs(10))
        .expect("expire the shutdown drain");
    assert_eq!(shutdown.await, Err(DRAIN_ELAPSED));
    // The task is now eligible to cancel without any further clock step or
    // acknowledgement. Observe cancellation before letting the commit resume.
    host.inner.operation_tasks.wait().await;
    let mut store = host
        .inner
        .store
        .try_lock()
        .expect("late writer dropped the store");
    let row = store
        .inspect(&operation)
        .await
        .expect("inspect committed mark")
        .expect("row exists");
    assert_eq!(row.stage, ProviderOperationStage::MayHaveDispatched);
    assert_eq!(row.effect, ProviderOperationEffect::Unknown);
    assert!(!rig.root.path().join("wire.json").exists());
    commit.release();
}
