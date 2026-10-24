//! The actual Local producer owns replay, refusal rollback and overlapping send admission.
use super::*;
use crate::push_record_delivery::PushDeliveryFailure;
use collaboration_mail_protocol::MailRefusal;
use durable_steps::{AtomicStore, StoreError};
use fleet_sync_delivery::{DeliveryError, DeliveryLimits, Inbox, minimum_settlement_allowance};
use fleet_sync_protocol::{DeliveryEnvelope, DeliveryItemRef};
#[path = "local_mail_interruption_tests.rs"]
mod interruption;

#[tokio::test]
async fn settled_local_answer_replays_after_pruning_without_resurrecting_a_push_or_call() {
    let runtime = Runtime::new().await;
    let mail = runtime.local_mail_runtime();
    let draft = runtime.fixture.draft();
    let id = runtime.fixture.item(&draft).id();
    let push = draft.push_id.clone();
    runtime.client.script(id, vec![accepted()]);
    let result = mail
        .send_local(draft.clone(), "first target label".into())
        .await
        .unwrap();
    let before = runtime
        .fixture
        .owner
        .message_send_answer(&mail_id(id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(runtime.client.calls(id), 1);
    assert_eq!(
        runtime
            .fixture
            .owner
            .prune_push_records(recorded_at() + chrono::Duration::days(31), 10)
            .await
            .unwrap(),
        1
    );
    drop(mail);
    let runtime = runtime.reopen().await;
    let mail = runtime.local_mail_runtime();
    assert!(
        runtime
            .fixture
            .owner
            .get_push_record(&push)
            .await
            .unwrap()
            .is_none()
    );
    let replay = mail
        .send_local(draft.clone(), "later target label".into())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(replay).unwrap(),
        serde_json::to_value(&result).unwrap()
    );
    assert!(
        runtime
            .fixture
            .owner
            .get_push_record(&push)
            .await
            .unwrap()
            .is_none()
    );
    let after = runtime
        .fixture
        .owner
        .message_send_answer(&mail_id(id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.recorded_at, before.recorded_at);
    assert_eq!(after.target_identity, "first target label");
    for operand in ["body", "target", "sender", "mode", "guard"] {
        let mut conflict = draft.clone();
        match operand {
            "body" => conflict.body = Some("changed content under compacted identity".into()),
            "target" => {
                conflict.target.session_id = "changed-target".to_owned().try_into().unwrap()
            }
            "sender" => {
                conflict.origin =
                    collaboration_protocol::PushOrigin::Session(conflict.target.clone())
            }
            "mode" => conflict.mode = Some(collaboration_protocol::MessageDelivery::Queue),
            "guard" => {
                conflict.guard = Some(
                    serde_json::from_value(serde_json::json!({
                        "serviceEpoch":conflict.target.endpoint.service_id,"generation":1
                    }))
                    .unwrap(),
                )
            }
            _ => panic!("unknown fixture operand"),
        }
        assert!(
            matches!(
                mail.send_local(conflict, "conflicting target label".into())
                    .await,
                Err(PushDeliveryFailure::MailRefused(
                    MailRefusal::ItemConflict { .. }
                ))
            ),
            "changed {operand} did not conflict before an effect"
        );
        assert_eq!(
            runtime.client.calls(id),
            1,
            "changed {operand} reached the native client"
        );
    }
    assert_eq!(runtime.client.calls(id), 1);
    let mut fresh = draft;
    fresh.push_id = MailItemId::generate().try_into().unwrap();
    let fresh_id = runtime.fixture.item(&fresh).id();
    runtime.client.script(fresh_id, vec![accepted()]);
    assert_eq!(
        mail.send_local(fresh, "fresh target".into())
            .await
            .unwrap()
            .receipt
            .outcome,
        DeliveryOutcome::Started
    );
    assert_eq!(runtime.client.calls(fresh_id), 1);
    assert_eq!(
        serde_json::to_value(
            runtime
                .fixture
                .owner
                .message_send_answer(&mail_id(id))
                .await
                .unwrap()
                .unwrap()
                .result
        )
        .unwrap(),
        serde_json::to_value(before.result).unwrap()
    );
    drop(mail);
    runtime.close().await;
}

#[tokio::test]
async fn a_full_real_inbox_refuses_new_projections_but_reserves_space_to_cancel_its_original() {
    let runtime = Runtime::new().await;
    let first = runtime.fixture.draft();
    let first_item = runtime.fixture.item(&first);
    let first_id = first_item.id();
    let mail = runtime.local_mail_runtime();
    runtime.client.script(
        first_id,
        vec![DeliveryReceipt {
            outcome: DeliveryOutcome::NotSubmitted {
                retryable: true,
                reason: "endpoint held before accepting input".into(),
            },
            reachability: None,
            client: None,
        }],
    );
    assert_eq!(
        mail.send_local(first.clone(), "held target".into())
            .await
            .unwrap()
            .delivery_state,
        PushDeliveryState::Held
    );
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct StoredOrdinaryReserve {
        acceptance_bound: u32,
        settlement_bytes: u32,
    }
    // Project the public snapshot's acceptanceBound/settlementBytes JSON fields;
    // the SDK owns reserve calculation and private storage queries.
    let first_identity = String::from(first_id);
    let observed = runtime
        .fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                let row = fleet_sync_delivery::test_support::snapshot(scope)
                    .await
                    .map_err(StoreError::backend)?
                    .into_iter()
                    .find(|row| row.category == "inbox" && row.identity == first_identity)
                    .and_then(|row| row.content)
                    .ok_or_else(|| StoreError::backend("ordinary inbox snapshot header missing"))?;
                serde_json::from_str::<StoredOrdinaryReserve>(&row).map_err(StoreError::backend)
            })
        })
        .await
        .unwrap();
    assert_eq!(observed.acceptance_bound, 64);
    assert!(observed.settlement_bytes > 0);
    let budget = u64::try_from(
        serde_json::to_vec(&DeliveryEnvelope::Item(first_item))
            .unwrap()
            .len(),
    )
    .unwrap()
        + u64::from(observed.settlement_bytes);
    let limits = DeliveryLimits::new::<RouterDeliveryTypes>(
        16 * 1024 * 1024,
        30,
        16 * 1024 * 1024,
        30,
        budget,
        minimum_settlement_allowance::<RouterDeliveryTypes>().unwrap(),
    )
    .unwrap();
    drop(mail);
    let mail = runtime.local_mail_with_limits(limits);
    let rejected = runtime.fixture.draft();
    let rejected_item = runtime.fixture.item(&rejected);
    let rejected_id = rejected_item.id();
    let reference = DeliveryItemRef {
        item: rejected_id,
        digest: rejected_item.digest().clone(),
    };
    assert!(matches!(
        mail.send_local(rejected.clone(), "must roll back".into())
            .await,
        Err(PushDeliveryFailure::MailRefused(
            MailRefusal::MailboxFull {}
        ))
    ));
    assert!(
        runtime
            .fixture
            .owner
            .get_push_record(&rejected.push_id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        runtime
            .fixture
            .owner
            .message_send_answer(&mail_id(rejected_id))
            .await
            .unwrap()
            .is_none()
    );
    let consumer_config = runtime.fixture.host.config.local;
    let absent = runtime
        .fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                match Inbox::<_, RouterDeliveryTypes>::new(scope, consumer_config.owner, limits)
                    .resolve_delivery(&reference)
                    .await
                {
                    Err(DeliveryError::Corrupt {
                        field: "local delivery custody row missing",
                    }) => Ok(true),
                    other => Err(StoreError::backend(format!(
                        "rejected Local custody was retained: {}",
                        match other {
                            Ok(_) => "item exists".to_owned(),
                            Err(error) => error.to_string(),
                        }
                    ))),
                }
            })
        })
        .await
        .unwrap();
    assert!(absent);
    assert_eq!(runtime.client.calls(rejected_id), 0);
    let cancel = ItemId::new(uuid::Uuid::now_v7()).unwrap();
    assert_eq!(
        local_reference(
            mail.cancel_local(
                cancel_for(&runtime, &first, cancel),
                requester("reserved cancellation")
            )
            .await
            .unwrap()
        )
        .item,
        cancel
    );
    assert_eq!(
        runtime
            .fixture
            .owner
            .local_cancel_delivery(&mail_id(cancel))
            .await
            .unwrap()
            .unwrap()
            .outcome,
        Some(RecipientOutcome::Prevented { cancel })
    );
    assert_eq!(runtime.client.calls(first_id), 1);
    drop(mail);
    let runtime = runtime.reopen().await;
    let mail = runtime.local_mail_runtime();
    // Same formerly refused ID, changed body: no custody digest or journal leaked.
    let mut fresh = rejected;
    fresh.body = Some("positive control after the refused transaction".into());
    runtime.client.script(rejected_id, vec![accepted()]);
    assert_eq!(
        mail.send_local(fresh, "now has capacity".into())
            .await
            .unwrap()
            .receipt
            .outcome,
        DeliveryOutcome::Started
    );
    assert_eq!(runtime.client.calls(rejected_id), 1);
    drop(mail);
    runtime.close().await;
}

#[tokio::test]
async fn overlapping_same_id_sends_never_repeat_the_native_attempt_and_later_replay_is_stable() {
    let runtime = Runtime::new().await;
    let mail = runtime.local_mail_runtime();
    let draft = runtime.fixture.draft();
    let item = runtime.fixture.item(&draft).id();
    let pause = runtime.client.pause_before_answer(item);
    runtime.client.script(item, vec![accepted()]);
    let first_mail = mail.clone();
    let first_draft = draft.clone();
    let first = tokio::spawn(async move {
        first_mail
            .send_local(first_draft, "first label".into())
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), pause.entered)
        .await
        .unwrap()
        .unwrap();
    let pending = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        mail.send_local(draft.clone(), "second label".into()),
    )
    .await
    .unwrap();
    // The pending retained answer must be truthful; it is neither success nor a new attempt.
    assert!(
        matches!(pending, Err(PushDeliveryFailure::DeliveryUnknown(ref id)) if id == &draft.push_id)
    );
    assert_eq!(runtime.client.calls(item), 1);
    pause.release.send(()).unwrap();
    let completed = first.await.unwrap().unwrap();
    assert_eq!(completed.receipt.outcome, DeliveryOutcome::Started);
    let replay = mail.send_local(draft, "third label".into()).await.unwrap();
    assert_eq!(
        serde_json::to_value(replay).unwrap(),
        serde_json::to_value(completed).unwrap()
    );
    assert_eq!(runtime.client.calls(item), 1);
    drop(mail);
    runtime.close().await;
}
