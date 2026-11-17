//! A session's run at the sink ends with its terminal fact (D1 R14, step 6):
//! nothing published for it afterwards reaches the hub, and a producer from
//! an earlier run can't publish into a later one. The sink asks the
//! connection's session runs; here a scripted run stands in for the
//! session's cell, whose own rules acp-client-runtime's session_run_claims
//! tests prove.

use super::*;
use collaboration_contracts::SessionEventHub;
use collaboration_runtime::{ProviderOperationStore, ProviderSessionEventHub};
use message_board::{EndpointId, ServiceId};
use session_event_model::{CapabilityReport, SessionSettings, SessionState};
use std::sync::Arc;
use tokio::sync::Mutex;

/// The one session's run, as its cell would hold it: the latest run and
/// whether it is still running.
#[derive(Default)]
struct ScriptedRuns(std::sync::Mutex<(u64, bool)>);

impl ScriptedRuns {
    /// The connection admits a new run.
    fn start(&self) -> SessionGeneration {
        let mut run = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        run.0 += 1;
        run.1 = true;
        SessionGeneration::from_number(run.0)
    }

    /// The run ends (its close is done), before its Closed is published.
    fn end(&self) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .1 = false;
    }
}

impl SessionRuns for ScriptedRuns {
    fn publish_if(
        &self,
        _session_id: &str,
        claim: RunClaim,
        enqueue: &mut dyn FnMut() -> Result<(), EventSinkClosed>,
    ) -> Result<GenerationPublication, EventSinkClosed> {
        let run = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (latest, running) = *run;
        let holds = match claim {
            RunClaim::Current => running || latest == 0,
            RunClaim::Exact(claimed) => running && claimed.number() == latest,
            RunClaim::Terminal(claimed) => !running && claimed.number() == latest,
        };
        if holds {
            enqueue().map(|()| GenerationPublication::Published)
        } else {
            Ok(GenerationPublication::Ended)
        }
    }

    fn current_generation(&self, _session_id: &str) -> SessionGeneration {
        SessionGeneration::from_number(
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0,
        )
    }
}

struct SinkFixture {
    _root: tempfile::TempDir,
    hub: Arc<ProviderSessionEventHub>,
    session: SessionRef,
    sink: HubSessionEventSink,
    runs: Arc<ScriptedRuns>,
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
    let runs = Arc::new(ScriptedRuns::default());
    sink.attach_session_runs(Arc::clone(&runs) as Arc<dyn SessionRuns>);
    SinkFixture {
        _root: root,
        hub,
        session,
        sink,
        runs,
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
    let first = fixture.runs.start();
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
    fixture.runs.end();
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
    let second = fixture.runs.start();
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
    let run_a = fixture.runs.start();
    sink.publish_activation("session-one", run_a, CapabilityReport::default())
        .await
        .expect("A active");
    let mut before_close = fixture
        .hub
        .attach(fixture.session.clone())
        .await
        .expect("attach");
    fixture.runs.end();
    sink.publish_closed("session-one", Some(run_a))
        .expect("Closed is enqueued");
    until_hub_closed(&mut before_close).await;

    // Act: run B is admitted and publishes its settings; A's delayed
    // producer publishes; B's resume invalidates history and activates.
    let run_b = fixture.runs.start();
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

/// Oracle: the Lead's step 6 conditions at the hub: run 1's events all land
/// before its Closed; run 2, whose actor exits on its own, ends with its
/// Unloaded as its terminal fact, and a late event of run 2 is dropped
/// (M4: once a run has ended, nothing of it is published).
#[tokio::test]
async fn a_runs_events_precede_its_terminal_fact_and_unloaded_ends_a_run() {
    // Arrange
    let fixture = sink_fixture().await;
    let sink = &fixture.sink;
    let running = || SessionEvent::StateChanged {
        state: SessionState::Running,
    };

    // Act: run 1 publishes, then closes.
    let run_one = fixture.runs.start();
    sink.publish_activation("session-one", run_one, CapabilityReport::default())
        .await
        .expect("run 1 active");
    let mut watching = fixture
        .hub
        .attach(fixture.session.clone())
        .await
        .expect("attach");
    sink.publish_in("session-one", run_one, running())
        .expect("run 1 running");
    fixture.runs.end();
    sink.publish_closed("session-one", Some(run_one))
        .expect("run 1 closed");
    let history_one = events_until_closed(&mut watching).await;
    // Run 2 starts, then its actor exits on its own.
    let run_two = fixture.runs.start();
    sink.publish_activation("session-one", run_two, CapabilityReport::default())
        .await
        .expect("run 2 active");
    let mut watching_two = fixture
        .hub
        .attach(fixture.session.clone())
        .await
        .expect("attach");
    fixture.runs.end();
    let unloaded = sink
        .publish_unloaded("session-one", run_two)
        .expect("run 2 unloaded");
    let late = sink
        .publish_in("session-one", run_two, running())
        .expect("late event");
    let history_two = events_until(&mut watching_two, SessionState::Unloaded).await;
    sink.shutdown().await.expect("sink shutdown");

    // Assert
    assert!(
        matches!(
            history_one.as_slice(),
            [
                ..,
                SessionEvent::StateChanged {
                    state: SessionState::Running
                },
                SessionEvent::StateChanged {
                    state: SessionState::Closed
                }
            ]
        ),
        "run 1's events land before its Closed: {history_one:?}"
    );
    assert_eq!(unloaded, GenerationPublication::Published);
    assert_eq!(late, GenerationPublication::Ended);
    assert!(
        matches!(
            history_two.as_slice(),
            [SessionEvent::StateChanged {
                state: SessionState::Unloaded
            }]
        ),
        "Unloaded is run 2's terminal fact and nothing follows it: {history_two:?}"
    );
}

/// The live events the hub delivers up to and including the session's
/// Closed, without a timer.
async fn events_until_closed(
    attachment: &mut collaboration_contracts::SessionEventAttachment,
) -> Vec<SessionEvent> {
    events_until(attachment, SessionState::Closed).await
}

/// The live events the hub delivers up to and including a change to
/// `state`, without a timer.
async fn events_until(
    attachment: &mut collaboration_contracts::SessionEventAttachment,
    state: SessionState,
) -> Vec<SessionEvent> {
    let mut events = Vec::new();
    loop {
        let event = attachment.receiver.recv().await.expect("hub event").event;
        let reached =
            matches!(&event, SessionEvent::StateChanged { state: reached } if *reached == state);
        events.push(event);
        if reached {
            return events;
        }
    }
}
