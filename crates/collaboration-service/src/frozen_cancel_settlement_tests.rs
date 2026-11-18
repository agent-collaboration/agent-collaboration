//! Frozen children must settle through the SDK's canonical control after reopening.
use super::*;
use crate::mail_delivery_runtime::{FrozenCancelSettlement, RemoteCancelReplay};
use board_sync_protocol::BoardDeliveryTypes;
use fleet_sync_protocol::DeliveryEnvelope;

#[tokio::test]
async fn a_frozen_local_child_rederives_the_earlier_control_after_reopen_without_false_custody() {
    // Arrange a requested cancel before its original: the canonical control is pending.
    let runtime = Runtime::new().await;
    let draft = runtime.fixture.draft();
    let first = ItemId::new(uuid::Uuid::now_v7()).unwrap();
    let proposed = ItemId::new(uuid::Uuid::now_v7()).unwrap();
    let original: DeliveryEnvelope<BoardDeliveryTypes> = serde_json::from_value(
        serde_json::to_value(DeliveryEnvelope::Item(runtime.fixture.item(&draft))).unwrap(),
    )
    .unwrap();
    let child: DeliveryEnvelope<BoardDeliveryTypes> = serde_json::from_value(
        serde_json::to_value(DeliveryEnvelope::Cancel(cancel_for(
            &runtime, &draft, proposed,
        )))
        .unwrap(),
    )
    .unwrap();
    let mail = runtime.local_mail_runtime();
    let canonical = local_reference(
        mail.cancel_local(
            cancel_for(&runtime, &draft, first),
            requester("earlier request"),
        )
        .await
        .unwrap(),
    );
    assert_eq!(
        mail.replay_frozen_cancel_and_observe(&original, &child, RemoteCancelReplay::QueryOnly)
            .await
            .unwrap(),
        FrozenCancelSettlement::Pending
    );
    drop(mail);
    let runtime = runtime.reopen().await;
    let mail = runtime.local_mail_runtime();

    // Act: the original arrives after the real owner reopen, then replay the frozen child.
    let answer = mail
        .send_local(draft, "target of the earlier requested cancel".into())
        .await
        .unwrap();
    let prevented = mail
        .replay_frozen_cancel_and_observe(&original, &child, RemoteCancelReplay::QueryOnly)
        .await
        .unwrap();
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
    let calls = runtime.client.calls(original.id());
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
    assert_eq!(
        prevented,
        FrozenCancelSettlement::Prevented,
        "the frozen child ignored its earlier canonical control after reopen"
    );
}
