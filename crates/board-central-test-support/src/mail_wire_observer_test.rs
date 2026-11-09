use super::*;
use fleet_protocol::{
    CallId, CancelBody, ClientAnswer, DeliveryCancel, DeliveryEnvelope, DeliveryRefusal, Recipient,
    ServerCallAnswer,
};
use serde_json::json;

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; setup propagates errors."
)]
fn push_wire_observation_keeps_item_frame_and_capacity_refusal() -> FixtureResult {
    // Arrange real protocol encodings, including a different frame after the refusal.
    let observer = MailWireObserver::default();
    let observation = observer.begin(None)?;
    let frame: FrameId = serde_json::from_value(json!("019f0000-0000-7000-8000-000000001101"))?;
    let other_frame: FrameId =
        serde_json::from_value(json!("019f0000-0000-7000-8000-000000001102"))?;
    let item: ItemId = serde_json::from_value(json!("019f0000-0000-7000-8000-000000001103"))?;
    let address = serde_json::from_value(json!({
        "endpoint":{"serviceId":"019f0000-0000-7000-8000-000000001104","endpointId":"codex-local"},
        "sessionId":"capacity-recipient"
    }))?;
    let peer = serde_json::from_value(json!("019f0000-0000-7000-8000-000000001105"))?;
    let cancel = DeliveryCancel::<BoardDeliveryTypes>::new(
        item,
        serde_json::from_value(
            json!({"kind":"humanUser","installation":"019f0000-0000-7000-8000-000000001104"}),
        )?,
        Recipient { peer, address },
        None,
        CancelBody {
            target: serde_json::from_value(json!("019f0000-0000-7000-8000-000000001106"))?,
        },
    )?;
    let call: CallId = serde_json::from_value(json!({
        "operation":"019f0000-0000-7000-8000-000000001107","step":"push"
    }))?;
    let pushed = ServerFrame::<BoardConsumerProtocol>::Request {
        frame,
        body: ServerRequest::Crate(ServerCall::Push {
            call,
            submitter: fleet_protocol::Submitter::Machine(peer),
            item: DeliveryEnvelope::Cancel(cancel),
        }),
    };

    // Act through the same byte readers the Access WebSocket bridge uses.
    assert!(!observer.server_frame(&serde_json::to_vec(&pushed)?)?);
    observer.client_frame(&serde_json::to_vec(
        &ClientFrame::<BoardConsumerProtocol>::Answer {
            frame,
            body: ClientAnswer::Crate(ServerCallAnswer::Pushed(PushResult::Refused {
                reason: DeliveryRefusal::MailboxFull,
            })),
        },
    )?)?;
    observer.client_frame(&serde_json::to_vec(
        &ClientFrame::<BoardConsumerProtocol>::Answer {
            frame: other_frame,
            body: ClientAnswer::Crate(ServerCallAnswer::Pushed(PushResult::Accepted {
                accepted_at: runtime_primitives::SyncTime::from_unix_millis(42),
            })),
        },
    )?)?;

    // Assert the actual refusal stays a refusal, with its original item and frame.
    let events = observation.events();
    assert_eq!(
        events.len(),
        3,
        "actual Push traffic disappeared: {events:?}"
    );
    assert!(
        matches!(events[0], MailWireEvent::PushSent { frame: actual, item: actual_item } if actual == frame && actual_item == item)
    );
    assert!(
        matches!(events[1], MailWireEvent::PushAnswered { frame: actual, result: PushResult::Refused { reason: DeliveryRefusal::MailboxFull } } if actual == frame)
    );
    assert!(
        matches!(events[2], MailWireEvent::PushAnswered { frame: actual, result: PushResult::Accepted { .. } } if actual == other_frame)
    );
    Ok(())
}
