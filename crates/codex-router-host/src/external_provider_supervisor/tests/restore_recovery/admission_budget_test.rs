use super::*;
use crate::external_provider_supervisor::supervisor_test_pauses::ReconcilePausePoint;
use collaboration_protocol::{
    ConversationAdmissionState, ProviderOperationKind, ProviderOperationStage,
};
use std::{task::Poll, time::Duration};

const KNOWN_UNSENT: &str =
    "the Router was busy and nothing was sent, so send again as a new operation";

async fn free_restore_rig() -> (RestoreRig, ExternalProviderSupervisor) {
    // One Host has one store lock. A separate store exercises the free path.
    let mut free = RestoreRig::new();
    free.target.session_id = SessionId::try_from("free-session".to_owned()).expect("session ID");
    free.seed_session().await;
    let host = free.open_host("restored-in-auto").await;
    (free, host)
}

#[tokio::test(flavor = "current_thread")]
async fn restore_admission_expires_with_no_row_or_owner_while_store_is_held() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("restored-in-auto").await;

    let (free, free_host) = free_restore_rig().await;

    let operation = OperationId::generate();
    let mut held_store = host.inner.store.lock().await;
    let admission = host.prepare_operation(
        host.admission_deadline(),
        operation.clone(),
        ProviderOperationKind::ConversationLoad,
        binding(),
        Some(&rig.target),
    );
    tokio::pin!(admission);
    assert!(futures_util::poll!(&mut admission).is_pending());

    free.assert_loads(&free_host).await;
    rig.clock
        .advance(Duration::from_secs(11))
        .expect("advance past L");

    let failure = match futures_util::poll!(&mut admission) {
        Poll::Ready(Err(failure)) => failure,
        Poll::Ready(Ok(_)) => panic!("held store must not admit restore after L"),
        Poll::Pending => panic!("held store left restore admission pending past L"),
    };
    assert_eq!(failure.kind, ConversationOperationFailureKind::Busy);
    assert_eq!(failure.effect, ProviderOperationEffect::None);
    assert_eq!(String::from(failure.message), KNOWN_UNSENT);
    assert!(
        held_store
            .inspect(&operation)
            .await
            .expect("inspect")
            .is_none()
    );
    assert!(host.owner_session_snapshot(&rig.target).is_none());
    drop(held_store);

    rig.assert_loads(&host).await;
    host.shutdown().await.expect("fixture shutdown");
    free_host.shutdown().await.expect("free fixture shutdown");
}

#[tokio::test(flavor = "current_thread")]
async fn restore_admission_deadline_settles_an_unacknowledged_commit_without_dispatch() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("restored-in-auto").await;
    let (free, free_host) = free_restore_rig().await;

    let mut commit = host
        .inner
        .store
        .lock()
        .await
        .pause_next_admission_after_commit_for_test();
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
        () = commit.reached() => {},
        _ = &mut admission => panic!("admission returned before the commit witness"),
    }

    free.assert_loads(&free_host).await;
    rig.clock
        .advance(Duration::from_secs(10))
        .expect("advance to L");
    let failure = match futures_util::poll!(&mut admission) {
        Poll::Ready(Err(failure)) => failure,
        Poll::Ready(Ok(_)) => panic!("restore admitted at L with an unacknowledged commit"),
        Poll::Pending => panic!("unacknowledged commit left restore admission pending at L"),
    };
    assert_eq!(failure.kind, ConversationOperationFailureKind::Busy);
    assert_eq!(failure.effect, ProviderOperationEffect::None);
    assert_eq!(String::from(failure.message), KNOWN_UNSENT);
    assert!(host.owner_session_snapshot(&rig.target).is_none());

    commit.release();
    let reuse = host
        .load(rig.load_request(operation))
        .await
        .expect("reused operation after commit");
    assert_eq!(reuse.admission, ConversationAdmissionState::Existing);
    assert_eq!(reuse.operation.stage, ProviderOperationStage::Terminal);
    assert_eq!(reuse.operation.effect, ProviderOperationEffect::None);
    assert!(host.owner_session_snapshot(&rig.target).is_none());
    assert!(!rig.root.path().join("wire.json").exists());

    rig.assert_loads(&host).await;
    host.shutdown().await.expect("fixture shutdown");
    free_host.shutdown().await.expect("free fixture shutdown");
}

#[tokio::test(flavor = "current_thread")]
async fn late_restore_admission_stays_known_unsent_if_its_terminal_write_fails() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("restored-in-auto").await;
    let operation = OperationId::generate();
    let mut commit = {
        let mut store = host.inner.store.lock().await;
        store.fail_next_terminal_write_of(&operation);
        store.pause_next_admission_after_commit_for_test()
    };
    let admission = host.prepare_operation(
        host.admission_deadline(),
        operation.clone(),
        ProviderOperationKind::ConversationLoad,
        binding(),
        Some(&rig.target),
    );
    tokio::pin!(admission);
    tokio::select! {
        () = commit.reached() => {},
        _ = &mut admission => panic!("admission returned before the commit witness"),
    }

    rig.clock
        .advance(Duration::from_secs(10))
        .expect("advance to L");
    let Poll::Ready(Err(failure)) = futures_util::poll!(&mut admission) else {
        panic!("admission must answer at L");
    };
    assert_eq!(failure.effect, ProviderOperationEffect::None);
    assert_eq!(String::from(failure.message), KNOWN_UNSENT);
    commit.release();

    let reuse = host
        .load(rig.load_request(operation.clone()))
        .await
        .expect("reuse the exact operation ID");
    assert_eq!(reuse.admission, ConversationAdmissionState::Existing);
    assert_eq!(reuse.operation.stage, ProviderOperationStage::Admitted);
    assert_eq!(reuse.operation.effect, ProviderOperationEffect::None);
    let row = rig
        .store()
        .await
        .inspect(&operation)
        .await
        .expect("read the durable operation")
        .expect("admission was committed");
    assert_eq!(row.dispatched_at_ms, None);
    assert_eq!(row.terminal_at_ms, None);
    assert!(host.owner_session_snapshot(&rig.target).is_none());
    assert!(!rig.root.path().join("wire.json").exists());
    host.shutdown().await.expect("fixture shutdown");

    let reopened = rig.open_host("restored-in-auto").await;
    let pending = reopened
        .wait(wait_request(operation))
        .await
        .expect("wait after restart");
    assert_eq!(pending.operation.stage, ProviderOperationStage::Admitted);
    assert_eq!(pending.operation.effect, ProviderOperationEffect::None);
    assert!(matches!(
        pending.output,
        ConversationOperationWaitOutput::Pending
    ));
    rig.assert_loads(&reopened).await;
    reopened
        .shutdown()
        .await
        .expect("reopened fixture shutdown");
}

fn pause_dispatch_mark(
    host: &ExternalProviderSupervisor,
) -> (
    tokio::sync::oneshot::Receiver<()>,
    tokio::sync::oneshot::Sender<()>,
) {
    let (entered, mark_waiting) = tokio::sync::oneshot::channel();
    let (finish_mark, release) = tokio::sync::oneshot::channel();
    *host
        .inner
        .dispatch_mark_test_pause
        .lock()
        .expect("mark pause") = Some(AdmissionTestPause { entered, release });
    (mark_waiting, finish_mark)
}

fn admission_pause() -> (
    AdmissionTestPause,
    tokio::sync::oneshot::Receiver<()>,
    tokio::sync::oneshot::Sender<()>,
) {
    let (entered, reached) = tokio::sync::oneshot::channel();
    let (resume, release) = tokio::sync::oneshot::channel();
    (AdmissionTestPause { entered, release }, reached, resume)
}

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
async fn late_dispatch_acknowledgement_returns_known_unsent_before_releasing_the_commit() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("restored-in-auto").await;
    let operation = OperationId::generate();
    let PreparedOperation::Admitted { live, .. } = host
        .prepare_operation(
            host.admission_deadline(),
            operation.clone(),
            ProviderOperationKind::ConversationLoad,
            binding(),
            Some(&rig.target),
        )
        .await
        .expect("prepared restore")
    else {
        panic!("restore must be newly admitted");
    };
    let mut commit = host
        .inner
        .store
        .lock()
        .await
        .pause_next_admission_after_commit_for_test();
    let request = rig.load_request(operation.clone());
    let record = Box::new(provider_session_record(
        request.target,
        request.working_directory,
        request.requested_policy,
        request.requested_by,
        request.approver,
        collaboration_runtime::ProviderSessionResumeInputs::default(),
        host.inner.now_ms(),
    ));
    let handoff = host.run_operation_for_handoff(
        operation.clone(),
        live,
        crate::external_provider_supervisor::provider_owner_operations::restore(
            &host,
            operation.clone(),
            record,
            provider_session_machines::session_lifecycle_machine::RestoreVia::Load,
        ),
    );
    tokio::pin!(handoff);
    tokio::select! {
        () = commit.reached() => {}
        () = &mut handoff => panic!("mark completed before the acknowledgement was held"),
    }
    rig.clock.advance(Duration::from_secs(10)).expect("reach L");
    assert!(futures_util::poll!(&mut handoff).is_pending());
    let mut wait = host.wait(wait_request(operation.clone()));
    let Poll::Ready(Err(failure)) = futures_util::poll!(&mut wait) else {
        panic!("dispatch mark must answer known unsent at L with acknowledgement still held");
    };
    assert_eq!(failure.stage, ConversationOperationFailureStage::Dispatch);
    assert_eq!(failure.effect, ProviderOperationEffect::None);
    assert_eq!(
        String::from(failure.message),
        "the store didn't respond in time, so nothing was sent"
    );
    assert!(host.owner_session_snapshot(&rig.target).is_none());
    let reuse = host.prepare_operation(
        host.admission_deadline(),
        operation.clone(),
        ProviderOperationKind::ConversationLoad,
        binding(),
        Some(&rig.target),
    );
    tokio::pin!(reuse);
    assert!(futures_util::poll!(&mut reuse).is_pending());
    commit.release();
    let ((), reused) = tokio::join!(handoff, reuse);
    let PreparedOperation::Existing(reused) = reused.expect("reuse waits for the late writer")
    else {
        panic!("reuse must see the original operation");
    };
    assert_eq!(reused.stage, ProviderOperationStage::Terminal);
    assert_eq!(reused.effect, ProviderOperationEffect::None);

    let row = rig
        .store()
        .await
        .inspect(&operation)
        .await
        .expect("read durable row")
        .expect("marked operation");
    assert_eq!(row.stage, ProviderOperationStage::Terminal);
    assert_eq!(row.effect, ProviderOperationEffect::None);
    assert_eq!(
        row.reconciliation_state,
        ProviderReconciliationState::Confirmed
    );
    assert!(host.owner_session_snapshot(&rig.target).is_none());
    assert!(!rig.root.path().join("wire.json").exists());
    host.shutdown().await.expect("fixture shutdown");
}

#[tokio::test(flavor = "current_thread")]
async fn restore_never_enters_the_owner_after_its_dispatch_mark_expires() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("restored-in-auto").await;
    let operation = OperationId::generate();
    let (mark_waiting, finish_mark) = pause_dispatch_mark(&host);
    host.load(rig.load_request(operation.clone()))
        .await
        .expect("admitted restore");
    mark_waiting.await.expect("dispatch mark is waiting");
    let mut commit = host
        .inner
        .store
        .lock()
        .await
        .pause_next_admission_after_commit_for_test();
    finish_mark.send(()).expect("run the dispatch mark");
    commit.reached().await;

    rig.clock.advance(Duration::from_secs(10)).expect("reach L");
    commit.release();
    let failure = host
        .wait(wait_request(operation))
        .await
        .expect_err("late dispatch mark is known unsent");
    assert_eq!(failure.stage, ConversationOperationFailureStage::Dispatch);
    assert_eq!(failure.effect, ProviderOperationEffect::None);
    assert_eq!(
        String::from(failure.message),
        "the store didn't respond in time, so nothing was sent"
    );
    host.shutdown().await.expect("fixture shutdown");
    assert!(!rig.root.path().join("wire.json").exists());
}

#[tokio::test(flavor = "current_thread")]
async fn restore_dispatch_mark_is_visible_only_after_admission_and_before_wire_handoff() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("silent-first-restore").await;
    let operation = OperationId::generate();
    let (mark_waiting, finish_mark) = pause_dispatch_mark(&host);
    let submission = host
        .load(rig.load_request(operation.clone()))
        .await
        .expect("admitted");
    mark_waiting
        .await
        .expect("retained task is waiting before its mark");

    assert_eq!(submission.operation.stage, ProviderOperationStage::Admitted);
    assert_eq!(submission.operation.effect, ProviderOperationEffect::None);
    let before = host
        .show(ConversationOperationShowRequest {
            operation_id: operation.clone(),
        })
        .await
        .expect("operation before mark");
    assert_eq!(before.stage, ProviderOperationStage::Admitted);
    assert_eq!(before.effect, ProviderOperationEffect::None);
    assert!(host.owner_session_snapshot(&rig.target).is_none());
    assert!(!rig.root.path().join("wire.json").exists());

    finish_mark.send(()).expect("release the mark");
    rig.until_wire_request("restores").await;
    let after = host
        .show(ConversationOperationShowRequest {
            operation_id: operation,
        })
        .await
        .expect("operation after mark");
    assert_eq!(after.stage, ProviderOperationStage::MayHaveDispatched);
    assert_eq!(after.effect, ProviderOperationEffect::Unknown);
    host.shutdown().await.expect("fixture shutdown");
}

#[tokio::test(flavor = "current_thread")]
async fn a_failed_or_expired_restore_dispatch_mark_never_enters_the_owner() {
    for expired in [false, true] {
        let rig = RestoreRig::new();
        rig.seed_session().await;
        let host = rig.open_host("restored-in-auto").await;
        let operation = OperationId::generate();
        let paused = if expired {
            Some(pause_dispatch_mark(&host))
        } else {
            host.inner
                .store
                .lock()
                .await
                .fail_next_dispatch_mark_of(&operation);
            None
        };
        host.load(rig.load_request(operation.clone()))
            .await
            .expect("admitted");
        let finish_mark = if let Some((mark_waiting, finish_mark)) = paused {
            mark_waiting
                .await
                .expect("dispatch mark held before taking store");
            rig.clock
                .advance(Duration::from_secs(10))
                .expect("reach mark deadline");
            Some(finish_mark)
        } else {
            None
        };

        let failure = host
            .wait(wait_request(operation.clone()))
            .await
            .expect_err("mark failure settles");
        assert_eq!(failure.stage, ConversationOperationFailureStage::Dispatch);
        assert_eq!(failure.effect, ProviderOperationEffect::None);
        let expected = if expired {
            "the store didn't respond in time, so nothing was sent"
        } else {
            "the store couldn't record the dispatch mark, so nothing was sent"
        };
        assert_eq!(String::from(failure.message), expected);
        let stored = host
            .show(ConversationOperationShowRequest {
                operation_id: operation,
            })
            .await
            .expect("known-unsent row remains visible");
        assert_eq!(stored.stage, ProviderOperationStage::Admitted);
        assert_eq!(stored.effect, ProviderOperationEffect::None);
        assert!(host.owner_session_snapshot(&rig.target).is_none());
        assert!(!rig.root.path().join("wire.json").exists());
        drop(finish_mark);
        host.shutdown().await.expect("fixture shutdown");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn reconcile_leaves_a_dispatched_restore_alone_while_the_owner_waits() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("silent-first-restore").await;
    let operation = OperationId::generate();
    host.load(rig.load_request(operation.clone()))
        .await
        .expect("admitted");
    rig.until_wire_request("restores").await;
    let before = rig
        .store()
        .await
        .inspect(&operation)
        .await
        .expect("read dispatch mark");
    let live = host
        .held_result_for_test(&operation)
        .expect("live dispatch")
        .0;
    let live_before = live.record();

    let answer = host
        .reconcile(ConversationOperationReconcileRequest {
            operation_id: operation.clone(),
        })
        .await
        .expect("read the live dispatch");
    assert_eq!(answer.stage, ProviderOperationStage::MayHaveDispatched);
    assert_eq!(answer.effect, ProviderOperationEffect::Unknown);
    assert_eq!(
        answer.reconciliation,
        ProviderReconciliationState::Unresolved
    );
    let after = rig
        .store()
        .await
        .inspect(&operation)
        .await
        .expect("read dispatch again");
    assert_eq!(after, before, "reconcile leaves the marked row alone");
    let live_after = host
        .held_result_for_test(&operation)
        .expect("dispatch remains live")
        .0;
    assert!(Arc::ptr_eq(&live, &live_after));
    assert_eq!(live_after.record(), live_before);
    assert!(live_after.result.lock().await.is_none());
    host.shutdown().await.expect("fixture shutdown");
}

#[tokio::test(flavor = "current_thread")]
async fn held_terminal_write_cannot_delay_an_explicit_restore_preflight_refusal() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("restored-in-auto").await;
    rig.assert_loads(&host).await;
    let operation = OperationId::generate();
    let PreparedOperation::Admitted { live, .. } = host
        .prepare_operation(
            host.admission_deadline(),
            operation.clone(),
            ProviderOperationKind::ConversationLoad,
            binding(),
            Some(&rig.target),
        )
        .await
        .expect("a second explicit restore is admitted to the Host")
    else {
        panic!("fresh operation must be admitted");
    };
    host.inner
        .store
        .lock()
        .await
        .mark_may_have_dispatched(&operation, host.inner.now_ms())
        .await
        .expect("mark before owner handoff");
    live.dispatch_marked();
    let request = rig.load_request(operation.clone());
    let record = Box::new(provider_session_record(
        request.target,
        request.working_directory,
        request.requested_policy,
        request.requested_by,
        request.approver,
        collaboration_runtime::ProviderSessionResumeInputs::default(),
        host.inner.now_ms(),
    ));
    let completion = crate::external_provider_supervisor::provider_owner_operations::restore(
        &host,
        operation.clone(),
        record,
        provider_session_machines::session_lifecycle_machine::RestoreVia::Load,
    )
    .await;
    let ProviderOperationCompletion::Failure(refusal) = &completion else {
        panic!("the live owner must refuse a second restore in preflight");
    };
    assert_eq!(refusal.stage, ConversationOperationFailureStage::Admission);
    assert_eq!(refusal.effect, ProviderOperationEffect::None);
    let expected = refusal.clone();

    let mut held = host.inner.store.lock().await;
    let finish = host.inner.finish(operation.clone(), live, completion);
    tokio::pin!(finish);
    assert!(futures_util::poll!(&mut finish).is_pending());
    rig.clock
        .advance(Duration::from_secs(10))
        .expect("reach the terminal write deadline");
    assert!(futures_util::poll!(&mut finish).is_pending());
    let mut wait = host.wait(wait_request(operation.clone()));
    let Poll::Ready(Err(answer)) = futures_util::poll!(&mut wait) else {
        panic!("the restore refusal must reach the caller while terminal storage is held");
    };
    assert_eq!(answer.kind, expected.kind);
    assert_eq!(answer.stage, expected.stage);
    assert_eq!(answer.effect, expected.effect);
    assert_eq!(answer.message, expected.message);
    let marked = held
        .inspect(&operation)
        .await
        .expect("read marked row")
        .expect("operation exists");
    assert_eq!(marked.stage, ProviderOperationStage::MayHaveDispatched);
    assert_eq!(marked.effect, ProviderOperationEffect::Unknown);
    drop(held);
    finish.await;

    let terminal = rig
        .store()
        .await
        .inspect(&operation)
        .await
        .expect("read terminal row")
        .expect("operation exists");
    assert_eq!(terminal.stage, ProviderOperationStage::Terminal);
    assert_eq!(terminal.effect, ProviderOperationEffect::None);
    assert_eq!(
        terminal.reconciliation_state,
        ProviderReconciliationState::Confirmed
    );
    host.shutdown().await.expect("fixture shutdown");
}
