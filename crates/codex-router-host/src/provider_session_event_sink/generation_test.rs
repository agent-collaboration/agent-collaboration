//! A session's generation at the sink ends with its Closed (D1 R14): nothing
//! published for it afterwards reaches the hub, and a producer from an earlier
//! run can't publish into a later one.

use super::*;
use collaboration_contracts::SessionEventHub;
use collaboration_runtime::{ProviderOperationStore, ProviderSessionEventHub};
use message_board::{EndpointId, ServiceId};
use session_event_model::{CapabilityReport, SessionSettings, SessionState};
use std::sync::Arc;
use tokio::sync::Mutex;

struct SinkFixture {
    _root: tempfile::TempDir,
    hub: Arc<ProviderSessionEventHub>,
    session: SessionRef,
    sink: HubSessionEventSink,
}

async fn sink_fixture() -> SinkFixture {
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
    SinkFixture {
        _root: root,
        hub,
        session,
        sink,
    }
}

/// Waits, without a timer, until the hub has applied the session's Closed.
async fn until_hub_closed(attachment: &mut collaboration_contracts::SessionEventAttachment) {
    loop {
        let event = attachment.receiver.recv().await.expect("hub event");
        if matches!(
            event.event,
            SessionEvent::StateChanged {
                state: SessionState::Closed
            }
        ) {
            return;
        }
    }
}

fn drained(mut attachment: collaboration_contracts::SessionEventAttachment) -> Vec<SessionEvent> {
    let mut events: Vec<SessionEvent> = attachment
        .snapshot
        .iter()
        .map(|event| event.event.clone())
        .collect();
    while let Ok(event) = attachment.receiver.try_recv() {
        events.push(event.event);
    }
    events
}

fn settings(model: &str) -> SessionSettings {
    SessionSettings {
        mode: None,
        model: Some(model.to_owned()),
        effort: None,
        config: Vec::new(),
    }
}

/// Oracle: the Lead's terminal-per-generation rule. After a run's Closed, its
/// producers and its activation are dropped; once the connection admits a
/// newer run, the old one's activation and close stay ended; the new run's
/// activation is the only thing the hub sees after Closed.
#[tokio::test]
async fn nothing_reaches_the_hub_for_a_generation_after_its_closed() {
    // Arrange: a run the connection admitted, published idle.
    let fixture = sink_fixture().await;
    let sink = &fixture.sink;
    let first = sink.open_generation("session-one");
    let activated = sink
        .publish_activation("session-one", first, CapabilityReport::default())
        .await
        .expect("activation");
    let mut before_close = fixture
        .hub
        .attach(fixture.session.clone())
        .await
        .expect("attach");

    // Act: the run closes; its producer and activation arrive late; a newer
    // run is admitted, the old one tries again, and the new one activates.
    let closed = sink
        .publish_closed("session-one", Some(first))
        .expect("Closed is enqueued");
    until_hub_closed(&mut before_close).await;
    let after_close = fixture
        .hub
        .attach(fixture.session.clone())
        .await
        .expect("attach");
    let late_producer = sink
        .publish_in(
            "session-one",
            first,
            SessionEvent::StateChanged {
                state: SessionState::Running,
            },
        )
        .expect("late producer");
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
        .publish_closed("session-one", Some(first))
        .expect("stale close");
    let new_activation = sink
        .publish_activation("session-one", second, CapabilityReport::default())
        .await
        .expect("new activation");
    let seen_after_close = drained(after_close);
    let state = fixture.hub.state(fixture.session.clone()).await;
    sink.shutdown().await.expect("sink shutdown");

    // Assert
    assert_eq!(activated, GenerationPublication::Published);
    assert_eq!(closed, GenerationPublication::Published);
    assert_eq!(late_producer, GenerationPublication::Ended);
    assert_eq!(late_activation, GenerationPublication::Ended);
    assert_eq!(stale_activation, GenerationPublication::Ended);
    assert_eq!(
        stale_close,
        GenerationPublication::Ended,
        "an old close doesn't end the new run"
    );
    assert_eq!(new_activation, GenerationPublication::Published);
    assert!(
        matches!(
            seen_after_close.as_slice(),
            [
                SessionEvent::CapabilitiesChanged { .. },
                SessionEvent::StateChanged {
                    state: SessionState::Idle
                }
            ]
        ),
        "after Closed the hub saw only the new run's activation: {seen_after_close:?}"
    );
    assert!(matches!(state, Ok(SessionState::Idle)), "{state:?}");
}

/// Oracle: the Reviewer's G2 and the Lead's order. Run A closes; the
/// connection admits a resume, run B, which publishes its fresh settings;
/// then a producer from A that was delayed publishes its own settings with
/// A's generation. A's publication is dropped, and B's resume carries B's
/// settings into its new history ahead of its activation.
#[tokio::test]
async fn a_producer_from_an_earlier_run_never_publishes_into_a_later_one() {
    // Arrange: run A, admitted, active and closed.
    let fixture = sink_fixture().await;
    let sink = &fixture.sink;
    let run_a = sink.open_generation("session-one");
    sink.publish_activation("session-one", run_a, CapabilityReport::default())
        .await
        .expect("A active");
    let mut before_close = fixture
        .hub
        .attach(fixture.session.clone())
        .await
        .expect("attach");
    sink.publish_closed("session-one", Some(run_a))
        .expect("Closed is enqueued");
    until_hub_closed(&mut before_close).await;

    // Act: run B is admitted and publishes its settings; A's delayed
    // producer publishes; B's resume invalidates history and activates.
    let run_b = sink.open_generation("session-one");
    let b_settings = sink
        .publish_in(
            "session-one",
            run_b,
            SessionEvent::SettingsChanged {
                settings: settings("model-b"),
            },
        )
        .expect("B's settings");
    let a_late_settings = sink
        .publish_in(
            "session-one",
            run_a,
            SessionEvent::SettingsChanged {
                settings: settings("model-a"),
            },
        )
        .expect("A's late settings");
    let invalidated = sink
        .begin_history_unavailable("session-one", run_b)
        .await
        .expect("B's history invalidation");
    let activated = sink
        .publish_activation("session-one", run_b, CapabilityReport::default())
        .await
        .expect("B's activation");
    let history = drained(
        fixture
            .hub
            .attach(fixture.session.clone())
            .await
            .expect("attach"),
    );
    sink.shutdown().await.expect("sink shutdown");

    // Assert
    assert_eq!(b_settings, GenerationPublication::Published);
    assert_eq!(
        a_late_settings,
        GenerationPublication::Ended,
        "A's producer can't publish into B"
    );
    assert_eq!(invalidated, GenerationPublication::Published);
    assert_eq!(activated, GenerationPublication::Published);
    assert!(
        matches!(
            history.as_slice(),
            [
                SessionEvent::SettingsChanged { settings },
                SessionEvent::CapabilitiesChanged { .. },
                SessionEvent::StateChanged {
                    state: SessionState::Idle
                }
            ] if settings.model.as_deref() == Some("model-b")
        ),
        "B's history holds B's settings, then its activation: {history:?}"
    );
}
