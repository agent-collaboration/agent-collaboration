//! A session's generation at the sink ends with its Closed (D1 R14): nothing
//! published for it afterwards reaches the hub, and only a new load or
//! resume generation publishes again.

use super::*;
use collaboration_contracts::SessionEventHub;
use collaboration_runtime::{ProviderOperationStore, ProviderSessionEventHub};
use message_board::{EndpointId, ServiceId};
use session_event_model::{CapabilityReport, SessionState};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Oracle: the Lead's terminal-per-generation rule. After a generation's
/// Closed, the client's own events and that generation's activation are
/// dropped; once a newer generation opens, the old one stays ended; the new
/// generation's activation is the only thing the hub sees after Closed.
#[tokio::test]
async fn nothing_reaches_the_hub_for_a_generation_after_its_closed() {
    // Arrange: a session published idle in its first generation.
    let root = tempfile::tempdir().expect("temporary store");
    let store = ProviderOperationStore::open(&root.path().join("operations.sqlite"))
        .await
        .expect("provider store");
    let hub = Arc::new(ProviderSessionEventHub::new(Arc::new(Mutex::new(store))));
    let endpoint = SessionEndpointRef {
        service_id: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())
            .expect("service"),
        endpoint_id: EndpointId::try_from("fixture-provider".to_owned()).expect("endpoint"),
    };
    let session = SessionRef {
        endpoint: endpoint.clone(),
        session_id: SessionId::try_from("session-one".to_owned()).expect("session"),
    };
    let sink = HubSessionEventSink::new(Arc::clone(&hub), endpoint);
    let first = sink.current_generation("session-one");
    let activated = sink
        .publish_activation("session-one", first, CapabilityReport::default())
        .await
        .expect("activation");
    let mut before_close = hub.attach(session.clone()).await.expect("attach");

    // Act: the generation closes; the client's events and the generation's
    // own activation arrive late; a newer generation opens, the old one tries
    // again, and the new one activates.
    let closed = sink
        .publish_closed("session-one", first)
        .expect("Closed is enqueued");
    loop {
        let event = before_close.receiver.recv().await.expect("hub event");
        if matches!(
            event.event,
            SessionEvent::StateChanged {
                state: SessionState::Closed
            }
        ) {
            break;
        }
    }
    let mut after_close = hub.attach(session.clone()).await.expect("attach");
    let late_client_event = sink.publish(
        "session-one",
        SessionEvent::StateChanged {
            state: SessionState::Running,
        },
    );
    let late_activation = sink
        .publish_activation("session-one", first, CapabilityReport::default())
        .await
        .expect("late activation");
    let second = sink.open_generation("session-one");
    let stale_activation = sink
        .publish_activation("session-one", first, CapabilityReport::default())
        .await
        .expect("stale activation");
    let stale_close = sink
        .publish_closed("session-one", first)
        .expect("stale close");
    let new_activation = sink
        .publish_activation("session-one", second, CapabilityReport::default())
        .await
        .expect("new activation");
    let mut seen_after_close = after_close.snapshot.clone();
    while let Ok(event) = after_close.receiver.try_recv() {
        seen_after_close.push(event);
    }
    let state = hub.state(session).await;
    sink.shutdown().await.expect("sink shutdown");

    // Assert
    assert_eq!(activated, GenerationPublication::Published);
    assert_eq!(closed, GenerationPublication::Published);
    assert!(late_client_event.is_ok(), "a late event is dropped quietly");
    assert_eq!(late_activation, GenerationPublication::Ended);
    assert_eq!(stale_activation, GenerationPublication::Ended);
    assert_eq!(
        stale_close,
        GenerationPublication::Ended,
        "an old close doesn't end the new generation"
    );
    assert_eq!(new_activation, GenerationPublication::Published);
    let kinds: Vec<&SessionEvent> = seen_after_close.iter().map(|event| &event.event).collect();
    assert!(
        matches!(
            kinds.as_slice(),
            [
                SessionEvent::CapabilitiesChanged { .. },
                SessionEvent::StateChanged {
                    state: SessionState::Idle
                }
            ]
        ),
        "after Closed the hub saw only the new generation's activation: {kinds:?}"
    );
    assert!(matches!(state, Ok(SessionState::Idle)), "{state:?}");
}
