use super::*;
use crate::external_provider_supervisor::supervisor_test_pauses::ReconcilePausePoint;

#[tokio::test(flavor = "current_thread")]
async fn reconcile_rechecks_liveness_after_admission_commits_before_its_store_guard() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("restored-in-auto").await;
    let operation_id = OperationId::generate();
    let (pause, before_store, take_store) = admission_pause();
    *host
        .inner
        .reconcile_test_pause
        .lock()
        .expect("reconcile pause") = Some((ReconcilePausePoint::BeforeStore, pause));
    let (mark_waiting, finish_mark) = pause_dispatch_mark(&host);

    let mut reconcile = host.reconcile(ConversationOperationReconcileRequest {
        operation_id: operation_id.clone(),
    });
    tokio::select! {
        reached = before_store => reached.expect("reconcile reached store boundary"),
        result = &mut reconcile => panic!("reconcile answered before the crossing: {result:?}"),
    }
    assert!(host.held_result_for_test(&operation_id).is_none());
    host.load(rig.load_request(operation_id.clone()))
        .await
        .expect("the retained admission committed");
    mark_waiting.await.expect("dispatch has not marked the row");

    take_store.send(()).expect("let reconcile take the store");
    let reconciled = reconcile.await.expect("reconcile observes the live row");
    assert_eq!(reconciled.stage, ProviderOperationStage::Admitted);
    assert_eq!(reconciled.effect, ProviderOperationEffect::None);
    assert_eq!(
        reconciled.reconciliation,
        ProviderReconciliationState::Unresolved
    );
    finish_mark.send(()).expect("continue normal restore");
    assert!(matches!(
        host.wait(wait_request(operation_id))
            .await
            .expect("restore finishes")
            .output,
        ConversationOperationWaitOutput::Available {
            settlement: ConversationOperationSettlement::Loaded { .. }
        }
    ));
    host.shutdown().await.expect("fixture shutdown");
}

#[tokio::test(flavor = "current_thread")]
async fn reservation_indexed_under_reconciles_store_guard_commits_after_reconcile() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("restored-in-auto").await;
    let operation_id = OperationId::generate();
    let (before_index, preflight_done, allow_reservation) = admission_pause();
    *host
        .inner
        .admission_before_reservation_test_pause
        .lock()
        .expect("pre-index pause") = Some(before_index);
    let (before_write, reserved, allow_write) = admission_pause();
    *host
        .inner
        .admission_test_pause
        .lock()
        .expect("pre-write pause") = Some(before_write);
    let (with_store, reconcile_holds_store, inspect_row) = admission_pause();
    *host
        .inner
        .reconcile_test_pause
        .lock()
        .expect("reconcile pause") = Some((ReconcilePausePoint::HoldingStore, with_store));

    let mut load = host.load(rig.load_request(operation_id.clone()));
    tokio::select! {
        reached = preflight_done => reached.expect("admission has read before indexing"),
        result = &mut load => panic!("load answered before reservation: {result:?}"),
    }
    let mut reconcile = host.reconcile(ConversationOperationReconcileRequest {
        operation_id: operation_id.clone(),
    });
    tokio::select! {
        reached = reconcile_holds_store => reached.expect("reconcile checked under the guard"),
        result = &mut reconcile => panic!("reconcile answered before admission was released: {result:?}"),
    }
    assert!(host.held_result_for_test(&operation_id).is_none());
    allow_reservation
        .send(())
        .expect("admission may index under the held store");
    reserved.await.expect("admission has indexed a reservation");
    let reservation = host
        .held_result_for_test(&operation_id)
        .expect("indexed reservation")
        .0;
    assert!(reservation.is_reserving());
    assert!(host.inner.store.try_lock().is_err());

    allow_write.send(()).expect("admission may try the store");
    inspect_row
        .send(())
        .expect("reconcile may inspect before releasing the store");
    let failure = reconcile
        .await
        .expect_err("the reservation has no row under reconcile's guard");
    assert_eq!(failure.kind, ConversationOperationFailureKind::NotFound);
    assert_eq!(
        load.await
            .expect("admission commits afterward")
            .operation
            .stage,
        ProviderOperationStage::Admitted
    );
    assert!(matches!(
        host.wait(wait_request(operation_id))
            .await
            .expect("restore finishes")
            .output,
        ConversationOperationWaitOutput::Available {
            settlement: ConversationOperationSettlement::Loaded { .. }
        }
    ));
    host.shutdown().await.expect("fixture shutdown");
}

#[tokio::test(flavor = "current_thread")]
async fn reconcile_reservation_wait_expires_without_touching_the_pending_admission() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("restored-in-auto").await;
    let mut reader = rig.store().await;
    let operation_id = OperationId::generate();
    let mut reconcile = host.reconcile(ConversationOperationReconcileRequest {
        operation_id: operation_id.clone(),
    });
    // Give reconciliation an earlier cutoff than the admission it is about to encounter.
    rig.clock
        .advance(Duration::from_secs(1))
        .expect("later admission");
    let mut commit = host
        .inner
        .store
        .lock()
        .await
        .pause_next_write_before_commit_for_test();
    let mut load = host.load(rig.load_request(operation_id.clone()));
    tokio::select! {
        () = commit.reached() => {}
        answer = &mut load => panic!("admission returned before COMMIT was held: {answer:?}"),
    }
    let before = host
        .held_result_for_test(&operation_id)
        .expect("live reservation")
        .0;
    assert!(before.is_reserving());
    assert!(
        reader
            .inspect(&operation_id)
            .await
            .expect("read before commit")
            .is_none()
    );
    assert!(futures_util::poll!(&mut reconcile).is_pending());

    rig.clock
        .advance(Duration::from_secs(9))
        .expect("reach reconcile L");
    let Poll::Ready(Err(failure)) = futures_util::poll!(&mut reconcile) else {
        panic!("reconcile must answer at L with the reservation still rowless");
    };
    assert_eq!(
        failure.stage,
        ConversationOperationFailureStage::Reconciliation
    );
    assert_eq!(failure.effect, ProviderOperationEffect::None);
    assert_eq!(
        String::from(failure.message),
        "the store didn't respond in time"
    );
    let after = host
        .held_result_for_test(&operation_id)
        .expect("reservation remains indexed")
        .0;
    assert!(Arc::ptr_eq(&before, &after));
    assert!(after.is_reserving());
    assert!(
        reader
            .inspect(&operation_id)
            .await
            .expect("still rowless")
            .is_none()
    );

    commit.release();
    assert_eq!(
        load.await
            .expect("admission commits before its own L")
            .operation
            .stage,
        ProviderOperationStage::Admitted
    );
    assert!(matches!(
        host.wait(wait_request(operation_id))
            .await
            .expect("restore finishes")
            .output,
        ConversationOperationWaitOutput::Available {
            settlement: ConversationOperationSettlement::Loaded { .. }
        }
    ));
    host.shutdown().await.expect("fixture shutdown");
}
