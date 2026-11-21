use super::*;

#[tokio::test(flavor = "current_thread")]
async fn a_marked_create_refuses_an_independent_direct_prompt() {
    check_marked_create_against_idle_prompt(PromptBusyPolicy::RefuseBusy).await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_marked_create_refuses_auto_ahead_of_an_idle_start() {
    check_marked_create_against_idle_prompt(PromptBusyPolicy::Send(MessageDelivery::Auto)).await;
}

async fn check_marked_create_against_idle_prompt(busy: PromptBusyPolicy) {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("restored-in-auto").await;
    rig.assert_loads(&host).await;
    let (reached, release) = host
        .runtime_for(&rig.target.endpoint)
        .expect("provider runtime")
        .preparation_fixture
        .pause_create();
    let create = OperationId::generate();
    host.create(create_request(&rig, create.clone()))
        .await
        .expect("create admitted");
    reached
        .await
        .expect("hold the provider create after its mark");
    let marked = host
        .show(ConversationOperationShowRequest {
            operation_id: create.clone(),
        })
        .await
        .expect("marked create");
    assert_eq!(marked.stage, ProviderOperationStage::MayHaveDispatched);
    assert!(marked.target.is_none(), "create blocks the whole binding");

    let prompt = OperationId::generate();
    let mut request = ProviderPromptContentsRequest::from_prompt_request(
        rig.prompt_request(prompt.clone(), "must stay local"),
    )
    .unwrap();
    request.busy = busy;
    let refusal = host
        .prompt_contents(request)
        .await
        .expect_err("independent idle prompt overlapped a marked wide create");
    assert_eq!(refusal.kind, ConversationOperationFailureKind::Busy);
    assert_eq!(refusal.effect, ProviderOperationEffect::None);
    assert!(String::from(refusal.message).contains(create.as_str()));
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
    let wire = rig.wire_receipt();
    assert!(wire["prompts"].as_array().unwrap().is_empty());

    release.send(()).expect("release the creating session");
    host.wait(wait_request(create))
        .await
        .expect("create settled");
    host.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "current_thread")]
async fn a_binding_wide_reservation_blocks_a_second_create_before_any_sql() {
    check_binding_wide_reservation(ProviderOperationKind::ConversationCreate).await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_binding_wide_reservation_blocks_targeted_load_before_any_sql() {
    check_binding_wide_reservation(ProviderOperationKind::ConversationLoad).await;
}

async fn check_binding_wide_reservation(second_kind: ProviderOperationKind) {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("restored-in-auto").await;
    let first = OperationId::generate();
    let second = OperationId::generate();
    let create_request = |operation_id| create_request(&rig, operation_id);
    let (pause, second_at_reservation, release_second) = admission_pause();
    *host
        .inner
        .admission_before_reservation_test_pause
        .lock()
        .unwrap() = Some(pause);
    let mut competing = if second_kind == ProviderOperationKind::ConversationCreate {
        host.create(create_request(second.clone()))
    } else {
        host.load(rig.load_request(second.clone()))
    };
    tokio::select! {
        reached = second_at_reservation => reached.expect("the second request reached reservation"),
        result = &mut competing => panic!("the competing request answered early: {result:?}"),
    }

    let (pause, indexed, release_first) = admission_pause();
    *host.inner.admission_test_pause.lock().unwrap() = Some(pause);
    let mut holding = host.create(create_request(first.clone()));
    tokio::select! {
        reached = indexed => reached.expect("the first create reserved before SQL"),
        result = &mut holding => panic!("the first create answered before writing: {result:?}"),
    }
    assert!(host.held_result_for_test(&first).unwrap().0.is_reserving());
    assert!(
        host.inner
            .store
            .lock()
            .await
            .inspect(&first)
            .await
            .unwrap()
            .is_none()
    );

    release_second
        .send(())
        .expect("run the competing reservation");
    let failure = competing
        .await
        .expect_err("the competing operation must not acquire custody");
    assert_eq!(failure.kind, ConversationOperationFailureKind::Busy);
    assert_eq!(failure.effect, ProviderOperationEffect::None);
    assert!(String::from(failure.message).contains(first.as_str()));
    assert!(host.held_result_for_test(&second).is_none());
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

    release_first.send(()).expect("finish the first create");
    holding.await.expect("first create admitted");
    let completion = host
        .wait(wait_request(first))
        .await
        .expect("first create completed");
    assert!(matches!(
        completion.output,
        ConversationOperationWaitOutput::Available {
            settlement: ConversationOperationSettlement::Created { .. }
        }
    ));
    assert_eq!(
        rig.wire_receipt()["sessions"]
            .as_array()
            .expect("created sessions")
            .len(),
        1
    );
    host.shutdown().await.expect("drain the selected create");
}

fn create_request(rig: &RestoreRig, operation_id: OperationId) -> ConversationCreateRequest {
    ConversationCreateRequest {
        operation_id,
        endpoint: rig.target.endpoint.clone(),
        generation: Some(generation()),
        working_directory: rig.working_directory(),
        created_by: rig.target.clone().into(),
        approver: rig.target.clone().into(),
        requested_policy: policy(),
        settings: Some(collaboration_protocol::ProviderRequestedSettings {
            mode: Some("auto".into()),
            ..Default::default()
        }),
        work_roots: vec![],
    }
}
