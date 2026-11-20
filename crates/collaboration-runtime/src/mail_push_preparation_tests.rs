//! Literal native policy and header expectations, without a sender-project lookup.
use super::*;
use collaboration_mail_protocol::{MailSender, MessageBody};
use collaboration_protocol::{CodexGeneration, PushDeliveryState, PushHeaderFacts};
use fleet_sync_protocol::{ItemId, PeerId, Recipient};
use runtime_primitives::{OperationId, StepName};
use serde_json::json;

fn machine() -> MachineIdentity {
    MachineIdentity::new(
        "00000000-0000-4000-8000-000000000001"
            .to_owned()
            .try_into()
            .unwrap(),
        Some("Recipient"),
    )
    .unwrap()
}
fn recipient() -> Recipient<message_board::SessionRef> {
    Recipient {
        peer: PeerId::new(uuid::Uuid::now_v7()).unwrap(),
        address: serde_json::from_value(json!({
            "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},
            "sessionId":"project-free-recipient"
        })).unwrap(),
    }
}
fn call() -> CallId {
    CallId {
        operation: OperationId::new(uuid::Uuid::now_v7()).unwrap(),
        step: StepName::new("deliver").unwrap(),
    }
}
fn message(
    mode: MessageDelivery,
    guard: Option<CodexGeneration>,
) -> DeliveryItem<RouterDeliveryTypes> {
    DeliveryItem::new(
        ItemId::new(uuid::Uuid::now_v7()).unwrap(),
        MailSender::Session(serde_json::from_value(json!({
            "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000002","endpointId":"codex-local"},
            "sessionId":"frozen-sender"
        })).unwrap()),
        recipient(),
        None,
        BoardDeliveryBody::Message(MessageBody {
            text: "literal frozen body".to_owned().try_into().unwrap(),
            reply_to: None,
            sender_display_name: Some("Frozen Sender".to_owned().try_into().unwrap()),
            delivery: mode.into(),
            guard: guard.map(TryInto::try_into).transpose().unwrap(),
        }),
    )
    .unwrap()
}
fn working(item: &DeliveryItem<RouterDeliveryTypes>) -> PushRecord {
    received_mail_push(item, chrono::Utc::now())
        .unwrap()
        .unwrap()
        .into_pending()
        .unwrap()
}
fn prepare(
    item: &DeliveryItem<RouterDeliveryTypes>,
    presence: TargetPresence,
) -> MailPushPreparation {
    prepare_mail_push(item, &working(item), &machine(), &presence, &call())
        .unwrap()
        .unwrap()
}

#[test]
fn message_request_keeps_frozen_label_mode_guard_and_stable_call_identity() {
    // Arrange: the stored copy differs, so it cannot supply frozen sender/text facts.
    let guard: CodexGeneration = serde_json::from_value(
        json!({"serviceEpoch":"00000000-0000-4000-8000-000000000007","generation":7}),
    )
    .unwrap();
    let item = message(MessageDelivery::Steer, Some(guard.clone()));
    let mut local = working(&item);
    local.body = Some("stale local text".to_owned());
    local.header_facts = PushHeaderFacts::DirectMessage {
        sender_display_name: None,
    };
    let call = call();
    // Act: prepare twice under the same journaled call identity.
    for _ in 0..2 {
        let Some(MailPushPreparation::Attempt { kind, request }) =
            prepare_mail_push(&item, &local, &machine(), &TargetPresence::Running, &call).unwrap()
        else {
            panic!("running guarded message must attempt");
        };
        // Assert: literal native contract; a fresh attempt ID or receiver lookup fails this.
        assert_eq!(kind, PushKind::DirectMessage);
        assert_eq!(request.mode, MessageDelivery::Steer);
        assert_eq!(request.payload.load_policy, LoadPolicy::LoadedOnly);
        assert_eq!(
            request.attempt.as_str(),
            call.operation.as_uuid().to_string()
        );
        assert_eq!(
            request.payload.push_id.as_str(),
            item.id().as_uuid().to_string()
        );
        assert!(request.payload.line.as_str().contains("Frozen Sender"));
        assert!(
            request
                .payload
                .line
                .as_str()
                .contains("literal frozen body")
        );
        assert!(!request.payload.line.as_str().contains("stale local text"));
        assert!(
            matches!(request.precondition, DeliveryPrecondition::EndpointGeneration {expected} if expected==guard)
        );
    }
}

#[test]
fn new_auto_wakes_unloaded_while_held_queue_and_guarded_policies_stay_strict() {
    let auto = message(MessageDelivery::Auto, None);
    let local = working(&auto);
    let policy = direct_message_load_policy(local.mode, local.guard.as_ref(), local.delivery_state);
    let Some(MailPushPreparation::Attempt { request, .. }) = prepare_mail_push_with_load_policy(
        &auto,
        &local,
        &machine(),
        &TargetPresence::Wakeable,
        &call(),
        policy,
    )
    .unwrap() else {
        panic!("fresh unguarded auto must reach native loading");
    };
    assert_eq!(request.payload.load_policy, LoadPolicy::MayLoad);
    let queue = message(MessageDelivery::Queue, None);
    assert!(matches!(
        prepare(&queue, TargetPresence::Wakeable),
        MailPushPreparation::Hold { .. }
    ));
    let mut held = working(&auto);
    held.delivery_state = collaboration_protocol::PushDeliveryState::Held;
    held.last_outcome = Some(collaboration_protocol::DeliveryReceipt {
        outcome: DeliveryOutcome::NotSubmitted {
            retryable: true,
            reason: "previous native attempt held".into(),
        },
        reachability: None,
        client: None,
    });
    assert!(matches!(
        prepare_mail_push(&auto, &held, &machine(), &TargetPresence::Wakeable, &call()).unwrap(),
        Some(MailPushPreparation::Hold { .. })
    ));
    assert_eq!(
        direct_message_load_policy(held.mode, held.guard.as_ref(), held.delivery_state),
        LoadPolicy::LoadedOnly,
    );
    let mut attempted = working(&auto);
    attempted.delivery_state = PushDeliveryState::Attempted;
    assert!(matches!(
        prepare_mail_push(
            &auto,
            &attempted,
            &machine(),
            &TargetPresence::Wakeable,
            &call()
        )
        .unwrap(),
        Some(MailPushPreparation::Hold { .. })
    ));
    let steer = message(MessageDelivery::Steer, None);
    let MailPushPreparation::Refuse { delivery } = prepare(&steer, TargetPresence::Wakeable) else {
        panic!("unloaded steer must refuse");
    };
    assert!(
        matches!(&delivery.receipt().outcome, DeliveryOutcome::Rejected(rejection) if rejection.reason==DeliveryRejectionReason::SteerUnsupported)
    );
    let guarded = message(
        MessageDelivery::Queue,
        Some(
            serde_json::from_value(
                json!({"serviceEpoch":"00000000-0000-4000-8000-000000000007","generation":2}),
            )
            .unwrap(),
        ),
    );
    let MailPushPreparation::Refuse { delivery } = prepare(&guarded, TargetPresence::Wakeable)
    else {
        panic!("unloaded guarded message must refuse");
    };
    assert!(
        matches!(&delivery.receipt().outcome, DeliveryOutcome::Rejected(rejection) if rejection.reason==DeliveryRejectionReason::StaleGeneration)
    );
    assert!(matches!(
        prepare(
            &auto,
            TargetPresence::Unreachable {
                reason: "ambiguous".to_owned()
            }
        ),
        MailPushPreparation::Attempt { .. }
    ));
}

#[test]
fn mismatched_direct_push_or_router_is_refused_before_dispatch() {
    let item = message(MessageDelivery::Auto, None);
    let mut wrong = working(&item);
    wrong.push_id = uuid::Uuid::now_v7().to_string().try_into().unwrap();
    assert!(
        prepare_mail_push(&item, &wrong, &machine(), &TargetPresence::Running, &call()).is_err()
    );
    let other = MachineIdentity::new(
        "00000000-0000-4000-8000-000000000003"
            .to_owned()
            .try_into()
            .unwrap(),
        Some("Other"),
    )
    .unwrap();
    assert!(
        prepare_mail_push(
            &item,
            &working(&item),
            &other,
            &TargetPresence::Running,
            &call()
        )
        .is_err()
    );
    assert_eq!(working(&item).delivery_state, PushDeliveryState::Pending);
}
