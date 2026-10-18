#![allow(clippy::expect_used)]
use collaboration_replication_protocol::*;
use serde::{Serialize, de::DeserializeOwned};
fn roundtrip<TValue: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: TValue) {
    let wire = serde_json::to_value(&value).expect("wire");
    assert_eq!(
        serde_json::from_value::<TValue>(wire).expect("decode"),
        value
    );
}
#[test]
fn forwarding_wire_shapes_are_closed_and_preserve_opaque_board_envelopes() {
    let project = ProjectId::new("019f0000-0000-7000-8000-000000000001").expect("project");
    let origin = InstallationId::new("019f0000-0000-7000-8000-000000000002").expect("origin");
    let forward_id = ForwardId::new("019f0000-0000-7000-8000-000000000003").expect("forward");
    let payload = BoardPayload::new(
        serde_json::json!({"request_id":"provider-owned-opaque","post":"hello","large":i64::MAX}),
    );
    for kind in [
        ForwardedPostKind::ThreadCreate,
        ForwardedPostKind::ThreadPost,
    ] {
        let post = ForwardedPost {
            kind,
            body: payload.clone(),
        };
        roundtrip(SessionRequest::Forward {
            project: project.clone(),
            post: post.clone(),
        });
        roundtrip(CentralFrame::Forwarded {
            forward_id: forward_id.clone(),
            origin: origin.clone(),
            origin_label: "machine label".into(),
            project: project.clone(),
            post,
        });
    }
    roundtrip(MachineFrame::ForwardAnswer {
        forward_id,
        reply: payload.clone(),
    });
    for outcome in [
        ForwardOutcome::Answered { reply: payload },
        ForwardOutcome::SenderHolds,
        ForwardOutcome::NotHeld,
        ForwardOutcome::HolderUnreachable {
            last_seen: chrono::DateTime::from_timestamp(1, 0).expect("time"),
        },
        ForwardOutcome::Unanswered,
        ForwardOutcome::Overloaded,
    ] {
        roundtrip(SessionAnswer::Forwarded(outcome));
    }
    assert!(
        serde_json::from_value::<ForwardedPost>(serde_json::json!({"kind":"taskCreate","body":{}}))
            .is_err()
    );
    roundtrip(SessionRefusal::Archived);
    assert!(
        serde_json::from_value::<SessionRefusal>(serde_json::json!({"kind":"unknownRefusal"}))
            .is_err()
    );
    assert!(ForwardId::new("not-a-v7-id").is_err());
}
