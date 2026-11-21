use super::*;
use collaboration_protocol::ProviderOperationStage;
use std::{task::Poll, time::Duration};

#[tokio::test(flavor = "current_thread")]
async fn owner_reply_unsaved_leaves_its_accepted_terminal_writer_in_custody() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("restored-in-auto").await;
    rig.assert_loads(&host).await;
    let operation = OperationId::generate();
    let (write_held, write_release) = host.pause_terminal_write_of(&rig.target, &operation);
    let (published, publication_release) = host.pause_after_result_publication(operation.clone());
    host.prompt(rig.prompt_request(operation.clone(), "answer with the write held"))
        .await
        .expect("prompt admitted");
    write_held
        .await
        .expect("the owner lane accepted its terminal write");
    let deadline = rig.clock.monotonic_now().after(Duration::from_secs(10));
    rig.clock.not_due_check_now(deadline).await;
    rig.clock.advance(Duration::from_secs(10)).expect("reach L");
    published
        .await
        .expect("the owner published its unsaved answer at L");
    let failure = host
        .wait(wait_request(operation.clone()))
        .await
        .expect_err("the owner settlement is not yet durable");
    assert_eq!(failure.effect, ProviderOperationEffect::Applied);
    assert_eq!(failure.stage, ConversationOperationFailureStage::Settlement);

    let snapshot = host
        .reconcile(ConversationOperationReconcileRequest {
            operation_id: operation.clone(),
        })
        .await
        .expect("reconcile reads the row without taking custody");
    assert_eq!(snapshot.stage, ProviderOperationStage::MayHaveDispatched);
    assert_eq!(
        snapshot.reconciliation,
        ProviderReconciliationState::Unresolved
    );
    assert!(host.live_operation_ids().contains(&operation));

    publication_release
        .send(())
        .expect("release the already published answer");
    write_release
        .send(())
        .expect("release the matched lane write");
    host.shutdown().await.expect("drain the operation");
    let terminal = host
        .show(ConversationOperationShowRequest {
            operation_id: operation.clone(),
        })
        .await
        .expect("the owned writer saved its effect");
    assert_eq!(terminal.stage, ProviderOperationStage::Terminal);
    assert_eq!(terminal.effect, ProviderOperationEffect::Applied);
    assert_eq!(
        terminal.reconciliation,
        ProviderReconciliationState::Confirmed
    );
    assert!(!host.live_operation_ids().contains(&operation));
}

#[tokio::test(flavor = "current_thread")]
async fn immediate_prompt_owner_refusal_answers_at_l_with_terminal_storage_held() {
    use provider_session_machines::{
        session_refusal_answers::session_busy_answer, session_turn_machine::SessionTurn,
    };

    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("late-cancel-confirmation").await;
    rig.assert_loads(&host).await;
    let foreground = OperationId::generate();
    host.prompt(rig.prompt_request(foreground, "held foreground"))
        .await
        .expect("foreground admitted");
    let owner = host.session_owner_for(&rig.target).expect("owner");
    let running = owner
        .snapshots()
        .wait_for(|snapshot| matches!(snapshot.turn, SessionTurn::Running { .. }))
        .await
        .expect("active turn")
        .clone();
    let steer_supported = running
        .projection
        .capabilities
        .as_ref()
        .is_some_and(|report| report.steer);
    let expected = session_busy_answer(&running.lifecycle, running.turn, steer_supported);
    let operation_id = OperationId::generate();
    let mut request = ProviderPromptContentsRequest::from_prompt_request(
        rig.prompt_request(operation_id.clone(), "refused foreground"),
    )
    .expect("valid prompt");
    request.busy = PromptBusyPolicy::RefuseBusy;
    let ack = host
        .runtime_for(&rig.target.endpoint)
        .expect("runtime")
        .preparation_fixture
        .pause_delivery_ack(false);
    let mut submit = host.submit_owner_prompt(request);
    tokio::select! {
        reached = ack.reached => reached.expect("the owner decided while its turn was running"),
        answer = &mut submit => panic!("submission answered before owner acknowledgement: {answer:?}"),
    }

    let mut store = host.inner.store.lock().await;
    let admitted = store
        .inspect(&operation_id)
        .await
        .expect("inspect admission")
        .expect("admitted row");
    assert_eq!(admitted.stage, ProviderOperationStage::Admitted);
    assert_eq!(admitted.effect, ProviderOperationEffect::None);
    let mut reconcile = host.reconcile(ConversationOperationReconcileRequest {
        operation_id: operation_id.clone(),
    });
    assert!(futures_util::poll!(&mut reconcile).is_pending());
    ack.release.send(()).expect("expose owner refusal");
    assert!(futures_util::poll!(&mut submit).is_pending());
    ack.finished.await.expect("owner acknowledgement released");
    rig.clock
        .advance(Duration::from_secs(10))
        .expect("reach the refusal deadline");
    let Poll::Ready(Err(failure)) = futures_util::poll!(&mut submit) else {
        panic!("the submitter must receive the owner refusal at L with terminal storage held");
    };
    assert_eq!(failure.kind, ConversationOperationFailureKind::Busy);
    assert_eq!(failure.stage, ConversationOperationFailureStage::Admission);
    assert_eq!(failure.effect, ProviderOperationEffect::None);
    assert_eq!(String::from(failure.message), expected);
    assert_eq!(
        store.inspect(&operation_id).await.expect("still admitted"),
        Some(admitted)
    );
    assert_eq!(owner.snapshots().borrow().turn, running.turn);
    drop(store);

    // Reconcile queued for the store first. Publication has happened, but
    // the terminal writer still needs its turn at the store.
    let during_write = reconcile.await.expect("read the operation under custody");
    assert_eq!(during_write.stage, ProviderOperationStage::Admitted);
    assert_eq!(during_write.effect, ProviderOperationEffect::None);
    assert_eq!(
        during_write.reconciliation,
        ProviderReconciliationState::Unresolved
    );
    host.shutdown()
        .await
        .expect("drain the actual terminal writer");
    let terminal = host
        .inner
        .store
        .lock()
        .await
        .inspect(&operation_id)
        .await
        .expect("read terminal row")
        .expect("finished row");
    assert_eq!(terminal.stage, ProviderOperationStage::Terminal);
    assert_eq!(terminal.effect, ProviderOperationEffect::None);
    assert_eq!(
        terminal.reconciliation_state,
        ProviderReconciliationState::Confirmed
    );
}
