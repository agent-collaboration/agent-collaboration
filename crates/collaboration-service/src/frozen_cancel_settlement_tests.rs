//! Frozen children must settle through the SDK's canonical control after reopening.
use super::*;
#[tokio::test]
async fn a_frozen_local_child_rederives_the_earlier_control_after_reopen_without_false_custody() {
    // Arrange a requested cancel before its original: the canonical control is pending.
    let runtime = Runtime::new().await;
    let draft = runtime.fixture.draft();
    let first = ItemId::new(uuid::Uuid::now_v7()).unwrap();
    let proposed = ItemId::new(uuid::Uuid::now_v7()).unwrap();
    let original = runtime.fixture.item(&draft).id();
    let mail = runtime.local_mail_runtime();
    let canonical = local_reference(
        mail.cancel_local(
            cancel_for(&runtime, &draft, first),
            requester("earlier request"),
        )
        .await
        .unwrap(),
    );
    let replay = local_reference(
        mail.cancel_local(
            cancel_for(&runtime, &draft, proposed),
            requester("duplicate"),
        )
        .await
        .unwrap(),
    );
    assert_eq!(replay.item, canonical.item);
    assert_eq!(replay.digest, canonical.digest);
    assert_eq!(runtime.fixture.local_outcome(original).await, None);
    drop(mail);
    let runtime = runtime.reopen().await;
    let mail = runtime.local_mail_runtime();

    // Act: the original arrives after the real owner reopen, then replay the frozen child.
    let answer = mail
        .send_local(
            draft.clone(),
            "target of the earlier requested cancel".into(),
        )
        .await
        .unwrap();
    let replay_after_reopen = local_reference(
        mail.cancel_local(
            cancel_for(&runtime, &draft, proposed),
            requester("duplicate after reopen"),
        )
        .await
        .unwrap(),
    );
    assert_eq!(replay_after_reopen.item, canonical.item);
    assert_eq!(replay_after_reopen.digest, canonical.digest);
    assert_eq!(
        runtime.fixture.local_outcome(original).await,
        Some(RecipientOutcome::Prevented { cancel: first }),
    );
    let local_proposed = runtime
        .fixture
        .owner
        .local_cancel_delivery(&mail_id(proposed))
        .await
        .unwrap();
    let epoch = runtime
        .fixture
        .owner
        .mail_submission_epoch(&mail_id(proposed))
        .await
        .unwrap();
    let retained = runtime
        .fixture
        .owner
        .local_cancel_delivery(&mail_id(canonical.item))
        .await
        .unwrap()
        .unwrap();
    drop(mail);
    let runtime = runtime.reopen().await;
    let still_absent = runtime
        .fixture
        .owner
        .local_cancel_delivery(&mail_id(proposed))
        .await
        .unwrap()
        .is_none();
    let calls = runtime.client.calls(original);
    runtime.close().await;

    // Assert the canonical consequence, while the proposed ID never acquires its custody.
    assert_eq!(answer.delivery_state, PushDeliveryState::Rejected);
    assert_eq!(
        retained.outcome,
        Some(RecipientOutcome::Prevented { cancel: first })
    );
    assert_eq!(retained.attribution, requester("earlier request"));
    assert!(local_proposed.is_none() && epoch.is_none() && still_absent);
    assert_eq!(calls, 0);
}
