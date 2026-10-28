//! The actual session Query resolves another canonical control, never the proposed refusal.
use super::*;
use crate::frozen_mail_query_fixture::FrozenMailQuery;
use crate::mail_delivery_runtime::{FrozenCancelSettlement, RemoteCancelReplay};
use board_sync_protocol::BoardDeliveryTypes;
use fleet_sync_protocol::{
    CancelResult, DeliveryEnvelope, DeliveryStatus, ItemStatus, ItemStatusState, StatusVersion,
};

fn envelopes(
    original: &DeliveryItem<collaboration_contracts::RouterDeliveryTypes>,
    proposed: ItemId,
) -> (
    DeliveryEnvelope<BoardDeliveryTypes>,
    DeliveryEnvelope<BoardDeliveryTypes>,
) {
    (
        serde_json::from_value(
            serde_json::to_value(DeliveryEnvelope::Item(replay_item(original))).unwrap(),
        )
        .unwrap(),
        serde_json::from_value(
            serde_json::to_value(DeliveryEnvelope::Cancel(child(original, proposed))).unwrap(),
        )
        .unwrap(),
    )
}

fn answer(
    reference: &DeliveryItemRef,
    status: DeliveryStatus<collaboration_mail_protocol::CarrierAcceptance>,
    cancel: Option<CancelResult>,
) -> ItemStatus<collaboration_mail_protocol::CarrierAcceptance> {
    ItemStatus {
        item: reference.item,
        state: ItemStatusState::Known {
            digest: reference.digest.clone(),
            status,
            version: StatusVersion::new(2),
            cancel,
        },
    }
}

async fn resolve(
    mail: &MailDeliveryRuntime,
    target: &DeliveryEnvelope<BoardDeliveryTypes>,
    control: &DeliveryEnvelope<BoardDeliveryTypes>,
) -> FrozenCancelSettlement {
    tokio::time::timeout(
        std::time::Duration::from_secs(6),
        mail.replay_frozen_cancel_and_observe(
            target,
            control,
            RemoteCancelReplay::Eligible {
                epoch: 5.try_into().unwrap(),
            },
        ),
    )
    .await
    .unwrap()
    .unwrap()
}

#[tokio::test]
async fn a_remote_pre_custody_duplicate_queries_the_canonical_control_and_target_after_reopen() {
    // Arrange real SDK admission; only the remote mailbox's typed answers stand in.
    let runtime = Runtime::new().await;
    let directory = tempfile::tempdir().unwrap();
    let peer = runtime.fixture.host.config.local.owner;
    let mut query = FrozenMailQuery::open(directory.path(), peer).await;
    query.release();
    let mail = MailDeliveryRuntime::new(
        peer,
        runtime.local_mail_props(),
        Some(query.session.clone()),
    )
    .unwrap();
    let original = project_item(&runtime, ProjectId::generate());
    let first = child(&original, ItemId::new(uuid::Uuid::now_v7()).unwrap());
    let canonical = DeliveryItemRef {
        item: first.id(),
        digest: first.digest().clone(),
    };
    let target_ref = DeliveryItemRef {
        item: original.id(),
        digest: original.digest().clone(),
    };
    mail.enqueue_project_cancel(first, 2.try_into().unwrap())
        .await
        .unwrap();
    let proposed = ItemId::new(uuid::Uuid::now_v7()).unwrap();
    let (target, control) = envelopes(&original, proposed);
    query.answer_for(
        canonical.item,
        answer(
            &canonical,
            DeliveryStatus::Held,
            Some(CancelResult::Prevented),
        ),
    );
    query.answer_for(
        target_ref.item,
        answer(&target_ref, DeliveryStatus::Cancelled, None),
    );

    // Act: a non-terminal canonical control cannot be bypassed by its cancel result.
    let pending_control = resolve(&mail, &target, &control).await;
    let first_query = query.entered.recv().await.unwrap();
    drop(mail);
    let runtime = runtime.reopen().await;
    let mail = MailDeliveryRuntime::new(
        peer,
        runtime.local_mail_props(),
        Some(query.session.clone()),
    )
    .unwrap();
    query.answer_for(
        canonical.item,
        answer(
            &canonical,
            DeliveryStatus::Cancelled,
            Some(CancelResult::Prevented),
        ),
    );
    query.answer_for(
        target_ref.item,
        answer(&target_ref, DeliveryStatus::Held, None),
    );
    let pending_target = resolve(&mail, &target, &control).await;
    query.answer_for(
        target_ref.item,
        answer(&target_ref, DeliveryStatus::Cancelled, None),
    );
    let settled = resolve(&mail, &target, &control).await;

    // The public Query identity and digest are checked even for terminal answers.
    let mut wrong_id = answer(
        &canonical,
        DeliveryStatus::Cancelled,
        Some(CancelResult::Prevented),
    );
    wrong_id.item = proposed;
    query.answer_for(canonical.item, wrong_id);
    let id_result = mail
        .replay_frozen_cancel_and_observe(
            &target,
            &control,
            RemoteCancelReplay::Eligible {
                epoch: 5.try_into().unwrap(),
            },
        )
        .await;
    let mut wrong_digest = answer(
        &canonical,
        DeliveryStatus::Cancelled,
        Some(CancelResult::Prevented),
    );
    let ItemStatusState::Known { digest, .. } = &mut wrong_digest.state else {
        panic!("canonical control query fixture must carry a known status")
    };
    *digest = target_ref.digest;
    query.answer_for(canonical.item, wrong_digest);
    let digest_result = mail
        .replay_frozen_cancel_and_observe(
            &target,
            &control,
            RemoteCancelReplay::Eligible {
                epoch: 5.try_into().unwrap(),
            },
        )
        .await;
    let absent_epoch = runtime
        .fixture
        .owner
        .mail_submission_epoch(&mail_id(proposed))
        .await
        .unwrap()
        .is_none();
    let absent_custody = outbox_is_unknown(&runtime, proposed).await;
    let first_epoch = runtime
        .fixture
        .owner
        .mail_submission_epoch(&mail_id(canonical.item))
        .await
        .unwrap()
        .unwrap();
    drop(mail);
    runtime.close().await;
    query.close().await;

    // Assert independently named IDs, terminality and the actual no-custody boundary.
    assert_eq!(first_query, vec![canonical.item]);
    assert_eq!(pending_control, FrozenCancelSettlement::Pending);
    assert_eq!(pending_target, FrozenCancelSettlement::Pending);
    assert_eq!(settled, FrozenCancelSettlement::Prevented);
    let id_error = id_result.unwrap_err().to_string();
    let digest_error = digest_result.unwrap_err().to_string();
    assert!(id_error.contains("query identity"), "{id_error}");
    assert!(digest_error.contains("query digest"), "{digest_error}");
    assert!(absent_epoch && absent_custody);
    assert_eq!(first_epoch.epoch, 2.try_into().unwrap());
}

#[tokio::test]
async fn a_relay_refused_proposed_child_keeps_its_epoch_but_waits_for_the_other_control() {
    use durable_steps::AtomicStore as _;
    let runtime = Runtime::new().await;
    let directory = tempfile::tempdir().unwrap();
    let peer = runtime.fixture.host.config.local.owner;
    let mut query = FrozenMailQuery::open(directory.path(), peer).await;
    query.release();
    let mail = MailDeliveryRuntime::new(
        peer,
        runtime.local_mail_props(),
        Some(query.session.clone()),
    )
    .unwrap();
    let original = project_item(&runtime, ProjectId::generate());
    let proposed = ItemId::new(uuid::Uuid::now_v7()).unwrap();
    let cancel = child(&original, proposed);
    let input = fleet_sync_delivery::SubmitInput {
        item: proposed,
        digest: cancel.digest().clone(),
        relay: query.session.relay,
        mailbox: original.recipient().peer,
    };
    mail.enqueue_project_cancel(cancel, 2.try_into().unwrap())
        .await
        .unwrap();
    let first = child(&original, ItemId::new(uuid::Uuid::now_v7()).unwrap());
    let canonical = DeliveryItemRef {
        item: first.id(),
        digest: first.digest().clone(),
    };
    let recorded = canonical.clone();
    let limits = runtime.fixture.host.config.local.limits;
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
                        existing: recorded,
                        status: DeliveryStatus::Held,
                        version: StatusVersion::new(1),
                        cancel: Some(CancelResult::Pending),
                    },
                )
                .await
                .map_err(durable_steps::StoreError::backend)
            })
        })
        .await
        .unwrap();
    drop(mail);
    let runtime = runtime.reopen().await;
    let mail = MailDeliveryRuntime::new(
        peer,
        runtime.local_mail_props(),
        Some(query.session.clone()),
    )
    .unwrap();
    let (target, control) = envelopes(&original, proposed);
    query.answer_for(
        canonical.item,
        answer(
            &canonical,
            DeliveryStatus::Held,
            Some(CancelResult::Pending),
        ),
    );
    let pending = resolve(&mail, &target, &control).await;
    let first_query = query.entered.recv().await.unwrap();
    let epoch = runtime
        .fixture
        .owner
        .mail_submission_epoch(&mail_id(proposed))
        .await
        .unwrap()
        .unwrap();
    let no_false_epoch = runtime
        .fixture
        .owner
        .mail_submission_epoch(&mail_id(canonical.item))
        .await
        .unwrap()
        .is_none();
    let proposed_retained = !outbox_is_unknown(&runtime, proposed).await;
    drop(mail);
    runtime.close().await;
    query.close().await;
    assert_eq!(pending, FrozenCancelSettlement::Pending);
    assert_eq!(first_query, vec![canonical.item]);
    assert_eq!(epoch.epoch, 2.try_into().unwrap());
    assert!(no_false_epoch && proposed_retained);
}

#[tokio::test]
async fn remote_retirement_preserves_an_earlier_control_and_retires_only_its_own_identities() {
    // Arrange public SDK custody, then physically reopen the native owner.
    let runtime = Runtime::new().await;
    let directory = tempfile::tempdir().unwrap();
    let peer = runtime.fixture.host.config.local.owner;
    let query = FrozenMailQuery::open(directory.path(), peer).await;
    query.release();
    let mail = MailDeliveryRuntime::new(
        peer,
        runtime.local_mail_props(),
        Some(query.session.clone()),
    )
    .unwrap();
    let original = project_item(&runtime, ProjectId::generate());
    mail.enqueue_project_item(replay_item(&original), 2.try_into().unwrap())
        .await
        .unwrap();
    let first = child(&original, ItemId::new(uuid::Uuid::now_v7()).unwrap());
    let canonical = DeliveryItemRef {
        item: first.id(),
        digest: first.digest().clone(),
    };
    let original_ref = DeliveryItemRef {
        item: original.id(),
        digest: original.digest().clone(),
    };
    mail.enqueue_project_cancel(first, 2.try_into().unwrap())
        .await
        .unwrap();
    let proposed = ItemId::new(uuid::Uuid::now_v7()).unwrap();
    let (target, other_control) = envelopes(&original, proposed);
    let (_, own_control) = envelopes(&original, canonical.item);
    drop(mail);
    let runtime = runtime.reopen().await;
    let mail = MailDeliveryRuntime::new(
        peer,
        runtime.local_mail_props(),
        Some(query.session.clone()),
    )
    .unwrap();
    query.answer_for(
        canonical.item,
        answer(
            &canonical,
            DeliveryStatus::Cancelled,
            Some(CancelResult::Prevented),
        ),
    );
    query.answer_for(
        original_ref.item,
        answer(&original_ref, DeliveryStatus::Cancelled, None),
    );

    // Act: another group's canonical identity never becomes this group's retirement.
    let earlier_done = mail
        .retire_frozen_control(&other_control, 5.try_into().unwrap())
        .await
        .unwrap();
    let earlier_requests = query.retirement_requests();
    query.answer_for(
        canonical.item,
        answer(
            &canonical,
            DeliveryStatus::Held,
            Some(CancelResult::Pending),
        ),
    );
    let own_pending = mail
        .retire_frozen_control(&own_control, 5.try_into().unwrap())
        .await
        .unwrap();
    let pending_requests = query.retirement_requests();
    query.answer_for(
        canonical.item,
        answer(
            &canonical,
            DeliveryStatus::Cancelled,
            Some(CancelResult::Prevented),
        ),
    );
    let own_done = mail
        .retire_frozen_control(&own_control, 5.try_into().unwrap())
        .await
        .unwrap();
    let original_done = mail.retire_frozen_original(&target).await.unwrap();
    let requests = query.retirement_requests();
    let no_proposed_custody = outbox_is_unknown(&runtime, proposed).await;
    let no_proposed_epoch = runtime
        .fixture
        .owner
        .mail_submission_epoch(&mail_id(proposed))
        .await
        .unwrap()
        .is_none();
    drop(mail);
    runtime.close().await;
    query.close().await;

    // Assert at the actual wire boundary; the remote deletion itself is scripted.
    assert!(
        earlier_done && earlier_requests.is_empty(),
        "another group's canonical control was retired: {earlier_requests:?}"
    );
    assert!(
        !own_pending && pending_requests.is_empty(),
        "a non-terminal own control was retired: {pending_requests:?}"
    );
    assert!(own_done && original_done);
    let mailbox = original.recipient().peer;
    assert_eq!(
        requests,
        vec![
            (mailbox, vec![canonical.item]),
            (mailbox, vec![original_ref.item]),
        ]
    );
    assert!(no_proposed_custody && no_proposed_epoch);
}
