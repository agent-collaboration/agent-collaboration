//! Actual Local intake, SDK and native owner; only the endpoint answer and clock stand in.
use super::*;
use crate::{LocalCancelReference, MailDeliveryRuntime};
use collaboration_mail_protocol::{CancelCause, MailItemId};
use collaboration_protocol::{MessageSendResult, PushRecordDraft};
use fleet_sync_delivery::DeliveryTransactionFailure;
use fleet_sync_protocol::{CancelBody, DeliveryCancel, DeliveryRefusal, ItemId, TooLateAt};
#[path = "local_mail_admission_tests.rs"]
mod admission;
#[path = "local_cancel_crash_tests.rs"]
mod crash;
#[path = "frozen_cancel_settlement_tests.rs"]
mod frozen_settlement;

#[path = "local_mail_readiness_tests.rs"]
mod readiness;

#[path = "local_cancel_report_tests.rs"]
mod report_admission;

fn mail_id(item: ItemId) -> MailItemId {
    item.as_uuid().to_string().try_into().unwrap()
}
fn cancel_for(
    runtime: &Runtime,
    draft: &PushRecordDraft,
    id: ItemId,
) -> DeliveryCancel<RouterDeliveryTypes> {
    let original = runtime.fixture.item(draft);
    DeliveryCancel::new(
        id,
        original.sender().clone(),
        original.recipient().clone(),
        None,
        CancelBody {
            target: original.id(),
        },
    )
    .unwrap()
}
fn local_reference(result: LocalCancelReference) -> fleet_sync_protocol::DeliveryItemRef {
    match result {
        LocalCancelReference::Local { reference } => reference,
        other @ LocalCancelReference::ExistingElsewhere { .. } => {
            panic!("Local attribution lost: {other:?}")
        }
    }
}

#[tokio::test]
async fn local_intake_prepares_attribution_before_the_original_is_prevented_without_native_io() {
    let runtime = Runtime::new().await;
    let mail = runtime.local_mail_runtime();
    let draft = runtime.fixture.draft();
    let original = runtime.fixture.item(&draft).id();
    let cancel = ItemId::new(uuid::Uuid::now_v7()).unwrap();
    assert_eq!(
        local_reference(
            mail.cancel_local(
                cancel_for(&runtime, &draft, cancel),
                CancelCause::BoardDerived {}
            )
            .await
            .unwrap()
        )
        .item,
        cancel
    );
    let pending = runtime
        .fixture
        .owner
        .local_cancel_delivery(&mail_id(cancel))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.attribution, CancelCause::BoardDerived {});
    assert!(pending.outcome.is_none());
    let result = mail
        .send_local(draft, "frozen cancelled target".into())
        .await
        .unwrap_or_else(|_| panic!("prevented admission did not return its Local answer"));
    assert_eq!(result.delivery_state, PushDeliveryState::Rejected);
    assert!(matches!(
        result.receipt.outcome,
        DeliveryOutcome::NotSubmitted {
            retryable: false,
            ..
        }
    ));
    assert_eq!(runtime.client.calls(original), 0);
    drop(mail);
    let runtime = runtime.reopen().await;
    let stored = runtime
        .fixture
        .owner
        .local_cancel_delivery(&mail_id(cancel))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.attribution, pending.attribution);
    assert_eq!(stored.recorded_at, pending.recorded_at);
    assert_eq!(stored.outcome, Some(RecipientOutcome::Prevented { cancel }));
    assert_eq!(runtime.client.calls(original), 0);
    runtime.close().await;
}

#[tokio::test]
async fn canonical_cancel_keeps_the_first_attribution_and_removes_only_the_new_tentative_row() {
    let runtime = Runtime::new().await;
    let mail = runtime.local_mail_runtime();
    let draft = runtime.fixture.draft();
    let first = ItemId::new(uuid::Uuid::now_v7()).unwrap();
    let second = ItemId::new(uuid::Uuid::now_v7()).unwrap();
    let reference = local_reference(
        mail.cancel_local(
            cancel_for(&runtime, &draft, first),
            CancelCause::BoardDerived {},
        )
        .await
        .unwrap(),
    );
    let before = runtime
        .fixture
        .owner
        .local_cancel_delivery(&mail_id(first))
        .await
        .unwrap()
        .unwrap();
    runtime.set_time(60_000);
    for proposed in [first, second] {
        let repeated = local_reference(
            mail.cancel_local(
                cancel_for(&runtime, &draft, proposed),
                CancelCause::BoardDerived {},
            )
            .await
            .unwrap(),
        );
        assert_eq!(repeated, reference);
    }
    // Same-ID different content must still be the SDK's typed conflict.
    let other = runtime.fixture.draft();
    assert!(matches!(
        mail.cancel_local(
            cancel_for(&runtime, &other, first),
            CancelCause::BoardDerived {}
        )
        .await,
        Err(DeliveryTransactionFailure::Refused(
            DeliveryRefusal::ItemConflict { .. }
        ))
    ));
    assert!(
        runtime
            .fixture
            .owner
            .local_cancel_delivery(&mail_id(second))
            .await
            .unwrap()
            .is_none()
    );
    drop(mail);
    let runtime = runtime.reopen().await;
    let after = runtime
        .fixture
        .owner
        .local_cancel_delivery(&mail_id(first))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.attribution, CancelCause::BoardDerived {});
    assert_eq!(after.recorded_at, before.recorded_at);
    assert_eq!(after.target_item, before.target_item);
    assert!(after.outcome.is_none());
    assert_eq!(
        runtime
            .sql_count("SELECT COUNT(*) AS count FROM local_cancel_deliveries")
            .await,
        1
    );
    runtime.close().await;
}

#[tokio::test]
async fn remote_canonical_cancel_returns_unknown_local_attribution_without_inventing_a_row() {
    let runtime = Runtime::new().await;
    let draft = runtime.fixture.draft();
    // Actual remote Inbox control intake, with no original and no Local projection.
    let remote = runtime.fixture.receive_cancel(&draft).await;
    let proposed = ItemId::new(uuid::Uuid::now_v7()).unwrap();
    let mail = runtime.local_mail_runtime();
    let result = mail
        .cancel_local(
            cancel_for(&runtime, &draft, proposed),
            CancelCause::BoardDerived {},
        )
        .await
        .unwrap();
    assert!(
        matches!(result, LocalCancelReference::ExistingElsewhere { reference } if reference.item == remote)
    );
    assert!(
        runtime
            .fixture
            .owner
            .local_cancel_delivery(&mail_id(remote))
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        runtime
            .fixture
            .owner
            .local_cancel_delivery(&mail_id(proposed))
            .await
            .unwrap()
            .is_none()
    );
    drop(mail);
    let runtime = runtime.reopen().await;
    assert_eq!(
        runtime
            .sql_count("SELECT COUNT(*) AS count FROM local_cancel_deliveries")
            .await,
        0
    );
    runtime.close().await;
}

#[tokio::test]
async fn late_local_intake_records_the_control_outcome_and_only_marks_the_delivered_target() {
    for (receipt, late) in [
        (accepted(), TooLateAt::Delivered),
        (
            DeliveryReceipt {
                outcome: DeliveryOutcome::Unknown,
                reachability: None,
                client: None,
            },
            TooLateAt::Uncertain,
        ),
    ] {
        let runtime = Runtime::new().await;
        let mail = runtime.local_mail_runtime();
        let draft = runtime.fixture.draft();
        let original = runtime.fixture.item(&draft).id();
        let push = draft.push_id.clone();
        runtime.client.script(original, vec![receipt]);
        let sent = mail
            .send_local(draft.clone(), "first native answer".into())
            .await
            .unwrap_or_else(|_| panic!("native send did not settle"));
        let before = runtime
            .fixture
            .owner
            .get_push_record(&push)
            .await
            .unwrap()
            .unwrap();
        let answer = serde_json::to_value(MessageSendResult::Local(sent)).unwrap();
        let cancel = ItemId::new(uuid::Uuid::now_v7()).unwrap();
        mail.cancel_local(
            cancel_for(&runtime, &draft, cancel),
            CancelCause::BoardDerived {},
        )
        .await
        .unwrap();
        assert_eq!(runtime.client.calls(original), 1);
        drop(mail);
        let runtime = runtime.reopen().await;
        let stored = runtime
            .fixture
            .owner
            .local_cancel_delivery(&mail_id(cancel))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            stored.outcome,
            Some(RecipientOutcome::TooLate { cancel, at: late })
        );
        assert_eq!(stored.attribution, CancelCause::BoardDerived {});
        let after = runtime
            .fixture
            .owner
            .get_push_record(&push)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            after.cancelled_after_delivery.as_ref().unwrap().cancel,
            mail_id(cancel)
        );
        let mut after_json = serde_json::to_value(after).unwrap();
        after_json
            .as_object_mut()
            .unwrap()
            .remove("cancelledAfterDelivery");
        assert_eq!(after_json, serde_json::to_value(before).unwrap());
        assert_eq!(
            serde_json::to_value(
                runtime
                    .fixture
                    .owner
                    .message_send_answer(&mail_id(original))
                    .await
                    .unwrap()
                    .unwrap()
                    .result
                    .unwrap()
            )
            .unwrap(),
            answer
        );
        assert_eq!(runtime.client.calls(original), 1);
        runtime.close().await;
    }
}

#[tokio::test]
async fn failed_control_projection_rolls_back_the_local_record_and_sdk_admission_together() {
    let runtime = Runtime::new().await;
    let mail: MailDeliveryRuntime = runtime.local_mail_runtime();
    let draft = runtime.fixture.draft();
    let original = runtime.fixture.item(&draft).id();
    runtime.client.script(original, vec![accepted()]);
    mail.send_local(draft.clone(), "committed target".into())
        .await
        .unwrap_or_else(|_| panic!("native send did not settle"));
    let cancel = ItemId::new(uuid::Uuid::now_v7()).unwrap();
    runtime.fixture.sql("CREATE TRIGGER reject_local_cancel_outcome BEFORE UPDATE OF outcome_json ON local_cancel_deliveries BEGIN SELECT RAISE(ABORT,'fixture Local control projection failed'); END").await;
    let error = mail
        .cancel_local(
            cancel_for(&runtime, &draft, cancel),
            CancelCause::BoardDerived {},
        )
        .await
        .unwrap_err();
    assert!(
        matches!(error, DeliveryTransactionFailure::OutcomeUnknown(ref cause) if cause.to_string().contains("fixture Local control projection failed")),
        "{error:?}"
    );
    assert!(
        runtime
            .fixture
            .owner
            .local_cancel_delivery(&mail_id(cancel))
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        runtime
            .fixture
            .owner
            .get_push_record(&draft.push_id)
            .await
            .unwrap()
            .unwrap()
            .cancelled_after_delivery
            .is_none()
    );
    runtime
        .fixture
        .sql("DROP TRIGGER reject_local_cancel_outcome")
        .await;
    drop(mail);
    let runtime = runtime.reopen().await;
    let mail = runtime.local_mail_runtime();
    // Same control can now settle: no committed SDK identity survived the failed transaction.
    assert_eq!(
        local_reference(
            mail.cancel_local(
                cancel_for(&runtime, &draft, cancel),
                CancelCause::BoardDerived {}
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
            .attribution,
        CancelCause::BoardDerived {}
    );
    assert_eq!(runtime.client.calls(original), 1);
    drop(mail);
    runtime.close().await;
}

#[tokio::test]
async fn failed_prevented_projection_rolls_back_the_original_but_keeps_the_pending_cancel() {
    let runtime = Runtime::new().await;
    let mail = runtime.local_mail_runtime();
    let draft = runtime.fixture.draft();
    let id = runtime.fixture.item(&draft).id();
    let cancel = ItemId::new(uuid::Uuid::now_v7()).unwrap();
    mail.cancel_local(
        cancel_for(&runtime, &draft, cancel),
        CancelCause::BoardDerived {},
    )
    .await
    .unwrap();
    let before = runtime
        .fixture
        .owner
        .local_cancel_delivery(&mail_id(cancel))
        .await
        .unwrap()
        .unwrap();
    runtime.fixture.sql("CREATE TRIGGER reject_prevented_original BEFORE UPDATE OF last_outcome_json ON router_pushes BEGIN SELECT RAISE(ABORT,'fixture prevented original outcome failed'); END").await;
    assert!(matches!(
        mail.send_local(draft.clone(), "failed original".into())
            .await,
        Err(crate::push_record_delivery::PushDeliveryFailure::StoreFailed)
    ));
    assert!(
        runtime
            .fixture
            .owner
            .get_push_record(&draft.push_id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        runtime
            .fixture
            .owner
            .message_send_answer(&mail_id(id))
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        runtime
            .fixture
            .owner
            .local_cancel_delivery(&mail_id(cancel))
            .await
            .unwrap()
            .unwrap()
            .outcome
            .is_none()
    );
    assert_eq!(runtime.client.calls(id), 0);
    runtime
        .fixture
        .sql("DROP TRIGGER reject_prevented_original")
        .await;
    drop(mail);
    let runtime = runtime.reopen().await;
    let mail = runtime.local_mail_runtime();
    let sent = mail
        .send_local(draft, "committed original".into())
        .await
        .unwrap();
    assert_eq!(sent.delivery_state, PushDeliveryState::Rejected);
    assert!(matches!(
        sent.receipt.outcome,
        DeliveryOutcome::NotSubmitted {
            retryable: false,
            ..
        }
    ));
    let after = runtime
        .fixture
        .owner
        .local_cancel_delivery(&mail_id(cancel))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.outcome, Some(RecipientOutcome::Prevented { cancel }));
    assert_eq!(after.attribution, before.attribution);
    assert_eq!(after.recorded_at, before.recorded_at);
    assert_eq!(runtime.client.calls(id), 0);
    drop(mail);
    runtime.close().await;
}
