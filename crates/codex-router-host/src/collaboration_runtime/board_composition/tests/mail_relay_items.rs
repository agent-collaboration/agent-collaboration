//! Measure accepted mail in the exact envelope the relay budgets, including JSON escaping.
use board_sync_protocol::BoardDeliveryBody;
use collaboration_contracts::RouterDeliveryTypes;
use collaboration_mail_protocol::{
    FollowDeliveryPolicy, GenerationGuard, MailDeliveryMode, MailSender, MessageBody,
    NotificationBody, NotificationSource, RecordPointer, RootRange,
};
use fleet_sync_protocol::{DeliveryItem, DeliveryTypes, ItemId, PeerId, Recipient};
use message_board::{
    BatchTiming, FeedCursor, Identity, MAX_SUBSCRIPTION_CAP_SECONDS,
    MAX_SUBSCRIPTION_LIFETIME_SECONDS, MAX_SUBSCRIPTION_NOTICE_ROOTS,
    MAX_SUBSCRIPTION_QUIET_SECONDS, ProjectId, RecordKind, RecordRef, SessionRef,
    SubscriptionGeneration, SubscriptionLifetime, SubscriptionMode, WhenIdle,
};
use runtime_primitives::SyncTime;
use serde_json::json;

fn largest_session() -> SessionRef {
    serde_json::from_value(json!({
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"a".repeat(64)},
        "sessionId":"\u{0001}".repeat(4096)
    }))
    .unwrap()
}

pub(super) fn largest_message(peer: PeerId) -> DeliveryItem<RouterDeliveryTypes> {
    let sender = MailSender::Session(largest_session());
    assert!(
        serde_json::to_vec(&sender).unwrap().len()
            <= RouterDeliveryTypes::MAX_SENDER_BYTES as usize
    );
    let address = largest_session();
    assert!(
        serde_json::to_vec(&address).unwrap().len()
            <= RouterDeliveryTypes::MAX_ADDRESS_BYTES as usize
    );
    DeliveryItem::new(
        ItemId::new(uuid::Uuid::now_v7()).unwrap(),
        sender,
        Recipient { peer, address },
        Some(SyncTime::from_unix_millis(i64::MAX)),
        BoardDeliveryBody::Message(MessageBody {
            // C0 controls other than tab/newline are invalid. Every legal byte
            // here needs two JSON bytes, as do quote and backslash (decision 683).
            text: "\t".repeat(65_536).try_into().unwrap(),
            delivery: MailDeliveryMode::Steer,
            guard: Some(
                GenerationGuard::new(
                    "00000000-0000-4000-8000-000000000001"
                        .to_owned()
                        .try_into()
                        .unwrap(),
                    9_007_199_254_740_991,
                )
                .unwrap(),
            ),
            sender_display_name: Some("\u{10ffff}".repeat(120).try_into().unwrap()),
            reply_to: Some(
                "01900000-0000-7000-8000-000000000725"
                    .to_owned()
                    .try_into()
                    .unwrap(),
            ),
        }),
    )
    .unwrap()
}

pub(super) fn largest_notification(peer: PeerId) -> DeliveryItem<RouterDeliveryTypes> {
    let maximum = i64::MAX as u64;
    let cursor = FeedCursor {
        epoch: maximum.try_into().unwrap(),
        position: maximum.try_into().unwrap(),
    };
    let address = largest_session();
    let project = ProjectId::generate();
    let roots = (0..MAX_SUBSCRIPTION_NOTICE_ROOTS)
        .map(|_| {
            RootRange::new(
                message_board::ThreadId::generate(),
                cursor,
                cursor,
                u64::MAX,
            )
            .unwrap()
        })
        .collect();
    DeliveryItem::new(
        ItemId::new(uuid::Uuid::now_v7()).unwrap(),
        MailSender::Project {
            project: project.clone(),
        },
        Recipient {
            peer,
            address: address.clone(),
        },
        Some(SyncTime::from_unix_millis(i64::MAX)),
        BoardDeliveryBody::Notification(NotificationBody {
            excerpt: "\0".repeat(280).try_into().unwrap(),
            pointer: RecordPointer {
                project,
                subject: RecordRef {
                    kind: RecordKind::StandDownAcknowledgement,
                    id: message_board::RecordId::generate(),
                },
                position: cursor,
            },
            source: NotificationSource::FollowBatch {
                membership: uuid::Uuid::now_v7().to_string().try_into().unwrap(),
                activated_at: cursor,
                generation: SubscriptionGeneration::new(u64::MAX).unwrap(),
                roots,
                policy: FollowDeliveryPolicy::new(
                    &Identity::Session { session: address },
                    SubscriptionMode::Deliver,
                    WhenIdle::Hold,
                    BatchTiming::new(MAX_SUBSCRIPTION_QUIET_SECONDS, MAX_SUBSCRIPTION_CAP_SECONDS)
                        .unwrap(),
                    SubscriptionLifetime::new(MAX_SUBSCRIPTION_LIFETIME_SECONDS).unwrap(),
                )
                .unwrap(),
                // false is the longer JSON spelling.
                draining: false,
            },
        }),
    )
    .unwrap()
}

pub(super) fn ordinary_message(peer: PeerId) -> DeliveryItem<RouterDeliveryTypes> {
    DeliveryItem::new(
        ItemId::new(uuid::Uuid::now_v7()).unwrap(),
        MailSender::HumanUser { installation: "00000000-0000-4000-8000-000000000001".to_owned().try_into().unwrap() },
        Recipient { peer, address: serde_json::from_value(json!({
            "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},
            "sessionId":"relay-recipient"
        })).unwrap() },
        None,
        BoardDeliveryBody::Message(MessageBody {
            text: "actual relay round trip".to_owned().try_into().unwrap(),
            delivery: MailDeliveryMode::Auto, guard: None, sender_display_name: None, reply_to: None,
        }),
    ).unwrap()
}

pub(super) fn ordinary_notification(peer: PeerId) -> DeliveryItem<RouterDeliveryTypes> {
    ordinary_project_notification(peer, ProjectId::generate())
}

pub(super) fn ordinary_project_notification(
    peer: PeerId,
    project: ProjectId,
) -> DeliveryItem<RouterDeliveryTypes> {
    let recipient = ordinary_message(peer).recipient().clone();
    DeliveryItem::new(
        ItemId::new(uuid::Uuid::now_v7()).unwrap(),
        MailSender::Project {
            project: project.clone(),
        },
        recipient,
        None,
        BoardDeliveryBody::Notification(NotificationBody {
            excerpt: "a frozen notification".to_owned().try_into().unwrap(),
            pointer: RecordPointer {
                project,
                subject: RecordRef {
                    kind: RecordKind::Post,
                    id: message_board::RecordId::generate(),
                },
                position: FeedCursor {
                    epoch: 2.try_into().unwrap(),
                    position: 1.try_into().unwrap(),
                },
            },
            source: NotificationSource::Intent {
                intent: message_board::NotifyIntentId::generate(),
            },
        }),
    )
    .unwrap()
}
