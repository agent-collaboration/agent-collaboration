//! Literal native policy and header expectations, without a sender-project lookup.
use super::*;
use collaboration_mail_protocol::{
    FollowDeliveryPolicy, MailSender, MessageBody, NotificationBody, RecordPointer, RootRange,
};
use collaboration_protocol::{CodexGeneration, PushDeliveryState};
use fleet_sync_protocol::{ItemId, PeerId, Recipient};
use message_board::{
    BatchTiming, FeedCursor, Identity, SubscriptionGeneration, SubscriptionLifetime,
};
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
fn cursor(position: u64) -> FeedCursor {
    FeedCursor {
        epoch: message_board::EpochNumber::INITIAL,
        position: position.try_into().unwrap(),
    }
}
fn follow(mode: SubscriptionMode, idle: WhenIdle) -> DeliveryItem<RouterDeliveryTypes> {
    let recipient = recipient();
    let project: message_board::ProjectId = uuid::Uuid::now_v7().to_string().try_into().unwrap();
    let root: message_board::ThreadId = uuid::Uuid::now_v7().to_string().try_into().unwrap();
    let roots = if mode == SubscriptionMode::Poll {
        vec![RootRange::new(root.clone(), cursor(2), cursor(500), 7).unwrap()]
    } else {
        vec![
            RootRange::new(root.clone(), cursor(2), cursor(300), 7).unwrap(),
            RootRange::new(root.clone(), cursor(400), cursor(500), 11).unwrap(),
        ]
    };
    DeliveryItem::new(
        ItemId::new(uuid::Uuid::now_v7()).unwrap(),
        MailSender::Project {
            project: project.clone(),
        },
        recipient.clone(),
        None,
        BoardDeliveryBody::Notification(NotificationBody {
            excerpt: "frozen follow".to_owned().try_into().unwrap(),
            pointer: RecordPointer {
                project,
                subject: serde_json::from_value(json!({"kind":"post","id":root.as_str()})).unwrap(),
                position: cursor(500),
            },
            source: NotificationSource::FollowBatch {
                membership: uuid::Uuid::now_v7().to_string().try_into().unwrap(),
                activated_at: cursor(1),
                generation: SubscriptionGeneration::new(1).unwrap(),
                roots,
                policy: FollowDeliveryPolicy::new(
                    &Identity::Session {
                        session: recipient.address,
                    },
                    mode,
                    idle,
                    BatchTiming::new(2, 10).unwrap(),
                    SubscriptionLifetime::new(600).unwrap(),
                )
                .unwrap(),
                draining: true,
            },
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
fn follow_policy_keeps_frozen_counts_and_draining_and_only_uses_local_hold_time() {
    let item = follow(SubscriptionMode::Deliver, WhenIdle::Drop);
    let mut local = working(&item);
    if let PushHeaderFacts::SubscriptionActivity {
        held_since,
        message_count,
        ..
    } = &mut local.header_facts
    {
        *held_since = Some("2026-10-19T12:00:00Z".to_owned());
        *message_count = 999;
    }
    let Some(MailPushPreparation::Attempt { request, .. }) =
        prepare_mail_push(&item, &local, &machine(), &TargetPresence::Running, &call()).unwrap()
    else {
        panic!("running reader ignores idle drop");
    };
    assert_eq!(request.payload.load_policy, LoadPolicy::LoadedOnly);
    assert_eq!(request.mode, MessageDelivery::Auto);
    assert!(
        request
            .payload
            .line
            .as_str()
            .contains("1 thread · 18 messages")
    );
    assert!(
        request
            .payload
            .line
            .as_str()
            .contains("held since 2026-10-19T12:00:00Z")
    );
    assert!(request.payload.line.as_str().contains("thread resolved"));
    assert!(matches!(
        prepare(&item, TargetPresence::Wakeable),
        MailPushPreparation::Drop { .. }
    ));
    assert!(matches!(
        prepare(
            &item,
            TargetPresence::Unreachable {
                reason: "ambiguous".to_owned()
            }
        ),
        MailPushPreparation::Drop { .. }
    ));
}

#[test]
fn follow_wakes_only_known_wakeable_and_poll_never_prepares_a_native_push() {
    let wake = follow(SubscriptionMode::Deliver, WhenIdle::Wake);
    let MailPushPreparation::Attempt { request, .. } = prepare(&wake, TargetPresence::Wakeable)
    else {
        panic!("wake policy must load");
    };
    assert_eq!(request.payload.load_policy, LoadPolicy::MayLoad);
    assert!(matches!(
        prepare(
            &wake,
            TargetPresence::Unreachable {
                reason: "ambiguous".to_owned()
            }
        ),
        MailPushPreparation::Hold { .. }
    ));
    let hold = follow(SubscriptionMode::Deliver, WhenIdle::Hold);
    assert!(matches!(
        prepare(&hold, TargetPresence::Wakeable),
        MailPushPreparation::Hold { .. }
    ));
    let poll = follow(SubscriptionMode::Poll, WhenIdle::Hold);
    assert!(
        prepare_mail_push(
            &poll,
            &working(&wake),
            &machine(),
            &TargetPresence::Running,
            &call()
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn intent_is_loaded_only_and_mismatched_push_or_router_is_refused_before_dispatch() {
    let template = follow(SubscriptionMode::Deliver, WhenIdle::Wake);
    let mut body = template.body().clone();
    let BoardDeliveryBody::Notification(notification) = &mut body else {
        panic!("notification");
    };
    notification.source = NotificationSource::Intent {
        intent: uuid::Uuid::now_v7().to_string().try_into().unwrap(),
    };
    let item = DeliveryItem::new(
        template.id(),
        template.sender().clone(),
        template.recipient().clone(),
        None,
        body,
    )
    .unwrap();
    assert!(matches!(
        prepare(&item, TargetPresence::Wakeable),
        MailPushPreparation::Hold { .. }
    ));
    let MailPushPreparation::Attempt { request, kind } = prepare(&item, TargetPresence::Running)
    else {
        panic!("loaded intent must attempt");
    };
    assert_eq!(request.payload.load_policy, LoadPolicy::LoadedOnly);
    assert_eq!(kind, PushKind::BoardNotification);
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
