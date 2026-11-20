//! V29: the cache-warm owner on one injected manual clock and a recording stand-in
//! for the supervisor's prompt path. No session, cancel or steer call exists on it.
use super::*;
use collaboration_protocol::EndpointId;
use provider_session_machines::{SessionGeneration, session_owner::OwnerAddress};
use router_clock::ManualClock;
use session_event_model::{StopReason, TurnOutcome};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::mpsc;

const MINUTES_55: Duration = Duration::from_secs(55 * 60);

/// Records every attempt; a refusal is still an attempt but sends nothing.
struct RecordingSubmitter {
    attempts: AtomicUsize,
    refusals_left: AtomicUsize,
    sent: mpsc::UnboundedSender<(SessionRef, InputId)>,
}

impl WarmPromptSubmitter for RecordingSubmitter {
    fn submit_warm_prompt(
        &self,
        target: SessionRef,
        _run: WarmRun,
        input_id: InputId,
    ) -> WarmPromptFuture<'_> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        let refused = self
            .refusals_left
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                left.checked_sub(1)
            })
            .is_ok();
        Box::pin(async move {
            if refused {
                return Err(WarmPromptRefused);
            }
            let _ = self.sent.send((target, input_id));
            Ok(())
        })
    }
}

struct Harness {
    clock: Arc<ManualClock>,
    submitter: Arc<RecordingSubmitter>,
    sent: mpsc::UnboundedReceiver<(SessionRef, InputId)>,
    warm: Arc<ProviderCacheWarm>,
}

fn endpoint() -> EndpointRef {
    EndpointRef {
        service_id: "0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89"
            .to_owned()
            .try_into()
            .expect("service ID"),
        endpoint_id: EndpointId::try_from("claude-local".to_owned()).expect("endpoint ID"),
    }
}

fn on(every_minutes: u32, for_hours: u32) -> CacheWarmSetting {
    CacheWarmSetting {
        enabled: true,
        every_minutes,
        for_hours,
    }
}

fn harness_with(refusals: usize) -> Harness {
    let clock = Arc::new(ManualClock::new(
        "2026-11-15T00:00:00Z".parse().expect("start time"),
    ));
    let (sender, receiver) = mpsc::unbounded_channel();
    let submitter = Arc::new(RecordingSubmitter {
        attempts: AtomicUsize::new(0),
        refusals_left: AtomicUsize::new(refusals),
        sent: sender,
    });
    let warm = ProviderCacheWarm::for_provider(
        ProviderKind::ClaudeCode,
        Some(on(55, 8)),
        endpoint(),
        clock.clone(),
        submitter.clone(),
    )
    .expect("Claude with the setting on gets a module");
    Harness {
        clock,
        submitter,
        sent: receiver,
        warm,
    }
}

fn harness() -> Harness {
    harness_with(0)
}

impl Harness {
    fn attempts(&self) -> usize {
        self.submitter.attempts.load(Ordering::SeqCst)
    }

    /// Lets every task that is ready on the single test thread run to its next wait.
    async fn settle(&self) {
        for _ in 0..32 {
            tokio::task::yield_now().await;
        }
    }

    async fn advance(&self, by: Duration) {
        self.clock.advance(by).expect("manual clock advance");
        self.settle().await;
    }

    fn observe(&self, session: &str, event: SessionEvent) {
        use provider_session_machines::session_owner::{ConnectionEpoch, OwnerIncarnation};
        let fixture_id = uuid::Uuid::from_u128(0x0000_0000_0000_7000_8000_0000_0000_0001);
        let run = WarmRun {
            owner: OwnerAddress {
                incarnation: OwnerIncarnation(fixture_id),
                connection_epoch: ConnectionEpoch(fixture_id),
            },
            generation: SessionGeneration::UNTRACKED,
        };
        self.warm.observe_live_event(session, run, &event);
    }

    fn start_turn(&self, session: &str, turn: &str, input: &InputId) {
        self.observe(
            session,
            SessionEvent::TurnStarted {
                turn_id: turn.to_owned(),
                input_id: input.clone(),
            },
        );
    }

    fn end_turn(&self, session: &str, turn: &str) {
        self.observe(
            session,
            SessionEvent::TurnEnded {
                turn_id: turn.to_owned(),
                outcome: TurnOutcome::Ended {
                    stop_reason: StopReason::EndTurn,
                    local_cause: None,
                },
            },
        );
    }

    fn real_turn(&self, session: &str, turn: &str) {
        let input = InputId::new(format!("real-input-{turn}")).expect("input ID");
        self.start_turn(session, turn, &input);
        self.end_turn(session, turn);
    }

    /// The warm prompt the module sent: its turn starts and ends like any other.
    fn warm_turn(&self, session: &str, turn: &str, input: &InputId) {
        self.start_turn(session, turn, input);
        self.end_turn(session, turn);
    }

    fn next_sent(&mut self) -> Option<(SessionRef, InputId)> {
        self.sent.try_recv().ok()
    }
}

#[tokio::test]
async fn idle_session_gets_one_warm_prompt() {
    let mut harness = harness();
    harness.real_turn("session-one", "turn-1");

    harness.advance(Duration::from_secs(54 * 60)).await;
    assert!(harness.next_sent().is_none(), "54 minutes is too early");

    harness.advance(Duration::from_secs(60)).await;
    let (target, _) = harness.next_sent().expect("55 minutes sends one prompt");
    assert_eq!(String::from(target.session_id.clone()), "session-one");
    assert_eq!(target.endpoint, endpoint());

    harness.advance(Duration::from_secs(60)).await;
    assert!(
        harness.next_sent().is_none(),
        "one prompt, not one per minute"
    );
    assert_eq!(harness.attempts(), 1);
}

#[tokio::test]
async fn busy_session_gets_none() {
    let mut harness = harness_with(1);
    harness.real_turn("session-one", "turn-1");

    // The supervisor refuses the first warm prompt because the session is busy.
    harness.advance(MINUTES_55).await;
    assert_eq!(harness.attempts(), 1);
    assert!(harness.next_sent().is_none(), "a refusal sends nothing");

    // Not now: no retry until the next real turn ends.
    harness.advance(MINUTES_55 * 3).await;
    assert_eq!(harness.attempts(), 1, "no retry timer after a refusal");

    harness.real_turn("session-one", "turn-2");
    harness.advance(MINUTES_55).await;
    assert!(
        harness.next_sent().is_some(),
        "the timer re-arms after the next real turn"
    );
}

#[tokio::test]
async fn running_turn_disarms_the_timer() {
    let mut harness = harness();
    harness.real_turn("session-one", "turn-1");
    harness.advance(Duration::from_secs(30 * 60)).await;

    let input = InputId::new("real-input-2").expect("input ID");
    harness.start_turn("session-one", "turn-2", &input);
    harness.advance(MINUTES_55 * 2).await;
    assert_eq!(harness.attempts(), 0, "no prompt while a turn runs");

    harness.end_turn("session-one", "turn-2");
    harness.advance(MINUTES_55).await;
    assert!(harness.next_sent().is_some());
}

#[tokio::test]
async fn warm_turns_do_not_extend_the_limit() {
    let mut harness = harness();
    harness.real_turn("session-one", "turn-1");

    // 55, 110, ... 440 minutes are inside the 8 hours after the real turn.
    for round in 1..=8 {
        harness.advance(MINUTES_55).await;
        let (_, input_id) = harness
            .next_sent()
            .unwrap_or_else(|| panic!("warm prompt {round} is due inside 8 hours"));
        harness.warm_turn("session-one", &format!("warm-{round}"), &input_id);
    }

    harness.advance(MINUTES_55).await;
    assert!(
        harness.next_sent().is_none(),
        "495 minutes is past 8 hours since the last real turn"
    );
    assert_eq!(harness.attempts(), 8);
}

#[tokio::test]
async fn real_input_steered_into_a_warm_turn_counts_as_real_work() {
    let mut harness = harness();
    harness.real_turn("session-one", "turn-1");
    harness.advance(MINUTES_55).await;
    let (_, warm_input) = harness.next_sent().expect("first warm prompt");
    harness.start_turn("session-one", "warm-1", &warm_input);
    harness.observe(
        "session-one",
        SessionEvent::InputAccepted {
            input_id: InputId::new("steered-real").expect("input ID"),
            turn_id: "warm-1".to_owned(),
        },
    );
    harness.advance(Duration::from_secs(7 * 3600)).await;
    harness.end_turn("session-one", "warm-1");

    // The steered message ended a real turn 7 hours in: 8 hours run from there.
    harness.advance(MINUTES_55).await;
    assert!(
        harness.next_sent().is_some(),
        "a steered real message moves the last real turn"
    );
}

#[tokio::test]
async fn off_and_cursor_get_none() {
    let clock: Arc<dyn RuntimeClock> = Arc::new(ManualClock::new(
        "2026-11-15T00:00:00Z".parse().expect("start time"),
    ));
    let (sender, _sent) = mpsc::unbounded_channel();
    let submitter = Arc::new(RecordingSubmitter {
        attempts: AtomicUsize::new(0),
        refusals_left: AtomicUsize::new(0),
        sent: sender,
    });
    let build = |provider, setting| {
        ProviderCacheWarm::for_provider(
            provider,
            setting,
            endpoint(),
            clock.clone(),
            submitter.clone(),
        )
    };

    assert!(build(ProviderKind::Cursor, Some(on(55, 8))).is_none());
    assert!(
        build(
            ProviderKind::ClaudeCode,
            Some(CacheWarmSetting {
                enabled: false,
                ..on(55, 8)
            })
        )
        .is_none()
    );
    assert!(build(ProviderKind::ClaudeCode, None).is_none());
    assert!(build(ProviderKind::ClaudeCode, Some(on(55, 8))).is_some());
}

#[tokio::test]
async fn close_drops_the_timer() {
    let mut harness = harness();
    harness.real_turn("session-one", "turn-1");
    harness.real_turn("session-two", "turn-1");

    harness.observe(
        "session-one",
        SessionEvent::StateChanged {
            state: SessionState::Closed,
        },
    );
    harness.advance(MINUTES_55).await;

    let (target, _) = harness.next_sent().expect("the open session is warmed");
    assert_eq!(String::from(target.session_id), "session-two");
    assert!(harness.next_sent().is_none(), "the closed session is not");
    assert_eq!(harness.attempts(), 1);
}

#[tokio::test]
async fn shutdown_stops_every_timer_before_anything_can_submit() {
    let harness = harness();
    harness.real_turn("session-one", "turn-1");

    harness.warm.shutdown().await;
    harness.advance(MINUTES_55 * 2).await;
    harness.real_turn("session-one", "turn-2");
    harness.advance(MINUTES_55).await;

    assert_eq!(harness.attempts(), 0);
}
