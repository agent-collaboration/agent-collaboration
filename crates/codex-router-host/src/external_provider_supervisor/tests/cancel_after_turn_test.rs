use super::*;

#[tokio::test]
async fn an_admitted_cancel_after_the_target_ends_settles_known_unsent() {
    use collaboration_protocol::{
        ConversationCancelRequest, ConversationOperationShowRequest, MessageDelivery,
        ProviderOperationStage,
    };

    let rig = steering_session_on_clock(
        "cursor-settings-agent.ts",
        "reactive-session-1",
        |receipt| {
            vec![
                "reactive".to_owned(),
                receipt.display().to_string(),
                "hold-first-prompt-open".to_owned(),
                "prompt-directives".to_owned(),
            ]
        },
        Arc::new(router_clock::ManualClock::new(chrono::DateTime::UNIX_EPOCH)),
    )
    .await;
    let prompt = OperationId::generate();
    rig.supervisor
        .prompt(ConversationPromptRequest {
            delivery: MessageDelivery::Auto,
            operation_id: prompt.clone(),
            input_id: None,
            target: rig.target.clone(),
            generation: Some(binding().generation),
            requested_by: rig.target.clone().into(),
            approver: rig.target.clone().into(),
            prompt: collaboration_protocol::MessageContent::HumanUser {
                text: MessageText::try_from("first".to_owned()).expect("prompt text"),
            },
        })
        .await
        .expect("prompt admitted");
    while recorded_requests(&rig.receipt, "session/prompt") != 1 {
        tokio::task::yield_now().await;
    }
    let (entered, reached) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    *rig.supervisor
        .inner
        .cancel_admission_test_pause
        .lock()
        .expect("cancel pause") = Some(AdmissionTestPause {
        entered,
        release: released,
    });
    let cancel = OperationId::generate();
    rig.supervisor
        .cancel(ConversationCancelRequest {
            operation_id: cancel.clone(),
            target_operation_id: prompt.clone(),
            target: rig.target.clone(),
            generation: Some(binding().generation),
            requested_by: rig.target.clone().into(),
            approver: rig.target.clone().into(),
        })
        .await
        .expect("cancel admitted while the turn is active");
    reached
        .await
        .expect("cancel reached the owner admission boundary");
    node_test_fixture::release_fixture(&rig.receipt, ".release").expect("finish the prompt");
    let wait = |operation_id| ConversationOperationWaitRequest {
        operation_id,
        timeout_seconds: PositiveSeconds::try_from(10).expect("bounded wait"),
    };
    let prompt_result = rig
        .supervisor
        .wait(wait(prompt))
        .await
        .expect("prompt ended");
    assert!(matches!(
        prompt_result.output,
        ConversationOperationWaitOutput::Available {
            settlement: ConversationOperationSettlement::PromptCompleted { .. }
        }
    ));
    release.send(()).expect("let cancel consult the owner");

    let refusal = rig
        .supervisor
        .wait(wait(cancel.clone()))
        .await
        .expect_err("target has ended");
    assert_eq!(refusal.kind, ConversationOperationFailureKind::NotFound);
    assert_eq!(refusal.effect, ProviderOperationEffect::None);
    assert_eq!(
        String::from(refusal.message),
        "target provider prompt operation is not active on this binding"
    );
    let row = rig
        .supervisor
        .show(ConversationOperationShowRequest {
            operation_id: cancel,
        })
        .await
        .expect("durable cancel");
    assert_eq!(row.stage, ProviderOperationStage::Terminal);
    assert_eq!(row.effect, ProviderOperationEffect::None);
    assert_eq!(
        row.reconciliation,
        collaboration_protocol::ProviderReconciliationState::Confirmed
    );
    assert_eq!(recorded_requests(&rig.receipt, "session/cancel"), 0);
    rig.supervisor.shutdown().await.expect("fixture shutdown");
}
