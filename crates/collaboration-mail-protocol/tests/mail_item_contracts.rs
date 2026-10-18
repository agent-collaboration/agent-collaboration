//! E30 uses a literal independent digest oracle and rejects malformed stored/wire items.
use collaboration_mail_protocol::*;
use serde_json::{Value, json};

#[allow(clippy::expect_used)] // Fixed wire literals must fail the test if invalid.
fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/message_item.json"))
        .expect("valid literal mail fixture")
}
#[allow(clippy::expect_used)] // Test-only conversion of the literal's content.
fn content(mut wire: Value) -> MailItemContent {
    wire.as_object_mut().expect("mail object").remove("digest");
    serde_json::from_value(wire).expect("valid literal mail content")
}
#[allow(clippy::expect_used)] // Test-only decoding of the independent fixture.
fn item() -> MailItem {
    serde_json::from_value(fixture()).expect("valid literal mail item")
}

#[test]
fn digest_is_canonical_and_covers_every_delivery_input() {
    let wire = fixture();
    let base = MailItem::new(content(wire.clone())).unwrap();
    assert_eq!(base.digest().as_str(), wire["digest"].as_str().unwrap());
    assert_eq!(serde_json::to_value(&base).unwrap(), wire);
    let mut changes = Vec::new();
    for (pointer, replacement) in [
        ("/id", json!("01900000-0000-7000-8000-000000000002")),
        (
            "/sender/installation",
            json!("00000000-0000-4000-8000-000000000009"),
        ),
        (
            "/recipient/endpoint/serviceId",
            json!("00000000-0000-4000-8000-000000000009"),
        ),
        ("/recipient/endpoint/endpointId", json!("claude-local")),
        ("/recipient/sessionId", json!("another-recipient")),
        ("/validUntil", json!("2026-10-16T11:00:00Z")),
        ("/body/text", json!("Different text")),
        ("/body/delivery", json!("auto")),
        (
            "/body/guard/serviceEpoch",
            json!("00000000-0000-4000-8000-000000000009"),
        ),
        ("/body/guard/generation", json!(9_007_199_254_740_991_u64)),
        ("/body/guard", Value::Null),
    ] {
        let mut changed = wire.clone();
        *changed.pointer_mut(pointer).unwrap() = replacement;
        changes.push((pointer, changed));
    }
    for (field, wire) in changes {
        let changed = MailItem::new(content(wire)).unwrap();
        assert_ne!(changed.digest(), base.digest(), "digest omitted {field}");
    }
    fn reverse_objects(value: &mut Value) {
        match value {
            Value::Object(object) => {
                let entries: Vec<_> = std::mem::take(object).into_iter().rev().collect();
                for (key, mut value) in entries {
                    reverse_objects(&mut value);
                    object.insert(key, value);
                }
            }
            Value::Array(values) => values.iter_mut().for_each(reverse_objects),
            _ => {}
        }
    }
    let mut reordered = wire;
    reverse_objects(&mut reordered);
    assert_eq!(serde_json::from_value::<MailItem>(reordered).unwrap(), base);
}

#[test]
fn digest_distinguishes_adjacent_large_board_positions() {
    let mut notification = content(fixture());
    notification.sender = MailSender::Project {
        project: "01900000-0000-7000-8000-000000000010"
            .to_owned()
            .try_into()
            .unwrap(),
    };
    let body = json!({"excerpt":"A reference","pointer":{"project":"01900000-0000-7000-8000-000000000010","subject":{"kind":"task","id":"01900000-0000-7000-8000-000000000011"},"position":{"epoch":1,"position":9_007_199_254_740_992_u64}},"source":{"kind":"intent","intent":"01900000-0000-7000-8000-000000000012"}});
    notification.body = MailBody::Notification(serde_json::from_value(body.clone()).unwrap());
    let first = MailItem::new(notification.clone()).unwrap();
    let mut changed = body;
    changed["pointer"]["position"]["position"] = json!(9_007_199_254_740_993_u64);
    notification.body = MailBody::Notification(serde_json::from_value(changed).unwrap());
    let second = MailItem::new(notification).unwrap();
    assert_ne!(first.body(), second.body());
    assert_ne!(
        first.digest(),
        second.digest(),
        "different positions shared a digest"
    );
}

#[test]
fn decoding_is_closed_and_validates_ids_text_guard_and_digest() {
    let base = fixture();
    for location in [
        "",
        "/sender",
        "/recipient",
        "/recipient/endpoint",
        "/body",
        "/body/guard",
    ] {
        let mut wire = base.clone();
        wire.pointer_mut(location)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("extra".into(), json!(true));
        assert!(
            serde_json::from_value::<MailItem>(wire).is_err(),
            "accepted unknown field at {location}"
        );
    }
    for (pointer, invalid) in [
        ("/id", json!("00000000-0000-4000-8000-000000000001")),
        ("/id", json!("")),
        ("/digest", json!("0".repeat(64))),
        ("/digest", json!("invalid")),
        ("/body/text", json!("")),
        ("/body/text", json!("bad\u{0008}")),
        ("/body/text", json!("🦁".repeat(16_385))),
        ("/body/guard/generation", json!(0)),
        ("/body/guard/serviceEpoch", json!("NOT-A-UUID")),
    ] {
        let mut wire = base.clone();
        *wire.pointer_mut(pointer).unwrap() = invalid;
        assert!(
            serde_json::from_value::<MailItem>(wire).is_err(),
            "accepted invalid {pointer}"
        );
    }
    assert!(MessageText::try_from("🦁".repeat(16_384)).is_ok());
    assert!(MessageText::try_from("layout\n\tretained".to_owned()).is_ok());
    assert!(Excerpt::try_from("🦁".repeat(280)).is_ok());
    assert!(Excerpt::try_from("🦁".repeat(281)).is_err());
}

#[test]
fn mismatched_kind_and_body_are_refused() {
    let mut mismatched = fixture();
    mismatched["body"]["kind"] = json!("notification");
    assert!(
        serde_json::from_value::<MailItem>(mismatched).is_err(),
        "notification tag accepted message body"
    );
    for kind in ["wake", "unknown"] {
        let mut wire = fixture();
        wire["body"]["kind"] = json!(kind);
        assert!(serde_json::from_value::<MailItem>(wire).is_err());
    }
}

#[test]
fn sender_installation_and_content_kind_are_bound_without_fabricating_sessions() {
    let base = item();
    let own = "00000000-0000-4000-8000-000000000001"
        .to_owned()
        .try_into()
        .unwrap();
    let other = "00000000-0000-4000-8000-000000000009"
        .to_owned()
        .try_into()
        .unwrap();
    assert!(base.sender().matches_submitting_installation(&own));
    assert!(!base.sender().matches_submitting_installation(&other));
    let mut session = content(fixture());
    session.sender = MailSender::Session(
        serde_json::from_value(
            json!({"endpoint":{"serviceId":own,"endpointId":"codex-local"},"sessionId":"sender"}),
        )
        .unwrap(),
    );
    let session = MailItem::new(session).unwrap();
    assert!(session.sender().matches_submitting_installation(&own));
    assert!(!session.sender().matches_submitting_installation(&other));
    assert_ne!(session.digest(), base.digest());

    let mut notification = content(fixture());
    notification.body = MailBody::Notification(serde_json::from_value(json!({"excerpt":"A reference","pointer":{"project":"01900000-0000-7000-8000-000000000010","subject":{"kind":"task","id":"01900000-0000-7000-8000-000000000011"},"position":{"epoch":1,"position":1}},"source":{"kind":"intent","intent":"01900000-0000-7000-8000-000000000012"}})).unwrap());
    assert!(
        MailItem::new(notification.clone()).is_err(),
        "human sent notification"
    );
    notification.sender = MailSender::Project {
        project: "01900000-0000-7000-8000-000000000010"
            .to_owned()
            .try_into()
            .unwrap(),
    };
    let notification = MailItem::new(notification).unwrap();
    assert_eq!(notification.kind(), MailKind::Notification);
    assert!(
        notification
            .sender()
            .matches_submitting_installation(&other)
    );
    let mut project_message = content(fixture());
    project_message.sender = notification.sender().clone();
    assert!(
        MailItem::new(project_message).is_err(),
        "project sent direct message"
    );
}

#[test]
fn cancel_causes_and_sender_mismatch_refusal_round_trip_as_closed_variants() {
    let original = item();
    let target = original.id().clone();
    let refusal = MailRefusal::CancelSenderMismatch {
        target: target.clone(),
    };
    let expected = json!({"kind":"cancelSenderMismatch","target":target});
    assert_eq!(serde_json::to_value(&refusal).unwrap(), expected);
    assert_eq!(
        serde_json::from_value::<MailRefusal>(expected).unwrap(),
        refusal
    );
    let requester: message_board::Identity =
        serde_json::from_value(json!({"kind":"human","humanId":"declared-owner"})).unwrap();
    for cause in [
        CancelCause::BySender {},
        CancelCause::ByRequester { by: requester },
        CancelCause::BoardDerived {},
    ] {
        let wire = serde_json::to_value(&cause).unwrap();
        assert_eq!(serde_json::from_value::<CancelCause>(wire).unwrap(), cause);
        let mut extra = serde_json::to_value(&cause).unwrap();
        extra["extra"] = json!(true);
        assert!(serde_json::from_value::<CancelCause>(extra).is_err());
    }
    let mut cancel = content(fixture());
    cancel.body = MailBody::Cancel(CancelBody {
        target: CancelTarget::Item(target),
        cause: CancelCause::BySender {},
    });
    let cancel = MailItem::new(cancel).unwrap();
    assert_eq!(cancel.kind(), MailKind::Cancel);
    assert_eq!(
        serde_json::from_value::<MailItem>(serde_json::to_value(&cancel).unwrap()).unwrap(),
        cancel
    );
    assert!(
        serde_json::from_value::<MailRefusal>(
            json!({"kind":"cancelSenderMismatch","target":original.id(),"extra":true})
        )
        .is_err()
    );
    assert!(serde_json::from_value::<CancelCause>(json!({"kind":"requested","by":{}})).is_err());
}

#[test]
fn ranges_statuses_and_reports_keep_their_validated_distinct_shapes() {
    let cursor = |epoch, position| {
        serde_json::from_value::<message_board::FeedCursor>(
            json!({"epoch":epoch,"position":position}),
        )
        .unwrap()
    };
    let root: message_board::ThreadId = "01900000-0000-7000-8000-000000000020"
        .to_owned()
        .try_into()
        .unwrap();
    assert!(
        RootRange::new(root.clone(), cursor(1, 2), cursor(2, 3)).is_err(),
        "range crossed epoch line"
    );
    assert!(RootRange::new(root.clone(), cursor(1, 3), cursor(1, 2)).is_err());
    let range = RootRange::new(root.clone(), cursor(1, 2), cursor(1, 3)).unwrap();
    assert_eq!(range.root(), &root);
    assert_eq!(range.from(), cursor(1, 2));
    assert_eq!(range.through(), cursor(1, 3));
    assert_eq!(
        serde_json::from_value::<RootRange>(serde_json::to_value(&range).unwrap()).unwrap(),
        range
    );
    let reader = message_board::Identity::Session {
        session: item().recipient().clone(),
    };
    let policy = FollowDeliveryPolicy::new(
        &reader,
        message_board::SubscriptionMode::Deliver,
        message_board::WhenIdle::Hold,
        message_board::BatchTiming::new(2, 10).unwrap(),
        message_board::SubscriptionLifetime::new(600).unwrap(),
    )
    .unwrap();
    let source = NotificationSource::FollowBatch {
        membership: "01900000-0000-7000-8000-000000000021"
            .to_owned()
            .try_into()
            .unwrap(),
        activated_at: cursor(1, 1),
        generation: message_board::SubscriptionGeneration::new(1).unwrap(),
        roots: vec![range],
        policy,
    };
    assert_eq!(
        serde_json::from_value::<NotificationSource>(serde_json::to_value(&source).unwrap())
            .unwrap(),
        source
    );
    let target = item().id().clone();
    for status in [
        MailStatus::Queued {},
        MailStatus::Sent {
            seq: MailSeq::new(1),
        },
        MailStatus::AtMachine {},
        MailStatus::Held {},
        MailStatus::Delivered {
            acceptance: CarrierAcceptance::PeerMessageWritten,
        },
        MailStatus::Uncertain {},
        MailStatus::Expired {},
        MailStatus::Cancelled {},
        MailStatus::Refused {
            reason: MailRefusal::CancelSenderMismatch {
                target: target.clone(),
            },
        },
    ] {
        assert_eq!(
            serde_json::from_value::<MailStatus>(serde_json::to_value(&status).unwrap()).unwrap(),
            status
        );
        let mut extra = serde_json::to_value(&status).unwrap();
        extra["extra"] = json!(true);
        assert!(serde_json::from_value::<MailStatus>(extra).is_err());
    }
    for outcome in [
        RecipientOutcome::Held {},
        RecipientOutcome::Delivered {
            acceptance: CarrierAcceptance::Queued,
        },
        RecipientOutcome::Uncertain {},
        RecipientOutcome::Expired {},
        RecipientOutcome::Prevented {
            cancel: target.clone(),
        },
        RecipientOutcome::TooLate {
            cancel: target.clone(),
            at: TooLateAt::Uncertain,
        },
        RecipientOutcome::Dropped {},
        RecipientOutcome::Refused {
            reason: MailRefusal::ItemConflict {
                existing: item().digest().clone(),
            },
        },
    ] {
        assert_eq!(
            serde_json::from_value::<RecipientOutcome>(serde_json::to_value(&outcome).unwrap())
                .unwrap(),
            outcome
        );
        let mut extra = serde_json::to_value(&outcome).unwrap();
        extra["extra"] = json!(true);
        assert!(serde_json::from_value::<RecipientOutcome>(extra).is_err());
    }
    for refusal in [
        MailRefusal::MailboxFull {},
        MailRefusal::ItemConflict {
            existing: item().digest().clone(),
        },
        MailRefusal::RecipientUnknown {},
        MailRefusal::RecipientRevoked {},
        MailRefusal::RecipientUnsupported {},
        MailRefusal::DroppedByPolicy {},
        MailRefusal::CancelSenderMismatch { target },
    ] {
        let wire = serde_json::to_value(&refusal).unwrap();
        assert_eq!(
            serde_json::from_value::<MailRefusal>(wire.clone()).unwrap(),
            refusal
        );
        let mut extra = wire;
        extra["extra"] = json!(true);
        assert!(serde_json::from_value::<MailRefusal>(extra).is_err());
    }
    for result in [
        CancelResult::Pending {},
        CancelResult::Prevented {},
        CancelResult::TooLate {
            at: TooLateAt::Delivered,
        },
    ] {
        let wire = serde_json::to_value(&result).unwrap();
        assert_eq!(
            serde_json::from_value::<CancelResult>(wire.clone()).unwrap(),
            result
        );
        let mut extra = wire;
        extra["extra"] = json!(true);
        assert!(serde_json::from_value::<CancelResult>(extra).is_err());
    }
    assert_eq!(
        serde_json::to_value(StatusVersion::new(4)).unwrap(),
        json!(4)
    );
    assert_eq!(serde_json::to_value(ReportSeq::new(7)).unwrap(), json!(7));
    assert!(serde_json::from_value::<ReportSeq>(json!(-1)).is_err());
    assert!(serde_json::from_value::<MailStatus>(json!({"kind":"finished"})).is_err());
    assert!(
        serde_json::from_value::<RecipientOutcome>(json!({"kind":"held","extra":true})).is_err()
    );
    assert!(MailStatus::Uncertain {}.is_terminal());
    assert!(!MailStatus::Held {}.is_terminal());
}
