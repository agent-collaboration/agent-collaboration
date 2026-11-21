use super::admission_race_tests::create_request;
use super::*;
use crate::binding_store_fixture::BindingFixtureWait;

#[tokio::test(flavor = "current_thread")]
async fn a_known_unsent_lane_terminal_releases_its_receipt_fault_blocker() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("restored-in-auto").await;
    rig.assert_loads(&host).await;
    let faulted = OperationId::generate();
    host.inner
        .store
        .lock()
        .await
        .fail_next_dispatch_mark_receipt_of(&faulted);

    host.prompt(rig.prompt_request(faulted.clone(), "mark with a lost receipt"))
        .await
        .expect("the faulted prompt was admitted");
    let unsent = host
        .wait(wait_request(faulted.clone()))
        .await
        .expect_err("a lost mark receipt cannot send");
    assert_eq!(unsent.effect, ProviderOperationEffect::None);
    let row = host
        .inner
        .store
        .lock()
        .await
        .inspect(&faulted)
        .await
        .unwrap()
        .expect("durable terminal row");
    assert_eq!(row.stage, ProviderOperationStage::Terminal);
    assert_eq!(row.effect, ProviderOperationEffect::None);
    assert_eq!(
        row.reconciliation_state,
        ProviderReconciliationState::Confirmed
    );
    assert!(rig.wire_receipt()["prompts"].as_array().unwrap().is_empty());

    let next = OperationId::generate();
    host.prompt(rig.prompt_request(next.clone(), "next independent prompt"))
        .await
        .expect("a row confirmed unsent cannot block the next prompt");
    host.wait(wait_request(next))
        .await
        .expect("the next prompt sends");

    let (create_held, release_create) = host
        .runtime_for(&rig.target.endpoint)
        .unwrap()
        .preparation_fixture
        .pause_create();
    let create = OperationId::generate();
    host.create(create_request(&rig, create.clone()))
        .await
        .unwrap();
    create_held
        .await
        .expect("the create holds the dispatch gate");
    let parked = host
        .inner
        .uncertainty
        .slot(&rig.target.endpoint)
        .fixture
        .notice_wait(rig.target.clone(), BindingFixtureWait::QueuedDispatchParked);
    let head = OperationId::generate();
    let request = ProviderPromptContentsRequest::from_prompt_request(
        rig.prompt_request(head.clone(), "retry the parked head"),
    )
    .unwrap();
    assert_eq!(
        host.submit_owned_delivery(request, MessageDelivery::Queue)
            .await
            .unwrap(),
        crate::session_owner_calls::InputAcceptance::Queued
    );
    let wait = host.wait(wait_request(head));
    tokio::pin!(wait);
    tokio::select! {
        reached = parked => reached.expect("the head parks on the live create"),
        result = &mut wait => panic!("the head settled before it parked: {result:?}"),
    }
    release_create.send(()).expect("release the live blocker");
    host.wait(wait_request(create))
        .await
        .expect("create completes");
    assert!(matches!(
        wait.await.expect("parked head resumes").output,
        ConversationOperationWaitOutput::Available {
            settlement: ConversationOperationSettlement::PromptCompleted { .. }
        }
    ));
    assert_eq!(rig.wire_receipt()["prompts"].as_array().unwrap().len(), 2);
    host.shutdown().await.expect("shutdown");
}
