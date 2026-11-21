use super::admission_race_tests::create_request;
use super::*;
use crate::binding_store_fixture::BindingFixtureWait;
use crate::external_provider_supervisor::PromptBusyPolicy;
use std::time::Duration;

#[tokio::test(flavor = "current_thread")]
async fn an_admitted_independent_prompt_blocks_before_its_owner_command() {
    independent_reservation_precedes_the_owner_command(false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_rowless_idle_auto_reservation_blocks_before_its_owner_command() {
    independent_reservation_precedes_the_owner_command(true).await;
}

async fn independent_reservation_precedes_the_owner_command(rowless: bool) {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("restored-in-auto").await;
    rig.assert_loads(&host).await;
    let first = OperationId::generate();
    let second = OperationId::generate();
    let (pause, before_owner, let_owner_run) = admission_pause();
    *host.inner.before_owner_prompt_test_pause.lock().unwrap() = Some((first.clone(), pause));
    let first_request = rig.prompt_request(first.clone(), "first independent request");
    let first_submission = async {
        if rowless {
            let request = ProviderPromptContentsRequest::from_prompt_request(first_request)
                .expect("valid Auto contents");
            host.submit_owned_delivery(request, MessageDelivery::Auto)
                .await
                .expect("Auto request admitted");
        } else {
            let mut request = ProviderPromptContentsRequest::from_prompt_request(first_request)
                .expect("valid direct contents");
            request.busy = PromptBusyPolicy::RefuseBusy;
            host.prompt_contents(request)
                .await
                .expect("direct prompt admitted");
        }
    };
    tokio::pin!(first_submission);
    tokio::select! {
        reached = before_owner => reached.expect("first reservation reached the owner boundary"),
        () = &mut first_submission => panic!("the first request passed its pause"),
    }
    let row = host.inner.store.lock().await.inspect(&first).await.unwrap();
    assert_eq!(
        row.as_ref().map(|row| row.stage),
        (!rowless).then_some(ProviderOperationStage::Admitted),
    );

    let refusal = host
        .prompt(rig.prompt_request(second.clone(), "competing independent prompt"))
        .await
        .expect_err("the second independent request must be refused at admission");
    assert_eq!(refusal.stage, ConversationOperationFailureStage::Validation);
    assert_eq!(refusal.kind, ConversationOperationFailureKind::Busy);
    assert!(String::from(refusal.message).contains(first.as_str()));
    assert!(
        host.inner
            .store
            .lock()
            .await
            .inspect(&second)
            .await
            .unwrap()
            .is_none()
    );
    assert!(rig.wire_receipt()["prompts"].as_array().unwrap().is_empty());

    let_owner_run.send(()).expect("release the first request");
    first_submission.await;
    host.wait(wait_request(first))
        .await
        .expect("first request completes");
    assert_eq!(rig.wire_receipt()["prompts"].as_array().unwrap().len(), 1);
    host.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "current_thread")]
async fn the_owner_line_progresses_while_an_independent_reservation_has_no_mark() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("restored-in-auto").await;
    rig.assert_loads(&host).await;
    let independent = OperationId::generate();
    let queue_head = OperationId::generate();
    let (pause, before_owner, let_owner_run) = admission_pause();
    *host.inner.before_owner_prompt_test_pause.lock().unwrap() = Some((independent.clone(), pause));
    let mut request = ProviderPromptContentsRequest::from_prompt_request(
        rig.prompt_request(independent.clone(), "held independent"),
    )
    .unwrap();
    request.busy = PromptBusyPolicy::RefuseBusy;
    let first = host.prompt_contents(request);
    tokio::pin!(first);
    tokio::select! {
        reached = before_owner => reached.expect("independent reservation before its owner command"),
        result = &mut first => panic!("independent request returned before the hold: {result:?}"),
    }
    let parked = host
        .inner
        .uncertainty
        .slot(&rig.target.endpoint)
        .fixture
        .notice_wait(rig.target.clone(), BindingFixtureWait::QueuedDispatchParked);
    let queued = ProviderPromptContentsRequest::from_prompt_request(
        rig.prompt_request(queue_head.clone(), "queue reaches the owner first"),
    )
    .unwrap();
    assert_eq!(
        host.submit_owned_delivery(queued, MessageDelivery::Queue)
            .await
            .unwrap(),
        crate::session_owner_calls::InputAcceptance::Queued
    );
    let request = wait_request(queue_head);
    let budget = Duration::from_secs(u32::from(request.timeout_seconds).into());
    let waiting = host.wait(request);
    tokio::pin!(waiting);
    let mut release = Some(let_owner_run);
    let completed = tokio::select! {
        biased;
        result = &mut waiting => result,
        reached = parked => {
            reached.expect("the queue head parked");
            release.take().unwrap().send(()).expect("release the independent request");
            assert!(futures_util::poll!(&mut first).is_pending());
            rig.clock.advance(budget).expect("bound the parked head with the injected clock");
            waiting.await
        }
    }
    .expect("queue head result");
    assert!(
        matches!(
            completed.output,
            ConversationOperationWaitOutput::Available {
                settlement: ConversationOperationSettlement::PromptCompleted { .. }
            }
        ),
        "the parked head cannot wait for a reservation whose command waits behind it: {completed:?}"
    );
    if let Some(release) = release {
        release.send(()).expect("release the independent request");
    }
    first
        .await
        .expect("the independent request reaches the owner after the head");
    host.wait(wait_request(independent))
        .await
        .expect("both inputs settle");
    assert_eq!(rig.wire_receipt()["prompts"].as_array().unwrap().len(), 2);
    host.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "current_thread")]
async fn a_lost_host_mark_receipt_transfers_its_durable_blocker_before_release() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("restored-in-auto").await;
    rig.assert_loads(&host).await;
    let operation = OperationId::generate();
    host.inner
        .store
        .lock()
        .await
        .fail_next_dispatch_mark_receipt_of(&operation);

    host.create(create_request(&rig, operation.clone()))
        .await
        .expect("create admitted");
    assert_eq!(
        host.wait(wait_request(operation.clone()))
            .await
            .expect_err("the lost mark receipt cannot authorize create")
            .effect,
        ProviderOperationEffect::None
    );
    let row = host
        .inner
        .store
        .lock()
        .await
        .inspect(&operation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.stage, ProviderOperationStage::MayHaveDispatched);
    assert_eq!(row.effect, ProviderOperationEffect::Unknown);
    assert!(!host.live_operation_ids().contains(&operation));
    assert!(
        rig.wire_receipt()["sessions"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let prompt = OperationId::generate();
    let refusal = host
        .prompt(rig.prompt_request(prompt.clone(), "unknown after receipt loss"))
        .await
        .expect_err("the actual wide mark must keep blocking after custody release");
    assert_eq!(refusal.kind, ConversationOperationFailureKind::Busy);
    assert!(String::from(refusal.message).contains(operation.as_str()));
    assert!(
        host.inner
            .store
            .lock()
            .await
            .inspect(&prompt)
            .await
            .unwrap()
            .is_none()
    );
    host.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "current_thread")]
async fn a_queued_first_mark_has_eligibility_before_its_receipt() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("restored-in-auto").await;
    rig.assert_loads(&host).await;
    let queued = OperationId::generate();
    let competing = OperationId::generate();
    let (pause, before_reservation, resume) = admission_pause();
    *host
        .inner
        .admission_before_reservation_test_pause
        .lock()
        .unwrap() = Some(pause);
    let admission = host.prepare_operation(
        host.admission_deadline(),
        competing.clone(),
        ProviderOperationKind::ConversationLoad,
        binding(),
        Some(&rig.target),
    );
    tokio::pin!(admission);
    tokio::select! {
        reached = before_reservation => reached.expect("competing request made its durable prechecks"),
        _ = &mut admission => panic!("competing request reached admission too early"),
    }
    let mut commit = host
        .inner
        .store
        .lock()
        .await
        .pause_next_admission_after_commit_for_test();
    let (terminal_reached, terminal_release) = host.pause_terminal_write_of(&rig.target, &queued);
    let request = ProviderPromptContentsRequest::from_prompt_request(
        rig.prompt_request(queued.clone(), "first owner-lane mark"),
    )
    .unwrap();
    let placed = host
        .submit_owned_delivery(request, MessageDelivery::Queue)
        .await
        .expect("queue placement");
    assert_eq!(placed, crate::session_owner_calls::InputAcceptance::Queued);
    commit.reached().await;

    resume
        .send(())
        .expect("resume the competing operation against the committed mark");
    assert!(futures_util::poll!(&mut admission).is_pending());
    assert!(
        host.held_result_for_test(&competing).is_none(),
        "a conflicting reservation passed before the queued mark reached its projection"
    );

    commit.release();
    let refusal = match admission.await {
        Err(refusal) => refusal,
        Ok(_) => panic!("the first durable mark must block the competing load"),
    };
    assert_eq!(refusal.kind, ConversationOperationFailureKind::Busy);
    assert!(String::from(refusal.message).contains(queued.as_str()));
    assert!(
        host.inner
            .store
            .lock()
            .await
            .inspect(&competing)
            .await
            .unwrap()
            .is_none()
    );
    terminal_reached
        .await
        .expect("the queued prompt's own terminal write");
    terminal_release
        .send(())
        .expect("release the queued terminal write");
    host.wait(wait_request(queued))
        .await
        .expect("queued prompt completes");
    host.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "current_thread")]
async fn a_queued_head_waits_behind_a_wide_mark_then_sends_in_fifo_order() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("restored-in-auto").await;
    rig.assert_loads(&host).await;
    let (create_held, release_create) = host
        .runtime_for(&rig.target.endpoint)
        .unwrap()
        .preparation_fixture
        .pause_create();
    let create = OperationId::generate();
    host.create(create_request(&rig, create.clone()))
        .await
        .unwrap();
    create_held.await.expect("the wide create is marked");
    let parked = host
        .inner
        .uncertainty
        .slot(&rig.target.endpoint)
        .fixture
        .notice_wait(rig.target.clone(), BindingFixtureWait::QueuedDispatchParked);
    let head = OperationId::generate();
    let tail = OperationId::generate();
    for (operation, text) in [(&head, "first queued"), (&tail, "second queued")] {
        let request = ProviderPromptContentsRequest::from_prompt_request(
            rig.prompt_request(operation.clone(), text),
        )
        .unwrap();
        assert_eq!(
            host.submit_owned_delivery(request, MessageDelivery::Queue)
                .await
                .unwrap(),
            crate::session_owner_calls::InputAcceptance::Queued
        );
    }
    tokio::select! {
        reached = parked => reached.expect("the actual head was blocked before its mark"),
        result = host.wait(wait_request(head.clone())) => panic!("queue head answered while the wide mark was held: {result:?}"),
    }
    let owner = host.session_owner_for(&rig.target).unwrap();
    let mut snapshot = owner.snapshots();
    snapshot
        .wait_for(|snapshot| {
            snapshot.queue.len() == 2 && snapshot.queue.iter().all(|entry| !entry.started)
        })
        .await
        .expect("two parked entries");
    assert!(rig.wire_receipt()["prompts"].as_array().unwrap().is_empty());
    for operation in [&head, &tail] {
        assert!(
            host.inner
                .store
                .lock()
                .await
                .inspect(operation)
                .await
                .unwrap()
                .is_none()
        );
    }

    release_create.send(()).expect("release the blocker");
    host.wait(wait_request(create))
        .await
        .expect("create completes");
    for operation in [&head, &tail] {
        let completed = host
            .wait(wait_request(operation.clone()))
            .await
            .expect("queued request finishes");
        assert!(matches!(
            completed.output,
            ConversationOperationWaitOutput::Available {
                settlement: ConversationOperationSettlement::PromptCompleted { .. }
            }
        ));
    }
    assert_eq!(rig.wire_receipt()["prompts"].as_array().unwrap().len(), 2);
    host.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "current_thread")]
async fn a_failed_late_cleanup_keeps_the_committed_wide_mark_blocking_after_release() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("restored-in-auto").await;
    rig.assert_loads(&host).await;
    let operation = OperationId::generate();
    let (pause, before_mark, release_mark) = admission_pause();
    *host.inner.dispatch_mark_test_pause.lock().unwrap() = Some(pause);
    host.create(create_request(&rig, operation.clone()))
        .await
        .unwrap();
    before_mark
        .await
        .expect("create is admitted and ready to mark");
    let queued = OperationId::generate();
    let parked = host
        .inner
        .uncertainty
        .slot(&rig.target.endpoint)
        .fixture
        .notice_wait(rig.target.clone(), BindingFixtureWait::QueuedDispatchParked);
    let request = ProviderPromptContentsRequest::from_prompt_request(
        rig.prompt_request(queued.clone(), "blocked by the wide create"),
    )
    .unwrap();
    assert_eq!(
        host.submit_owned_delivery(request, MessageDelivery::Queue)
            .await
            .unwrap(),
        crate::session_owner_calls::InputAcceptance::Queued
    );
    parked
        .await
        .expect("the head initially waits for a live holder");
    let mut store = host.inner.store.lock().await;
    store.fail_next_terminal_write_of(&operation);
    let mut commit = store.pause_next_admission_after_commit_for_test();
    drop(store);
    let (published, release_publication) = host.pause_after_result_publication(operation.clone());
    release_mark.send(()).unwrap();
    commit.reached().await;
    rig.clock.advance(Duration::from_secs(10)).unwrap();
    published
        .await
        .expect("known-unsent answer published before late cleanup");
    assert_eq!(
        host.wait(wait_request(operation.clone()))
            .await
            .expect_err("creation was not authorized")
            .effect,
        ProviderOperationEffect::None
    );
    release_publication.send(()).unwrap();
    commit.release();
    let tracked = host.tracked_operation_tasks_for_test();
    tracked.close();
    tracked.wait().await;
    let row = host
        .inner
        .store
        .lock()
        .await
        .inspect(&operation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.stage, ProviderOperationStage::MayHaveDispatched);
    assert_eq!(row.effect, ProviderOperationEffect::Unknown);
    assert!(
        !host.live_operation_ids().contains(&operation),
        "the failed cleanup receipt ended live custody"
    );
    let queued_failure = host
        .wait(wait_request(queued))
        .await
        .expect_err("retry sees durable unknown and settles not sent");
    assert_eq!(queued_failure.effect, ProviderOperationEffect::None);
    assert_eq!(
        String::from(queued_failure.message),
        format!(
            "an earlier prompt's outcome is unknown; check the session, then run operation reconcile for {}",
            operation.as_str()
        )
    );
    assert!(rig.wire_receipt()["prompts"].as_array().unwrap().is_empty());

    let prompt = OperationId::generate();
    let refusal = host
        .prompt(rig.prompt_request(prompt.clone(), "durable blocker"))
        .await
        .expect_err("committed residual must still refuse an independent start");
    assert_eq!(refusal.kind, ConversationOperationFailureKind::Busy);
    assert!(String::from(refusal.message).contains(operation.as_str()));
    assert!(
        host.inner
            .store
            .lock()
            .await
            .inspect(&prompt)
            .await
            .unwrap()
            .is_none()
    );
    host.shutdown().await.expect("shutdown");
}
