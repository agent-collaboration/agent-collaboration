use super::*;
use collaboration_protocol::{ConversationCancelRequest, ConversationOperationReconcileRequest};
use provider_session_machines::{
    session_lifecycle_machine::SessionLifecycle, session_turn_machine::SessionTurn,
};
use std::time::Duration;

const CONFIRMATION_BUDGET: Duration = Duration::from_secs(30);

async fn submit_and_cancel(
    rig: &RestoreRig,
    host: &ExternalProviderSupervisor,
) -> (OperationId, OperationId) {
    let foreground = OperationId::generate();
    host.prompt(rig.prompt_request(foreground.clone(), "held prompt"))
        .await
        .expect("foreground admitted");
    rig.until_wire_request("prompts").await;
    let cancel = OperationId::generate();
    host.cancel(ConversationCancelRequest {
        operation_id: cancel.clone(),
        target_operation_id: foreground.clone(),
        target: rig.target.clone(),
        generation: Some(generation()),
        requested_by: rig.target.clone().into(),
        approver: rig.target.clone().into(),
    })
    .await
    .expect("cancel admitted");
    let settled = host
        .wait(wait_request(cancel.clone()))
        .await
        .expect("cancel settles at writer take");
    assert!(
        matches!(settled.output, ConversationOperationWaitOutput::Available {
        settlement: ConversationOperationSettlement::CancelRequested { target_operation_id, .. }
    } if target_operation_id == foreground)
    );
    rig.until_wire_request("cancels").await;
    (foreground, cancel)
}

async fn expire_confirmation(
    rig: &RestoreRig,
    host: &ExternalProviderSupervisor,
    foreground: &OperationId,
) {
    let deadline = rig.clock.monotonic_now().after(CONFIRMATION_BUDGET);
    rig.clock.not_due_check_now(deadline).await;
    rig.clock
        .advance(Duration::from_secs(29))
        .expect("just before the deadline");
    let pending = host
        .show(ConversationOperationShowRequest {
            operation_id: foreground.clone(),
        })
        .await
        .expect("prompt row");
    assert_eq!(pending.stage, ProviderOperationStage::MayHaveDispatched);
    rig.clock
        .advance(Duration::from_secs(1))
        .expect("cancel confirmation deadline");
    let failure = host
        .wait(wait_request(foreground.clone()))
        .await
        .expect_err("an unconfirmed cancel settles the foreground prompt");
    assert_eq!(
        failure.kind,
        ConversationOperationFailureKind::OutcomeUnknown
    );
    assert_eq!(failure.effect, ProviderOperationEffect::Unknown);
    assert_eq!(
        String::from(failure.message),
        format!(
            "the harness didn't stop the turn, so its outcome is unknown; close the session, then check it and run operation reconcile for {}",
            foreground.as_str(),
        )
    );
    let owner = host
        .session_owner_for(&rig.target)
        .expect("session owner stays alive");
    let snapshot = owner.snapshots().borrow().clone();
    assert!(matches!(snapshot.lifecycle, SessionLifecycle::Live { .. }));
    assert!(matches!(snapshot.turn, SessionTurn::Cancelling { .. }));
    assert_eq!(
        snapshot
            .cancel_unconfirmed
            .expect("foreground remains fenced")
            .operation
            .to_string(),
        foreground.as_str()
    );
    assert_eq!(rig.wire_receipt()["closes"], serde_json::json!([]));
}

#[tokio::test]
async fn unconfirmed_cancel_keeps_a_fence_until_the_exact_late_prompt_end_is_reconciled() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("late-cancel-confirmation").await;
    rig.assert_loads(&host).await;
    let (foreground, cancel) = submit_and_cancel(&rig, &host).await;
    let queued = OperationId::generate();
    let mut second = rig.prompt_request(queued.clone(), "queued while cancelling");
    second.delivery = MessageDelivery::Queue;
    host.prompt(second).await.expect("queue admission");
    wait_for_queued_input(&host, &rig.target, &queued).await;
    expire_confirmation(&rig, &host, &foreground).await;
    assert_eq!(
        rig.wire_receipt()["prompts"]
            .as_array()
            .expect("wire prompts")
            .len(),
        1
    );

    node_test_fixture::release_fixture(&rig.root.path().join("wire.json"), ".release-cancel")
        .expect("release exact original wire request");
    let completed = host
        .wait(wait_request(queued))
        .await
        .expect("queued prompt settles after reconciliation");
    assert!(matches!(
        completed.output,
        ConversationOperationWaitOutput::Available {
            settlement: ConversationOperationSettlement::PromptCompleted { .. }
        }
    ));
    let stored = rig
        .store()
        .await
        .inspect(&foreground)
        .await
        .expect("query durable foreground")
        .expect("durable row");
    assert_eq!(stored.effect, ProviderOperationEffect::Applied);
    assert_eq!(
        stored.reconciliation_state,
        collaboration_protocol::ProviderReconciliationState::Confirmed
    );
    assert_eq!(
        host.wait(wait_request(foreground))
            .await
            .expect_err("retained receipt is immutable")
            .effect,
        ProviderOperationEffect::Unknown
    );
    assert!(matches!(
        host.wait(wait_request(cancel))
            .await
            .expect("cancel receipt retained")
            .output,
        ConversationOperationWaitOutput::Available {
            settlement: ConversationOperationSettlement::CancelRequested { .. }
        }
    ));
    host.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "current_thread")]
async fn late_cancel_repair_cannot_write_after_the_original_lifecycle_is_evicted() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("late-cancel-confirmation").await;
    rig.assert_loads(&host).await;
    let (foreground, _) = submit_and_cancel(&rig, &host).await;
    expire_confirmation(&rig, &host, &foreground).await;
    for _ in 0..MAX_RETAINED_SETTLEMENTS {
        let later = OperationId::generate();
        host.hold_live_operation(later.clone());
        assert!(host.publish_held_result(&later).await);
    }
    assert!(
        host.held_result_for_test(&foreground).is_none(),
        "the original lifecycle was evicted"
    );
    let finished = release_late_cancel(&rig, &host);
    finished
        .await
        .expect("the real lane attempted the late repair");

    let row = host
        .show(ConversationOperationShowRequest {
            operation_id: foreground.clone(),
        })
        .await
        .expect("existing row");
    assert_eq!(row.stage, ProviderOperationStage::Terminal);
    assert_eq!(
        row.effect,
        ProviderOperationEffect::Unknown,
        "late repair needs the original retained lifecycle"
    );
    let reconciled = host
        .reconcile(ConversationOperationReconcileRequest {
            operation_id: foreground,
        })
        .await
        .expect("explicit reconcile remains available");
    assert_eq!(
        reconciled.reconciliation,
        ProviderReconciliationState::NotReconcilable
    );
    host.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "current_thread")]
async fn late_cancel_repair_and_reconcile_follow_store_guard_order() {
    for repair_first in [false, true] {
        let rig = RestoreRig::new();
        rig.seed_session().await;
        let host = rig.open_host("late-cancel-confirmation").await;
        rig.assert_loads(&host).await;
        let (foreground, _) = submit_and_cancel(&rig, &host).await;
        expire_confirmation(&rig, &host, &foreground).await;

        let mut reconcile = host.reconcile(ConversationOperationReconcileRequest {
            operation_id: foreground.clone(),
        });
        if repair_first {
            let mut commit = host
                .inner
                .store
                .lock()
                .await
                .pause_next_write_before_commit_for_test();
            let finished = release_late_cancel(&rig, &host);
            commit.reached().await;
            assert!(futures_util::poll!(&mut reconcile).is_pending());
            commit.release();
            let observed = reconcile.await.expect("reconcile reads the repaired row");
            assert_eq!(observed.effect, ProviderOperationEffect::Applied);
            assert_eq!(
                observed.reconciliation,
                ProviderReconciliationState::Confirmed
            );
            finished.await.expect("repair finished");
        } else {
            let observed = reconcile.await.expect("reconcile takes the guard first");
            assert_eq!(observed.effect, ProviderOperationEffect::Unknown);
            assert_eq!(
                observed.reconciliation,
                ProviderReconciliationState::NotReconcilable
            );
            release_late_cancel(&rig, &host)
                .await
                .expect("later repair finished");
        }
        let row = host
            .show(ConversationOperationShowRequest {
                operation_id: foreground.clone(),
            })
            .await
            .expect("original row");
        assert_eq!(row.effect, ProviderOperationEffect::Applied);
        assert_eq!(row.reconciliation, ProviderReconciliationState::Confirmed);
        assert_eq!(
            host.wait(wait_request(foreground))
                .await
                .expect_err("original answer remains unknown")
                .effect,
            ProviderOperationEffect::Unknown
        );
        host.shutdown().await.expect("shutdown");
    }
}

fn release_late_cancel(
    rig: &RestoreRig,
    host: &ExternalProviderSupervisor,
) -> tokio::sync::oneshot::Receiver<()> {
    let finished = host
        .inner
        .uncertainty
        .slot(&rig.target.endpoint)
        .fixture
        .notice_wait(
            rig.target.clone(),
            crate::binding_store_fixture::BindingFixtureWait::LatePromptRepair,
        );
    node_test_fixture::release_fixture(&rig.root.path().join("wire.json"), ".release-cancel")
        .expect("release the original wire request");
    finished
}

#[tokio::test]
async fn unconfirmed_cancel_same_generation_store_reopen_requires_reconcile() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig
        .open_host_with(&["silent-prompts", "advertise-close"])
        .await;
    rig.assert_loads(&host).await;
    let (foreground, _) = submit_and_cancel(&rig, &host).await;
    expire_confirmation(&rig, &host, &foreground).await;

    let closing = OperationId::generate();
    host.close(ConversationCloseRequest {
        operation_id: closing.clone(),
        target: rig.target.clone(),
        generation: Some(generation()),
        requested_by: rig.target.clone().into(),
        approver: rig.target.clone().into(),
    })
    .await
    .expect("close can recover the live run");
    let deadline = rig.clock.monotonic_now().after(CONFIRMATION_BUDGET);
    rig.clock.not_due_check_now(deadline).await;
    rig.clock
        .advance(CONFIRMATION_BUDGET)
        .expect("close drain deadline");
    let outcome = host
        .wait(wait_request(closing))
        .await
        .expect_err("close released locally without a confirmed drain");
    assert_eq!(outcome.effect, ProviderOperationEffect::None);
    assert!(String::from(outcome.message).contains("the Router released the session locally"));
    assert_eq!(rig.wire_receipt()["closes"], serde_json::json!([]));
    host.shutdown().await.expect("first shutdown");

    let backend = rig.open_host("plain-prompts").await;
    let resumed = OperationId::generate();
    let refused = backend
        .prompt(rig.prompt_request(resumed.clone(), "after restart"))
        .await
        .expect_err("the durable unknown still blocks");
    assert_eq!(
        String::from(refused.message),
        format!(
            "an earlier prompt's outcome is unknown; check the session, then run operation reconcile for {}",
            foreground.as_str()
        )
    );
    let reconciled = backend
        .reconcile(ConversationOperationReconcileRequest {
            operation_id: foreground.clone(),
        })
        .await
        .expect("deliberate reconciliation");
    assert_eq!(reconciled.effect, ProviderOperationEffect::Unknown);
    assert_eq!(
        reconciled.reconciliation,
        collaboration_protocol::ProviderReconciliationState::NotReconcilable
    );
    rig.assert_loads(&backend).await;
    backend
        .prompt(rig.prompt_request(resumed.clone(), "after restart"))
        .await
        .expect("reconciled prompt no longer blocks admission");
    assert!(matches!(
        backend
            .wait(wait_request(resumed))
            .await
            .expect("successor prompt settles")
            .output,
        ConversationOperationWaitOutput::Available {
            settlement: ConversationOperationSettlement::PromptCompleted { .. }
        }
    ));
    backend.shutdown().await.expect("second shutdown");

    let reopened = rig.open_host("plain-prompts").await;
    rig.assert_loads(&reopened).await;
    let reopened_prompt = OperationId::generate();
    reopened
        .prompt(rig.prompt_request(reopened_prompt.clone(), "after reconciliation reopened"))
        .await
        .expect("acknowledged row is absent from the reopened permit");
    assert!(matches!(
        reopened
            .wait(wait_request(reopened_prompt))
            .await
            .expect("later prompt settles")
            .output,
        ConversationOperationWaitOutput::Available {
            settlement: ConversationOperationSettlement::PromptCompleted { .. }
        }
    ));
    reopened.shutdown().await.expect("final shutdown");
}
