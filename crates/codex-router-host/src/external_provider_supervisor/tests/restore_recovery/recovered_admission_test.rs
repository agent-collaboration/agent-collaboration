use super::*;
use crate::session_owner_door::OwnerDoorError;
use provider_session_machines::{
    session_lifecycle_machine::{CloseAttempt, SessionLifecycle},
    session_owner::SessionOwnerCommand,
};

async fn recovered_prompt_failure(taken: bool, recovery_first: bool) {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("silent-prompts").await;
    rig.assert_loads(&host).await;
    let old_owner = host.session_owner_for(&rig.target).expect("session owner");
    let runtime = host.runtime_for(&rig.target.endpoint).expect("runtime");
    let acknowledgement = runtime.preparation_fixture.pause_delivery_ack(taken);
    let (recovery_reached, release_recovery) = runtime.preparation_fixture.pause_recovery();
    // Saturation fails the owner when it tries to reserve the mark receipt,
    // before it can create a transport authorization.
    let capacity = (!taken).then(|| old_owner.hold_completion_capacity());
    let operation = OperationId::generate();
    let prompt = host.prompt(rig.prompt_request(operation.clone(), "in-flight admission"));
    tokio::pin!(prompt);
    tokio::select! {
        result = acknowledgement.reached => result.expect("delivery acknowledgement edge"),
        result = &mut prompt => panic!("prompt answered before its acknowledgement edge: {result:?}"),
    }
    drop(capacity);

    if taken {
        // Discard the acknowledgement while keeping a real writer take. A
        // different command then fails the old owner before a prompt result.
        rig.until_wire_request("prompts").await;
        let snapshot = old_owner.snapshots().borrow().clone();
        let SessionLifecycle::Live { generation } = snapshot.lifecycle else {
            panic!("taken prompt must have a live owner");
        };
        let capacity = old_owner.hold_completion_capacity();
        let failure = old_owner
            .admit(SessionOwnerCommand::Close {
                generation,
                attempt: CloseAttempt(uuid::Uuid::now_v7()),
            })
            .await
            .expect_err("owner stops before acknowledging close");
        assert!(matches!(failure, OwnerDoorError::OwnerStopped));
        drop(capacity);
    }

    recovery_reached
        .await
        .expect("failed owner recovery has its evidence");
    if recovery_first {
        release_recovery.send(()).expect("release recovery");
        old_owner.wait_for_recovery_replies().await;
        acknowledgement
            .release
            .send(())
            .expect("release acknowledgement");
        acknowledgement
            .finished
            .await
            .expect("caller handles acknowledgement");
    } else {
        acknowledgement
            .release
            .send(())
            .expect("release acknowledgement");
        acknowledgement
            .finished
            .await
            .expect("caller handles acknowledgement first");
        release_recovery.send(()).expect("release recovery");
    }
    prompt
        .await
        .expect("admitted operation exposes the recovered result");
    let failure = host
        .wait(wait_request(operation.clone()))
        .await
        .expect_err("prompt did not settle successfully");
    if taken {
        assert_eq!(failure.effect, ProviderOperationEffect::Unknown);
        assert_eq!(
            failure.kind,
            ConversationOperationFailureKind::OutcomeUnknown
        );
        assert_eq!(
            rig.wire_receipt()["prompts"],
            serde_json::json!([rig.target.session_id])
        );
    } else {
        assert_eq!(failure.effect, ProviderOperationEffect::None);
        assert_eq!(
            String::from(failure.message),
            "the provider connection ended; reconnect and send again; the saved result isn't available and may differ",
        );
        assert_eq!(rig.wire_receipt()["prompts"], serde_json::json!([]));
        assert_unsent_row(&host, &operation).await;
        rig.assert_loads(&host).await;
    }
    host.shutdown().await.expect("shut down fixture");
    if !taken {
        let reopened = rig.open_host("restored-in-auto").await;
        assert_unsent_row(&reopened, &operation).await;
        let wait = reopened
            .wait(wait_request(operation))
            .await
            .expect("after restart only the original admission row remains");
        assert!(matches!(
            wait.output,
            ConversationOperationWaitOutput::Pending
        ));
        assert_eq!(wait.operation.stage, ProviderOperationStage::Admitted);
        assert_eq!(wait.operation.effect, ProviderOperationEffect::None);

        rig.assert_loads(&reopened).await;
        let later = OperationId::generate();
        reopened
            .prompt(rig.prompt_request(later.clone(), "a new operation"))
            .await
            .expect("the unmarked row does not hold new input");
        let settled = reopened
            .wait(wait_request(later))
            .await
            .expect("new prompt settles");
        assert!(matches!(
            settled.output,
            ConversationOperationWaitOutput::Available {
                settlement: ConversationOperationSettlement::PromptCompleted { .. }
            }
        ));
        reopened
            .shutdown()
            .await
            .expect("shut down reopened fixture");
    }
}

async fn assert_unsent_row(host: &ExternalProviderSupervisor, operation: &OperationId) {
    let shown = host
        .show(ConversationOperationShowRequest {
            operation_id: operation.clone(),
        })
        .await
        .expect("show the retained admission row");
    assert_eq!(shown.stage, ProviderOperationStage::Admitted);
    assert_eq!(shown.effect, ProviderOperationEffect::None);
    assert!(shown.queue_state.is_none());
    assert!(
        !host
            .inner
            .store
            .lock()
            .await
            .blocking_uncertain_operation_ids(&binding(), shown.target.as_ref())
            .await
            .expect("query uncertain operations")
            .contains(operation)
    );
}

#[tokio::test]
async fn a_lost_admission_ack_keeps_unsent_recovery_when_the_caller_is_first() {
    recovered_prompt_failure(false, false).await;
}

#[tokio::test]
async fn a_lost_admission_ack_keeps_unsent_recovery_when_recovery_is_first() {
    recovered_prompt_failure(false, true).await;
}

#[tokio::test]
async fn a_lost_admission_ack_keeps_taken_recovery_when_the_caller_is_first() {
    recovered_prompt_failure(true, false).await;
}

#[tokio::test]
async fn a_lost_admission_ack_keeps_taken_recovery_when_recovery_is_first() {
    recovered_prompt_failure(true, true).await;
}
