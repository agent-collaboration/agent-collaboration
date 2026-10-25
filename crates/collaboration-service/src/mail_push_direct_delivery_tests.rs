//! Real actors, SDK tasks and projections; endpoints, time and release/report inputs stand in.
use super::*;
use crate::{NativeDeliveryResult, NativeRefusalCode, UnsentCause};
use collaboration_protocol::{
    DeliveryNextAction, DeliveryRejection, DeliveryRejectionReason, MessageDelivery,
};
use fleet_sync_protocol::{DeliveryFailureKind, DeliveryRefusal, ItemId};
use serde_json::json;

struct Wakeable;
impl TargetPresenceProbe for Wakeable {
    fn presence(
        &self,
        _: &collaboration_protocol::SessionRef,
    ) -> crate::DeliveryFuture<'_, TargetPresence> {
        Box::pin(async { Ok(TargetPresence::Wakeable) })
    }
}

fn rejected_as(reason: DeliveryRejectionReason) -> NativeDeliveryResult {
    NativeDeliveryResult::rejected(
        DeliveryRejection {
            native_failure: None,
            reason,
            next_action: DeliveryNextAction::InspectTarget,
            client_code: Some(47),
            detail: Some("entire native refusal detail survives".to_owned()),
            claims: None,
        },
        UnsentCause::for_rejection(reason),
    )
}

async fn read_native_push_record(
    runtime: &Runtime,
    id: ItemId,
) -> collaboration_protocol::PushRecord {
    runtime
        .fixture
        .owner
        .get_push_record(&id.as_uuid().to_string().try_into().unwrap())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn direct_rejection_table_reaches_real_sdk_custody_and_native_projection_after_reopen() {
    use DeliveryRejectionReason as Reason;
    for (reason, expected) in [
        (Reason::ChildThread, Some("childThread")),
        (Reason::Busy, Some("busy")),
        (Reason::HeldByAnotherClient, Some("heldByAnotherClient")),
        (Reason::SettingsUnresolved, Some("settingsUnresolved")),
        (Reason::NotResumable, Some("notResumable")),
        (Reason::PermissionDenied, Some("permissionDenied")),
        (Reason::UnsupportedCapability, Some("unsupportedCapability")),
        (Reason::Unknown, Some("unknown")),
        (Reason::EndpointUnavailable, Some("endpointUnavailable")),
        (Reason::NoRoute, None),
        (Reason::LiveElsewhere, Some("liveElsewhere")),
        (Reason::SteerUnsupported, Some("steerUnsupported")),
        (Reason::QueueUnsupported, Some("queueUnsupported")),
        (Reason::StaleGeneration, Some("staleGeneration")),
        (Reason::ProviderSessionNotFound, None),
        (Reason::ProviderRejected, Some("providerRejected")),
    ] {
        // Arrange: hard native request; each actual call returns one typed endpoint answer.
        let runtime = Runtime::new().await;
        let mut draft = runtime.fixture.draft();
        draft.mode = Some(MessageDelivery::Steer);
        let item = runtime.fixture.item(&draft);
        let id = item.id();
        let delivery = rejected_as(reason);
        let complete_receipt = serde_json::to_value(delivery.receipt()).unwrap();
        runtime.client.script_results(id, vec![delivery]);
        runtime.fixture.receive_items(vec![item]).await.unwrap();
        // Act: actual journal task -> per-reader native actor -> outcome hook, then cold store reopen.
        let engine = runtime.engine();
        engine.drive_due().await.unwrap();
        drop(engine);
        let runtime = runtime.reopen().await;
        // Assert: native unknown-reason rejection is terminal, never uncertain or a policy drop.
        let outcome = runtime.fixture.local_outcome(id).await.unwrap();
        let expected = expected.map_or_else(
            || json!({"kind":"refused","value":{"reason":{"kind":"recipientUnknown"}}}),
            |code| json!({"kind":"refused","value":{"reason":{"kind":"recipientRefused","value":{"code":code}}}}),
        );
        assert_eq!(serde_json::to_value(&outcome).unwrap(), expected);
        assert_eq!(
            serde_json::to_value(runtime.fixture.report(id).await.unwrap()).unwrap(),
            expected
        );
        let record = read_native_push_record(&runtime, id).await;
        assert_eq!(record.delivery_state, PushDeliveryState::Rejected);
        assert_eq!(
            serde_json::to_value(record.last_outcome.unwrap()).unwrap(),
            complete_receipt
        );
        assert_eq!(runtime.client.calls(id), 1);
        assert_eq!(runtime.count(id).await, 0);
        let engine = runtime.engine();
        engine.drive_due().await.unwrap();
        assert_eq!(
            runtime.client.calls(id),
            1,
            "a terminal refusal must not resend"
        );
        drop(engine);
        runtime.close().await;
    }
}

#[tokio::test]
async fn native_no_route_normalization_precedes_canonical_refusal_and_respects_the_guard() {
    for (mode, guarded, held) in [
        (MessageDelivery::Auto, false, true),
        (MessageDelivery::Queue, false, true),
        (MessageDelivery::Steer, false, false),
        (MessageDelivery::Auto, true, false),
    ] {
        // Arrange: the native actor receives the same definite NoRoute in every case.
        let runtime = Runtime::new().await;
        let mut draft = runtime.fixture.draft();
        draft.mode = Some(mode);
        if guarded {
            draft.guard = Some(
                serde_json::from_value(json!({
                    "serviceEpoch":"00000000-0000-4000-8000-000000000007","generation":7
                }))
                .unwrap(),
            );
        }
        let item = runtime.fixture.item(&draft);
        let id = item.id();
        runtime
            .client
            .script_results(id, vec![rejected_as(DeliveryRejectionReason::NoRoute)]);
        runtime.fixture.receive_items(vec![item]).await.unwrap();
        // Act.
        let engine = runtime.engine();
        engine.drive_due().await.unwrap();
        // Assert: real normalization holds only unpinned Auto/Queue; hard requests remain unknown recipients.
        let outcome = runtime.fixture.local_outcome(id).await.unwrap();
        if held {
            assert!(matches!(outcome, RecipientOutcome::Held));
            assert_eq!(
                read_native_push_record(&runtime, id).await.delivery_state,
                PushDeliveryState::Held
            );
        } else {
            assert!(matches!(
                outcome,
                RecipientOutcome::Refused {
                    reason: DeliveryRefusal::RecipientUnknown
                }
            ));
            assert_eq!(
                read_native_push_record(&runtime, id).await.delivery_state,
                PushDeliveryState::Rejected
            );
        }
        assert_eq!(runtime.client.calls(id), 1);
        drop(engine);
        runtime.close().await;
    }
}

#[tokio::test]
async fn holdable_native_answer_obeys_frozen_steer_and_guard_without_losing_its_receipt() {
    for (mode, guarded, held) in [
        (MessageDelivery::Auto, false, true),
        (MessageDelivery::Queue, false, true),
        (MessageDelivery::Steer, false, false),
        (MessageDelivery::Auto, true, false),
    ] {
        let runtime = Runtime::new().await;
        let mut draft = runtime.fixture.draft();
        draft.mode = Some(mode);
        if guarded {
            draft.guard = Some(
                serde_json::from_value(json!({
                    "serviceEpoch":"00000000-0000-4000-8000-000000000007","generation":7
                }))
                .unwrap(),
            );
        }
        let item = runtime.fixture.item(&draft);
        let id = item.id();
        let delivery = NativeDeliveryResult::not_submitted(
            "new human availability explanation",
            true,
            UnsentCause::Holdable {
                otherwise: NativeRefusalCode::NativeUnavailable,
            },
        );
        let receipt = serde_json::to_value(delivery.receipt()).unwrap();
        runtime.client.script_results(id, vec![delivery]);
        runtime.fixture.receive_items(vec![item]).await.unwrap();
        let engine = runtime.engine();
        engine.drive_due().await.unwrap();
        let outcome = runtime.fixture.local_outcome(id).await.unwrap();
        if held {
            assert!(matches!(outcome, RecipientOutcome::Held));
        } else {
            assert!(
                matches!(outcome, RecipientOutcome::Refused {reason: DeliveryRefusal::RecipientRefused {code}} if code.as_str()=="nativeUnavailable")
            );
        }
        assert_eq!(
            serde_json::to_value(
                read_native_push_record(&runtime, id)
                    .await
                    .last_outcome
                    .unwrap()
            )
            .unwrap(),
            receipt
        );
        assert_eq!(runtime.client.calls(id), 1);
        drop(engine);
        runtime.close().await;
    }
}

#[tokio::test]
async fn direct_defects_are_terminal_but_follow_defects_keep_item_backoff_and_full_receipts() {
    for cause in [UnsentCause::InvalidInput, UnsentCause::ProcessingFailure] {
        for is_follow in [false, true] {
            // Arrange: identical classified no-effect failure through the real actor and SDK task.
            let runtime = Runtime::new().await;
            let item = if is_follow {
                follow(&runtime, 2, 10)
            } else {
                runtime.fixture.item(&runtime.fixture.draft())
            };
            let id = item.id();
            let delivery = if cause == UnsentCause::ProcessingFailure {
                NativeDeliveryResult::rejected(
                    DeliveryRejection {
                        native_failure: None,
                        reason: DeliveryRejectionReason::UnsupportedCapability,
                        next_action: DeliveryNextAction::CorrectRequest,
                        client_code: None,
                        detail: Some("same complete native detail".to_owned()),
                        claims: None,
                    },
                    cause,
                )
            } else {
                NativeDeliveryResult::not_submitted("same complete native detail", false, cause)
            };
            let receipt = serde_json::to_value(delivery.receipt()).unwrap();
            runtime.client.script_results(id, vec![delivery]);
            runtime.fixture.receive_items(vec![item]).await.unwrap();
            // Act.
            let engine = runtime.engine();
            engine.drive_due().await.unwrap();
            // Assert: 507 keeps legacy Follow retry; 487 gives direct defects their precise home.
            let outcome = runtime.fixture.local_outcome(id).await.unwrap();
            if is_follow {
                assert!(matches!(outcome, RecipientOutcome::Held));
                assert_eq!(runtime.count(id).await, 1);
                // The new attempt is admitted now with a not_before input.
                // Its first real check schedules that deadline without I/O.
                engine.drive_due().await.unwrap();
                assert_eq!(runtime.client.calls(id), 1);
                assert_eq!(
                    runtime.next_attempt(id, 1).await,
                    recorded_at().timestamp_millis() + 30_000
                );
                assert_eq!(
                    read_native_push_record(&runtime, id).await.delivery_state,
                    PushDeliveryState::Held
                );
            } else {
                let expected = if cause == UnsentCause::InvalidInput {
                    DeliveryFailureKind::InvalidPayload
                } else {
                    DeliveryFailureKind::ExecutionFailed
                };
                assert!(
                    matches!(outcome, RecipientOutcome::Refused {reason: DeliveryRefusal::ProcessingFailed {kind}} if kind==expected)
                );
                assert_eq!(runtime.count(id).await, 0);
                assert_eq!(
                    read_native_push_record(&runtime, id).await.delivery_state,
                    PushDeliveryState::Rejected
                );
            }
            assert_eq!(
                serde_json::to_value(
                    read_native_push_record(&runtime, id)
                        .await
                        .last_outcome
                        .unwrap()
                )
                .unwrap(),
                receipt
            );
            assert_eq!(runtime.client.calls(id), 1);
            drop(engine);
            let runtime = runtime.reopen().await;
            assert_eq!(runtime.count(id).await, u32::from(is_follow));
            runtime.close().await;
        }
    }
}

#[tokio::test]
async fn unloaded_direct_policy_holds_or_returns_coded_refusal_without_native_submission() {
    for (mode, guarded, expected) in [
        (MessageDelivery::Auto, false, None),
        (MessageDelivery::Steer, false, Some("steerUnsupported")),
        (MessageDelivery::Queue, true, Some("staleGeneration")),
    ] {
        // Arrange: actual presence probe boundary reports a known unloaded recipient.
        let runtime = Runtime::new().await;
        runtime.set_presence(Arc::new(Wakeable));
        let mut draft = runtime.fixture.draft();
        draft.mode = Some(mode);
        if guarded {
            draft.guard = Some(
                serde_json::from_value(json!({
                    "serviceEpoch":"00000000-0000-4000-8000-000000000007","generation":7
                }))
                .unwrap(),
            );
        }
        let item = runtime.fixture.item(&draft);
        let id = item.id();
        runtime.fixture.receive_items(vec![item]).await.unwrap();
        // Act: no scripted native answer exists, so an unexpected native call fails.
        let engine = runtime.engine();
        engine.drive_due().await.unwrap();
        // Assert: preparatory hard refusal uses the same coded SDK mapping, not policy drop.
        match expected {
            None => assert!(matches!(
                runtime.fixture.local_outcome(id).await,
                Some(RecipientOutcome::Held)
            )),
            Some(expected) => assert!(matches!(runtime.fixture.local_outcome(id).await,
                Some(RecipientOutcome::Refused {reason: DeliveryRefusal::RecipientRefused {code}}) if code.as_str()==expected)),
        }
        assert_eq!(runtime.client.calls(id), 0);
        assert_eq!(runtime.count(id).await, 0);
        drop(engine);
        runtime.close().await;
    }
}

#[tokio::test]
async fn cold_replay_after_attempt_mark_holds_unloaded_auto_without_native_submission() {
    // Arrange: lose the call-local policy after committing the actual attempt projection.
    let runtime = Runtime::new().await;
    let mut draft = runtime.fixture.draft();
    draft.mode = Some(MessageDelivery::Auto);
    let item = runtime.fixture.item(&draft);
    let push_id = draft.push_id.clone();
    runtime
        .fixture
        .receive_items(vec![runtime.fixture.item(&draft)])
        .await
        .unwrap();
    runtime
        .fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                interaction_storage::mark_mail_push_attempted_in_scope(scope, &push_id)
                    .await
                    .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await
        .unwrap();
    let runtime = runtime.reopen().await;
    runtime.set_presence(Arc::new(Wakeable));
    assert_eq!(
        read_native_push_record(&runtime, item.id())
            .await
            .delivery_state,
        PushDeliveryState::Attempted
    );
    let carrier = runtime
        .fixture
        .host
        .consumer
        .native
        .lock()
        .unwrap()
        .clone()
        .unwrap();
    let call = runtime_primitives::CallId {
        operation: runtime_primitives::OperationId::new(uuid::Uuid::now_v7()).unwrap(),
        step: runtime_primitives::StepName::new("deliver").unwrap(),
    };
    // Act: the default SDK callback has no pre-attempt load permission to carry.
    let delivery = carrier.deliver_push(&item, &call).await.unwrap().unwrap();
    // Assert: even a saved Auto envelope cannot load after a cold replay.
    assert!(matches!(
        delivery.outcome,
        LocalOutcome::Held { until: None }
    ));
    assert_eq!(runtime.client.calls(item.id()), 0);
    drop(carrier);
    runtime.close().await;
}

#[tokio::test]
async fn coded_direct_outcome_rollback_recovers_saved_answer_without_a_second_native_call() {
    // Arrange: a real direct native call succeeds as a definite refusal, then the owning projection fails.
    let runtime = Runtime::new().await;
    let draft = runtime.fixture.draft();
    let item = runtime.fixture.item(&draft);
    let id = item.id();
    let delivery = rejected_as(DeliveryRejectionReason::PermissionDenied);
    let receipt = serde_json::to_value(delivery.receipt()).unwrap();
    runtime.client.script_results(id, vec![delivery]);
    runtime.fixture.admit_local(draft).await.unwrap();
    runtime.fixture.sql("CREATE TRIGGER reject_coded_outcome BEFORE UPDATE ON router_pushes WHEN NEW.delivery_state<>'attempted' BEGIN SELECT RAISE(ABORT, 'coded outcome failure'); END").await;
    let engine = runtime.engine();
    let report = engine.drive_due().await.unwrap();
    assert_eq!(report.retried.len(), 1, "{report:?}");
    assert!(runtime.fixture.local_outcome(id).await.is_none());
    assert_eq!(
        read_native_push_record(&runtime, id).await.delivery_state,
        PushDeliveryState::Attempted
    );
    assert_eq!(runtime.client.calls(id), 1);
    let retry_at = runtime.next_attempt(id, 0).await;
    assert!(retry_at > recorded_at().timestamp_millis());
    runtime
        .fixture
        .sql("DROP TRIGGER reject_coded_outcome")
        .await;
    drop(engine);
    // Act: cold reopen and advance to the journal-owned retry, without another scripted native answer.
    let runtime = runtime.reopen().await;
    runtime.set_time(retry_at - recorded_at().timestamp_millis());
    let engine = runtime.engine();
    engine.drive_due().await.unwrap();
    // Assert: custody, full push receipt and recorded RPC result commit together from the saved answer.
    assert!(
        matches!(runtime.fixture.local_outcome(id).await, Some(RecipientOutcome::Refused {reason: DeliveryRefusal::RecipientRefused {code}}) if code.as_str()=="permissionDenied")
    );
    assert_eq!(
        serde_json::to_value(
            read_native_push_record(&runtime, id)
                .await
                .last_outcome
                .unwrap()
        )
        .unwrap(),
        receipt
    );
    let answer_id = id.as_uuid().to_string().try_into().unwrap();
    let answer = runtime
        .fixture
        .owner
        .message_send_answer(&answer_id)
        .await
        .unwrap()
        .unwrap();
    let collaboration_protocol::MessageSendResult::Local(answer) = answer.result.unwrap() else {
        panic!("local delivery recorded remote sender custody");
    };
    assert_eq!(serde_json::to_value(answer.receipt).unwrap(), receipt);
    assert_eq!(runtime.client.calls(id), 1);
    drop(engine);
    runtime.close().await;
}
