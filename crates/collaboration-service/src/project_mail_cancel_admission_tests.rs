//! Another canonical control never supplies the proposed child's SDK custody or epoch.
use super::*;
use fleet_sync_protocol::{CancelBody, DeliveryCancel, DeliveryItemRef};
#[path = "frozen_remote_cancel_tests.rs"]
mod frozen_remote;

fn child(
    item: &DeliveryItem<collaboration_contracts::RouterDeliveryTypes>,
    id: ItemId,
) -> DeliveryCancel<collaboration_contracts::RouterDeliveryTypes> {
    DeliveryCancel::new(
        id,
        item.sender().clone(),
        item.recipient().clone(),
        None,
        CancelBody { target: item.id() },
    )
    .unwrap()
}

#[tokio::test]
async fn a_relay_cancel_refusal_remains_another_canonical_control_on_same_id_replay() {
    let runtime = Runtime::new().await;
    let directory = tempfile::tempdir().unwrap();
    let peer = runtime.fixture.host.config.local.owner;
    let session = unwelcomed_session(directory.path(), peer).await;
    let relay = session.relay;
    let mail = MailDeliveryRuntime::new(peer, runtime.local_mail_props(), Some(session)).unwrap();
    let original = project_item(&runtime, ProjectId::generate());
    let id = ItemId::new(uuid::Uuid::now_v7()).unwrap();
    mail.enqueue_project_cancel(child(&original, id), 2.try_into().unwrap())
        .await
        .unwrap();
    let epoch = runtime
        .fixture
        .owner
        .mail_submission_epoch(&mail_id(id))
        .await
        .unwrap()
        .unwrap();
    let earlier_cancel = child(&original, ItemId::new(uuid::Uuid::now_v7()).unwrap());
    let earlier = DeliveryItemRef {
        item: earlier_cancel.id(),
        digest: earlier_cancel.digest().clone(),
    };
    let answer_ref = earlier.clone();
    let input = fleet_sync_delivery::SubmitInput {
        item: id,
        digest: child(&original, id).digest().clone(),
        relay,
        mailbox: original.recipient().peer,
    };
    let limits = runtime.fixture.host.config.local.limits;
    // A typed SubmitResult stands in for relay policy; the public SDK transition,
    // replay classification and physical owner remain real. No canonical custody is claimed.
    use durable_steps::AtomicStore as _;
    runtime
        .fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                fleet_sync_delivery::Outbox::<_, collaboration_contracts::RouterDeliveryTypes>::new(
                    scope, limits,
                )
                .mark_submitted(
                    &input,
                    fleet_sync_protocol::SubmitResult::CancelAlreadyRequested {
                        existing: answer_ref,
                        status: fleet_sync_protocol::DeliveryStatus::Held,
                        version: fleet_sync_protocol::StatusVersion::new(1),
                        cancel: Some(fleet_sync_protocol::CancelResult::Pending),
                    },
                )
                .await
                .map_err(durable_steps::StoreError::backend)
            })
        })
        .await
        .unwrap();
    let replay = mail
        .enqueue_project_cancel(child(&original, id), 5.try_into().unwrap())
        .await
        .unwrap();
    assert_eq!(
        crate::mail_delivery_runtime::other_canonical_cancel(&replay),
        Some(&earlier),
        "relay refusal was treated as the proposed child's successful replay"
    );
    assert!(matches!(replay, OutboxAdmission::Replayed {
        status: fleet_sync_protocol::DeliveryStatus::Refused {
            reason: fleet_sync_protocol::DeliveryRefusal::CancelAlreadyRequested { existing },
        }, ..
    } if existing == earlier));
    drop(mail);
    let runtime = runtime.reopen().await;
    assert_eq!(
        runtime
            .fixture
            .owner
            .mail_submission_epoch(&mail_id(id))
            .await
            .unwrap()
            .unwrap(),
        epoch
    );
    assert!(
        runtime
            .fixture
            .owner
            .mail_submission_epoch(&mail_id(earlier.item))
            .await
            .unwrap()
            .is_none()
    );
    runtime.close().await;
}

#[tokio::test]
async fn another_canonical_cancel_leaves_no_proposed_child_custody_or_epoch_after_reopen() {
    let runtime = Runtime::new().await;
    let directory = tempfile::tempdir().unwrap();
    let peer = runtime.fixture.host.config.local.owner;
    let session = unwelcomed_session(directory.path(), peer).await;
    let mail = MailDeliveryRuntime::new(peer, runtime.local_mail_props(), Some(session)).unwrap();
    let original = project_item(&runtime, ProjectId::generate());
    let first = ItemId::new(uuid::Uuid::now_v7()).unwrap();
    let canonical = child(&original, first);
    let reference = DeliveryItemRef {
        item: first,
        digest: canonical.digest().clone(),
    };
    assert!(
        matches!(mail.enqueue_project_cancel(canonical, 2.try_into().unwrap()).await.unwrap(),
        OutboxAdmission::Queued { reference: queued } if queued == reference)
    );
    let epoch = runtime
        .fixture
        .owner
        .mail_submission_epoch(&mail_id(first))
        .await
        .unwrap()
        .unwrap();
    runtime.set_time(60_000);
    assert!(matches!(
        mail.enqueue_project_cancel(child(&original, first), 5.try_into().unwrap())
            .await
            .unwrap(),
        OutboxAdmission::Replayed { .. }
    ));
    assert_eq!(
        runtime
            .fixture
            .owner
            .mail_submission_epoch(&mail_id(first))
            .await
            .unwrap()
            .unwrap(),
        epoch
    );
    let changed = DeliveryCancel::new(
        first,
        original.sender().clone(),
        original.recipient().clone(),
        Some(runtime_primitives::SyncTime::from_unix_millis(120_000)),
        CancelBody {
            target: original.id(),
        },
    )
    .unwrap();
    assert!(matches!(
        mail.enqueue_project_cancel(changed, 5.try_into().unwrap())
            .await,
        Err(DeliveryTransactionFailure::Refused(
            fleet_sync_protocol::DeliveryRefusal::ItemConflict { .. }
        ))
    ));
    assert_eq!(
        runtime
            .fixture
            .owner
            .mail_submission_epoch(&mail_id(first))
            .await
            .unwrap()
            .unwrap(),
        epoch
    );
    let second = ItemId::new(uuid::Uuid::now_v7()).unwrap();
    let refused_child = mail
        .enqueue_project_cancel(child(&original, second), 5.try_into().unwrap())
        .await
        .unwrap();
    assert_eq!(
        crate::mail_delivery_runtime::other_canonical_cancel(&refused_child),
        Some(&reference)
    );
    assert!(
        matches!(refused_child, OutboxAdmission::CancelAlreadyRequested { existing, .. } if existing == reference)
    );
    assert!(
        runtime
            .fixture
            .owner
            .mail_submission_epoch(&mail_id(second))
            .await
            .unwrap()
            .is_none(),
        "another canonical cancel created a submission epoch for the proposed child"
    );
    assert!(outbox_is_unknown(&runtime, second).await);
    drop(mail);
    let runtime = runtime.reopen().await;
    assert_eq!(
        runtime
            .fixture
            .owner
            .mail_submission_epoch(&mail_id(first))
            .await
            .unwrap()
            .unwrap(),
        epoch
    );
    assert!(
        runtime
            .fixture
            .owner
            .mail_submission_epoch(&mail_id(second))
            .await
            .unwrap()
            .is_none(),
        "another canonical cancel created a submission epoch for the proposed child"
    );
    assert!(outbox_is_unknown(&runtime, second).await);
    runtime.close().await;
}

#[tokio::test]
async fn unconfigured_project_cancel_returns_unavailable_without_custody_or_epoch() {
    let runtime = Runtime::new().await;
    let mail = runtime.local_mail_runtime();
    let original = project_item(&runtime, ProjectId::generate());
    let id = ItemId::new(uuid::Uuid::now_v7()).unwrap();
    assert!(matches!(
        mail.enqueue_project_cancel(child(&original, id), 2.try_into().unwrap())
            .await,
        Err(DeliveryTransactionFailure::OutcomeUnknown(_))
    ));
    drop(mail);
    let runtime = runtime.reopen().await;
    assert!(
        runtime
            .fixture
            .owner
            .mail_submission_epoch(&mail_id(id))
            .await
            .unwrap()
            .is_none()
    );
    assert!(outbox_is_unknown(&runtime, id).await);
    runtime.close().await;
}
