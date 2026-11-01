//! A listing has its own kind; its body must never impersonate a board intent.
use collaboration_protocol::{PushKind, PushRecordDraft};

#[test]
fn notification_listing_decodes_as_a_distinct_router_push_with_a_body() {
    let id = uuid::Uuid::now_v7().to_string();
    let target = serde_json::json!({
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},
        "sessionId":"notification-recipient"
    });
    let origin = collaboration_protocol::RouterOriginRef::NotificationListing {
        target: serde_json::from_value(target.clone()).unwrap(),
        listing_id: id.clone().try_into().unwrap(),
    }
    .canonical_string()
    .unwrap();
    let draft = serde_json::json!({
        "pushId":id,
        "kind":"notification-listing",
        "origin":{"router":"notification-listing"},
        "originRouterRef":origin,
        "target":target,
        "replyToPushId":null,
        "headerFacts":{"kind":"notificationListing","itemCount":51},
        "body":"50 notification references; 1 more through notification_members",
        "activity":null,
        "createdAt":"2026-11-01T00:00:00Z"
    });
    let decoded: PushRecordDraft = serde_json::from_value(draft)
        .expect("the published listing contract must decode a Router listing");
    let record = decoded
        .into_pending()
        .expect("a listing carries its own body");
    assert_eq!(
        serde_json::to_value(record.kind).unwrap(),
        "notification-listing"
    );
    assert_ne!(record.kind, PushKind::BoardNotification);
    assert_ne!(record.kind, PushKind::DirectMessage);
}
