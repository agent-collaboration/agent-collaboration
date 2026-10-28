//! The recipient's real push renderer has no project or sender-label lookup.
use super::*;
use collaboration_mail_protocol::{
    FollowDeliveryPolicy, NotificationBody, RecordPointer, RootRange,
};
use collaboration_protocol::{
    MachineId, MachineLabel, PushLineInput, RouterLink, render_push_line,
};
use fleet_sync_protocol::{ItemId, PeerId, Recipient};
use message_board::{
    BatchTiming, FeedCursor, Identity, PendingRootNotice, SubscriptionBatch,
    SubscriptionGeneration, SubscriptionLifetime, SubscriptionMode, WhenIdle,
};
use serde_json::json;

fn feed_cursor(position: u64) -> FeedCursor {
    FeedCursor {
        epoch: message_board::EpochNumber::INITIAL,
        position: position.try_into().unwrap(),
    }
}

fn follow_item(
    draining: bool,
    counts: [u64; 2],
    mode: SubscriptionMode,
) -> DeliveryItem<RouterDeliveryTypes> {
    let target: message_board::SessionRef = serde_json::from_value(json!({
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},
        "sessionId":"recipient-without-project"
    }))
    .unwrap();
    let policy = FollowDeliveryPolicy::new(
        &Identity::Session {
            session: target.clone(),
        },
        mode,
        WhenIdle::Hold,
        BatchTiming::new(2, 10).unwrap(),
        SubscriptionLifetime::new(600).unwrap(),
    )
    .unwrap();
    let mut roots: Vec<_> = [
        "01900000-0000-7000-8000-000000000010",
        "01900000-0000-7000-8000-000000000011",
    ]
    .into_iter()
    .zip(counts)
    .map(|(root, count)| {
        RootRange::new(
            root.to_owned().try_into().unwrap(),
            feed_cursor(2),
            feed_cursor(300),
            count,
        )
        .unwrap()
    })
    .collect();
    if mode == SubscriptionMode::Poll {
        roots.truncate(1);
    }
    let project: message_board::ProjectId = "01900000-0000-7000-8000-000000000020"
        .to_owned()
        .try_into()
        .unwrap();
    let body = NotificationBody {
        excerpt: "frozen notification".to_owned().try_into().unwrap(),
        pointer: RecordPointer {
            project: project.clone(),
            subject: serde_json::from_value(
                json!({"kind":"post","id":"01900000-0000-7000-8000-000000000010"}),
            )
            .unwrap(),
            position: feed_cursor(300),
        },
        source: NotificationSource::FollowBatch {
            membership: "01900000-0000-7000-8000-000000000030"
                .to_owned()
                .try_into()
                .unwrap(),
            activated_at: feed_cursor(1),
            generation: SubscriptionGeneration::new(1).unwrap(),
            roots,
            policy,
            draining,
        },
    };
    DeliveryItem::new(
        ItemId::new(uuid::Uuid::now_v7()).unwrap(),
        MailSender::Project { project },
        Recipient {
            peer: PeerId::new(uuid::Uuid::now_v7()).unwrap(),
            address: target,
        },
        None,
        board_sync_protocol::BoardDeliveryBody::Notification(body),
    )
    .unwrap()
}

#[test]
fn recipient_renders_frozen_counts_and_resolution_without_project_access() {
    for draining in [true, false] {
        // Arrange: the range spans 299 positions, but the producer counted 7+11 posts.
        let item = follow_item(draining, [7, 11], SubscriptionMode::Deliver);
        // Act: only frozen wire content enters the native projection and real renderer.
        let draft = received_mail_push(&item, Utc::now()).unwrap().unwrap();
        let line = render_push_line(&PushLineInput {
            link: RouterLink::new(
                MachineId::from(draft.target.endpoint.service_id.clone()),
                draft.push_id.clone(),
            ),
            machine_label: MachineLabel::try_from("Recipient".to_owned()).unwrap(),
            origin: draft.origin.clone(),
            header_facts: draft.header_facts.clone(),
            body: draft.body.clone(),
        })
        .unwrap();
        // Assert: literal contract values, independent of cursor arithmetic or project data.
        assert!(line.contains(" · 2 threads · 18 messages"), "{line}");
        assert_eq!(line.contains(" · thread resolved"), draining, "{line}");
        assert!(
            !line.contains("held since"),
            "producer hold metadata leaked: {line}"
        );
        let origin =
            RouterOriginRef::parse_canonical(draft.origin_router_ref.as_deref().unwrap()).unwrap();
        assert!(
            matches!(origin, RouterOriginRef::SubscriptionActivity {batch_id,..} if batch_id.as_str()==item.id().as_uuid().to_string())
        );
        assert!(!draft.activity.unwrap().held);
    }
}

#[test]
fn recipient_refuses_count_overflow_and_poll_does_not_create_a_push() {
    let overflow = follow_item(false, [u64::MAX, 1], SubscriptionMode::Deliver);
    assert!(received_mail_push(&overflow, Utc::now()).is_err());
    let poll = follow_item(true, [7, 11], SubscriptionMode::Poll);
    assert!(received_mail_push(&poll, Utc::now()).unwrap().is_none());
}

#[test]
fn recipient_keeps_disjoint_holes_for_one_thread_and_counts_the_thread_once() {
    // PD4 section 4c permits several disjoint ranges for one root in a replacement.
    let template = follow_item(false, [7, 11], SubscriptionMode::Deliver);
    let mut body = template.body().clone();
    let board_sync_protocol::BoardDeliveryBody::Notification(notification) = &mut body else {
        panic!("notification fixture");
    };
    let NotificationSource::FollowBatch { roots, .. } = &mut notification.source else {
        panic!("follow fixture");
    };
    roots[1] = RootRange::new(
        roots[0].root().clone(),
        feed_cursor(400),
        feed_cursor(450),
        11,
    )
    .unwrap();
    let item = DeliveryItem::<RouterDeliveryTypes>::new(
        template.id(),
        template.sender().clone(),
        template.recipient().clone(),
        None,
        body,
    )
    .unwrap();
    let projected = received_mail_push(&item, Utc::now()).unwrap().unwrap();
    assert!(matches!(
        projected.header_facts,
        PushHeaderFacts::SubscriptionActivity {
            root_count: 1,
            message_count: 18,
            ..
        }
    ));
    let activity = serde_json::to_value(projected.activity.unwrap()).unwrap();
    assert_eq!(activity["ranges"].as_array().unwrap().len(), 2);
    assert_eq!(activity["ranges"][0]["from"]["position"], 2);
    assert_eq!(activity["ranges"][0]["through"]["position"], 300);
    assert_eq!(activity["ranges"][1]["from"]["position"], 400);
    assert_eq!(activity["ranges"][1]["through"]["position"], 450);
}

#[test]
fn producer_freezes_selected_counts_and_draining_without_its_local_hold_metadata() {
    let item = follow_item(true, [7, 11], SubscriptionMode::Deliver);
    let board_sync_protocol::BoardDeliveryBody::Notification(body) = item.body() else {
        panic!("notification fixture")
    };
    let NotificationSource::FollowBatch {
        membership,
        activated_at,
        generation,
        policy,
        roots,
        ..
    } = &body.source
    else {
        panic!("follow fixture")
    };
    let selected = SubscriptionBatch {
        batch_id: item.id().as_uuid().to_string().try_into().unwrap(),
        held: true,
        held_since: Some(Utc::now()),
        draining: true,
        roots: roots
            .iter()
            .map(|root| {
                PendingRootNotice::new(
                    root.root().clone(),
                    body.pointer.project.clone(),
                    root.from(),
                    root.through(),
                    root.post_count(),
                )
                .unwrap()
            })
            .collect(),
    };
    let frozen = NotificationSource::follow_batch(
        membership.clone(),
        *activated_at,
        *generation,
        policy.clone(),
        &selected,
    )
    .unwrap();
    let wire = serde_json::to_value(frozen).unwrap();
    assert_eq!(wire["draining"], true);
    assert_eq!(wire["roots"][0]["postCount"], 7);
    assert_eq!(wire["roots"][1]["postCount"], 11);
    assert!(wire.get("held").is_none());
    assert!(wire.get("heldSince").is_none());
}

#[test]
fn recipient_keeps_the_frozen_sender_label_in_the_direct_message_header() {
    let template = follow_item(false, [1, 1], SubscriptionMode::Deliver);
    let item = DeliveryItem::<RouterDeliveryTypes>::new(
        template.id(),
        serde_json::from_value(json!({"kind":"session","endpoint":{"serviceId":"00000000-0000-4000-8000-000000000002","endpointId":"claude-local"},"sessionId":"sender-session"})).unwrap(),
        template.recipient().clone(),None,
        serde_json::from_value(json!({"kind":"message","text":"frozen text","delivery":"queue","guard":null,"senderDisplayName":"🐒 Frozen sender","replyTo":null})).unwrap(),
    ).unwrap();
    let draft = received_mail_push(&item, Utc::now()).unwrap().unwrap();
    assert!(
        matches!(draft.header_facts,PushHeaderFacts::DirectMessage {sender_display_name:Some(name)} if name.as_str()=="🐒 Frozen sender")
    );
    assert_eq!(
        draft.mode,
        Some(collaboration_protocol::MessageDelivery::Queue)
    );
    assert_eq!(draft.body.as_deref(), Some("frozen text"));
}

#[test]
fn send_producer_copies_the_exact_label_through_shared_wire_and_recipient_projection() {
    let template = follow_item(false, [1, 1], SubscriptionMode::Deliver);
    let mut draft = PushRecordDraft {
        push_id: template.id().as_uuid().to_string().try_into().unwrap(),
        kind: PushKind::DirectMessage,
        origin: PushOrigin::Session(serde_json::from_value(json!({
            "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000002","endpointId":"claude-local"},
            "sessionId":"sender-session"
        })).unwrap()),
        origin_router_ref:None,
        target:template.recipient().address.clone().try_into().unwrap(),
        mode:Some(collaboration_protocol::MessageDelivery::Queue),
        guard:Some(serde_json::from_value(json!({"serviceEpoch":"00000000-0000-4000-8000-000000000003","generation":17})).unwrap()),
        reply_to_push_id:None,
        header_facts:PushHeaderFacts::DirectMessage {sender_display_name:Some("🐒 Frozen producer".to_owned().try_into().unwrap())},
        body:Some("The producer's exact text".to_owned()),activity:None,created_at:Utc::now(),
    };
    let installation = "00000000-0000-4000-8000-000000000002"
        .to_owned()
        .try_into()
        .unwrap();
    let item =
        crate::outgoing_mail_message(&draft, &installation, template.recipient().peer).unwrap();
    let wire = serde_json::to_vec(&item).unwrap();
    let shared: fleet_sync_protocol::DeliveryItem<board_sync_protocol::BoardDeliveryTypes> =
        serde_json::from_slice(&wire).unwrap();
    let received: DeliveryItem<RouterDeliveryTypes> =
        serde_json::from_slice(&serde_json::to_vec(&shared).unwrap()).unwrap();
    let projected = received_mail_push(&received, Utc::now()).unwrap().unwrap();
    assert!(
        matches!(&projected.header_facts,PushHeaderFacts::DirectMessage {sender_display_name:Some(name)} if name.as_str()=="🐒 Frozen producer")
    );
    assert_eq!(projected.header_facts, draft.header_facts);
    assert_eq!(projected.mode, draft.mode);
    assert_eq!(projected.guard, draft.guard);
    assert_eq!(projected.body, draft.body);
    draft.header_facts = PushHeaderFacts::DirectMessage {
        sender_display_name: Some("🐒 Changed producer".to_owned().try_into().unwrap()),
    };
    assert_ne!(
        crate::outgoing_mail_message(&draft, &installation, template.recipient().peer)
            .unwrap()
            .digest(),
        item.digest()
    );
    draft.origin = PushOrigin::OwnerUnverified;
    let human =
        crate::outgoing_mail_message(&draft, &installation, template.recipient().peer).unwrap();
    assert!(
        matches!(human.sender(),MailSender::HumanUser {installation:origin} if origin==&installation)
    );
    assert_ne!(installation, human.recipient().address.endpoint.service_id);
}
