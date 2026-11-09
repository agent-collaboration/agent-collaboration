#![allow(clippy::unwrap_used)]
use crate::{rule_test_support::*, *};
use fleet_protocol::*;
use serde_json::json;

#[tokio::test]
async fn default_session_admission_does_not_negotiate_dormant_capabilities() {
    let mut scope = SqlScope::new().await;
    let connection = binding(2);
    attach_offer(
        &mut scope,
        &connection,
        Some(FrameCapabilities::SUPPORTED),
        FrameCapabilities::default(),
    )
    .await;
    assert!(matches!(
        scope.registration(&connection.connection),
        Some(ConnectionRegistration::Attached {
            capabilities: None,
            ..
        })
    ));
}

#[tokio::test]
async fn an_offer_cannot_enable_a_capability_the_receiver_does_not_honor() {
    for honored in [
        FrameCapabilities {
            batches: false,
            notices_applied: true,
        },
        FrameCapabilities {
            batches: true,
            notices_applied: false,
        },
    ] {
        let mut scope = SqlScope::new().await;
        let connection = binding(2);
        attach_offer(
            &mut scope,
            &connection,
            Some(FrameCapabilities::SUPPORTED),
            honored,
        )
        .await;
        assert!(
            matches!(scope.registration(&connection.connection), Some(ConnectionRegistration::Attached { capabilities: Some(actual), .. }) if actual == honored)
        );
        let bytes = if honored.batches {
            serde_json::to_vec(&ClientFrame::<NoConsumer>::Request {
                frame: FrameId::new(session(5).as_uuid()).unwrap(),
                body: OperationRequest::NoticesApplied {
                    through: NoticePosition::new(40),
                },
            })
            .unwrap()
        } else {
            batch_bytes()
        };
        let effects =
            frame_received::<_, NoConsumer>(&mut scope, &connection, &bytes, now(2), honored)
                .await
                .unwrap();
        assert!(effects.iter().any(|effect| matches!(
            effect,
            SessionEffect::Close {
                reason: CloseReason::MalformedFrame,
                ..
            }
        )));
    }
}

async fn attach_offer(
    scope: &mut SqlScope,
    connection: &ConnectionBinding,
    offer: Option<FrameCapabilities>,
    honored: FrameCapabilities,
) {
    connection_opened::<_, NoConsumer>(scope, connection, now(1))
        .await
        .unwrap();
    let hello: ClientFrame = ClientFrame::Hello {
        session_id: session(3),
        schema_support: SchemaSupport { max_version: 1 },
        capabilities: offer,
    };
    frame_received::<_, NoConsumer>(
        scope,
        connection,
        &serde_json::to_vec(&hello).unwrap(),
        now(1),
        honored,
    )
    .await
    .unwrap();
    for input in std::mem::take(&mut scope.publications) {
        complete_schema_publication(scope, &input, now(1))
            .await
            .unwrap();
    }
}

fn batch_bytes() -> Vec<u8> {
    serde_json::to_vec(&json!({"kind":"requestBatch","value":[{
        "sendId":"019eef00-0000-7000-8000-000000000005",
        "body":{"kind":"consumer","value":null}
    }]}))
    .unwrap()
}

#[tokio::test]
async fn negotiated_batches_reach_typed_admission_and_legacy_connections_refuse_them() {
    for offer in [None, Some(FrameCapabilities::SUPPORTED)] {
        let mut scope = SqlScope::new().await;
        let connection = binding(2);
        attach_offer(&mut scope, &connection, offer, FrameCapabilities::SUPPORTED).await;
        assert!(
            matches!(scope.registration(&connection.connection), Some(ConnectionRegistration::Attached { capabilities, .. }) if capabilities == offer)
        );
        let effects = frame_received::<_, NoConsumer>(
            &mut scope,
            &connection,
            &batch_bytes(),
            now(2),
            fleet_protocol::FrameCapabilities::default(),
        )
        .await
        .unwrap();
        if offer.is_some() {
            assert!(
                matches!(effects.as_slice(), [SessionEffect::RequestBatch { batch, .. }] if batch.items().len() == 1)
            );
        } else {
            assert!(effects.iter().any(|effect| matches!(
                effect,
                SessionEffect::Close {
                    reason: CloseReason::MalformedFrame,
                    ..
                }
            )));
        }
    }
}

#[tokio::test]
async fn notice_acknowledgments_require_their_own_capability_and_do_not_execute() {
    for notices_applied in [false, true] {
        let mut scope = SqlScope::new().await;
        let connection = binding(2);
        attach_offer(
            &mut scope,
            &connection,
            Some(FrameCapabilities {
                batches: true,
                notices_applied,
            }),
            FrameCapabilities::SUPPORTED,
        )
        .await;
        let request: ClientFrame = ClientFrame::Request {
            frame: FrameId::new(session(5).as_uuid()).unwrap(),
            body: OperationRequest::NoticesApplied {
                through: NoticePosition::new(40),
            },
        };
        let effects = frame_received::<_, NoConsumer>(
            &mut scope,
            &connection,
            &serde_json::to_vec(&request).unwrap(),
            now(2),
            fleet_protocol::FrameCapabilities::default(),
        )
        .await
        .unwrap();
        if notices_applied {
            assert!(
                matches!(effects.as_slice(), [SessionEffect::NoticesApplied { through, .. }] if through.value() == 40)
            );
        } else {
            assert!(effects.iter().any(|effect| matches!(
                effect,
                SessionEffect::Close {
                    reason: CloseReason::MalformedFrame,
                    ..
                }
            )));
        }
    }
}

#[tokio::test]
async fn oversized_raw_batch_and_pre_hello_batch_close_without_routing() {
    let mut scope = SqlScope::new().await;
    let connection = binding(2);
    connection_opened::<_, NoConsumer>(&mut scope, &connection, now(1))
        .await
        .unwrap();
    let effects = frame_received::<_, NoConsumer>(
        &mut scope,
        &connection,
        &batch_bytes(),
        now(2),
        fleet_protocol::FrameCapabilities::default(),
    )
    .await
    .unwrap();
    assert!(effects.iter().any(|effect| matches!(
        effect,
        SessionEffect::Close {
            reason: CloseReason::MalformedFrame,
            ..
        }
    )));
    let mut scope = SqlScope::new().await;
    attach_offer(
        &mut scope,
        &connection,
        Some(FrameCapabilities::SUPPORTED),
        FrameCapabilities::SUPPORTED,
    )
    .await;
    let mut oversized = batch_bytes();
    oversized.extend(std::iter::repeat_n(b' ', MAX_BATCH_BYTES));
    let effects = frame_received::<_, NoConsumer>(
        &mut scope,
        &connection,
        &oversized,
        now(2),
        fleet_protocol::FrameCapabilities::default(),
    )
    .await
    .unwrap();
    assert!(effects.iter().any(|effect| matches!(
        effect,
        SessionEffect::Close {
            reason: CloseReason::MalformedFrame,
            ..
        }
    )));
}
