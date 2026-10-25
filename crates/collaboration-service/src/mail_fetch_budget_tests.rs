//! Measure accepted mail in the exact envelope the relay budgets, including JSON escaping.
use crate::RouterDeliveryTypes;
use board_sync_protocol::BoardDeliveryBody;
use collaboration_mail_protocol::{
    FollowDeliveryPolicy, GenerationGuard, MailDeliveryMode, MailSender, MessageBody,
    NotificationBody, NotificationSource, RecordPointer, RootRange,
};
use fleet_sync_protocol::{
    DeliveryEnvelope, DeliveryItem, DeliverySeq, DeliveryTypes, FetchEntry, ItemId, PeerId,
    Recipient, unit_budget::TARGET_UNIT_BYTES,
};
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

pub(crate) fn largest_message(peer: PeerId) -> DeliveryItem<RouterDeliveryTypes> {
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

pub(crate) fn largest_notification(peer: PeerId) -> DeliveryItem<RouterDeliveryTypes> {
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

#[test]
fn maximal_accepted_message_and_twenty_root_notification_fit_the_fetch_unit() {
    let peer = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    for (kind, item) in [
        ("message", largest_message(peer)),
        ("notification", largest_notification(peer)),
    ] {
        let entry = FetchEntry::Item {
            seq: DeliverySeq::new(u64::MAX),
            item: DeliveryEnvelope::Item(item),
        };
        let encoded = serde_json::to_vec(&entry).unwrap();
        // Real decoding revalidates the item digest and each nested domain value.
        let decoded: FetchEntry<RouterDeliveryTypes> = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), encoded);
        println!(
            "maximal {kind} FetchEntry::Item bytes={}, fetch unit={TARGET_UNIT_BYTES}",
            encoded.len()
        );
        assert!(
            encoded.len() <= TARGET_UNIT_BYTES,
            "maximal {kind} exceeds the configured unit"
        );
    }
}
