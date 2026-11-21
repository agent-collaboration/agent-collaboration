use super::*;
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
