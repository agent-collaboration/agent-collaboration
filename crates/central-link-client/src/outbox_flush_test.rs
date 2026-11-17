use super::*;
use crate::flush_marks::{FLUSH_INTERVAL_MILLIS, FLUSH_JITTER_MILLIS};
use hegel::{TestCase, generators as gs};
use std::collections::HashMap;

const START: i64 = 1_763_380_800_000 - 40_000;

fn request_id(seed: u64) -> RequestId {
    let mut random = [0_u8; 10];
    random[..8].copy_from_slice(&seed.to_be_bytes());
    uuid::Builder::from_unix_timestamp_millis(1_763_380_000_000 + seed, &random)
        .into_uuid()
        .to_string()
        .parse()
        .unwrap()
}

fn at(millis: i64) -> SyncTime {
    SyncTime::from_unix_millis(millis)
}

/// Drives the reducer like a Host and checks every effect against a model of
/// which writes are stored, in flight and answered.
struct FlushModel {
    state: OutboxFlushState,
    now: i64,
    next_seed: u64,
    flush_at: HashMap<RequestId, SyncTime>,
    stored: BTreeSet<RequestId>,
    awaiting_answer: BTreeSet<RequestId>,
    answered: BTreeSet<RequestId>,
    armed_wake: Option<SyncTime>,
}

impl FlushModel {
    fn new() -> Self {
        Self {
            state: OutboxFlushState::default(),
            now: START,
            next_seed: 0,
            flush_at: HashMap::new(),
            stored: BTreeSet::new(),
            awaiting_answer: BTreeSet::new(),
            answered: BTreeSet::new(),
            armed_wake: None,
        }
    }

    fn apply(&mut self, event: FlushEvent) -> Vec<FlushEffect> {
        let (state, effects) = transition(self.state.clone(), event).expect("valid event");
        self.state = state;
        for effect in &effects {
            self.check_effect(effect);
        }
        effects
    }

    fn check_effect(&mut self, effect: &FlushEffect) {
        match effect {
            FlushEffect::SendBatch { mark, requests } => {
                assert!(!requests.is_empty() && requests.len() <= MAX_BATCH_WRITES);
                assert!(
                    self.now.abs_diff(mark.unix_millis()) <= FLUSH_JITTER_MILLIS.unsigned_abs(),
                    "a batch goes only inside its mark's window"
                );
                for request in requests {
                    assert!(self.flush_at[request] <= *mark, "sent before its mark");
                    assert!(!self.answered.contains(request), "answered write resent");
                    assert!(
                        self.awaiting_answer.insert(request.clone()),
                        "write sent twice while awaiting its answer"
                    );
                }
            }
            FlushEffect::RecordOutcome { request, .. } => {
                assert!(self.awaiting_answer.remove(request));
                assert!(self.answered.insert(request.clone()), "answered twice");
            }
            FlushEffect::ReturnToOutbox { requests } => {
                for request in requests {
                    assert!(self.awaiting_answer.remove(request));
                }
            }
            FlushEffect::WakeAt { mark } => {
                assert_eq!(mark.unix_millis().rem_euclid(FLUSH_INTERVAL_MILLIS), 0);
                self.armed_wake = Some(*mark);
            }
        }
    }

    /// Wake like the Host: on the armed mark plus jitter.
    fn wake_as_host(&mut self, jitter: i64) -> Vec<FlushEffect> {
        let mark = self.armed_wake.take().expect("a wake is armed");
        self.now = self.now.max(mark.unix_millis() + jitter);
        self.apply(FlushEvent::Tick { now: at(self.now) })
    }

    fn tick_on_next_mark(&mut self, jitter: i64) -> Vec<FlushEffect> {
        let mark = next_mark_after(at(self.now)).unix_millis();
        self.now = self.now.max(mark + jitter);
        self.apply(FlushEvent::Tick { now: at(self.now) })
    }

    fn store_next(&mut self) {
        let request = request_id(self.next_seed);
        self.next_seed += 1;
        let flush_at = self.state.receipt_mark(at(self.now));
        assert!(flush_at.unix_millis() + FLUSH_JITTER_MILLIS >= self.now);
        self.flush_at.insert(request.clone(), flush_at);
        self.stored.insert(request.clone());
        self.apply(FlushEvent::Stored { request, flush_at });
    }

    fn in_flight(&self) -> Vec<RequestId> {
        self.state.in_flight_members().cloned().collect()
    }
}

#[hegel::state_machine]
impl FlushModel {
    #[rule(weight = 4)]
    fn store_a_write(&mut self, _: TestCase) {
        self.store_next();
    }

    /// A Host reloading its store after the clock stepped back holds writes
    /// whose marks are still ahead.
    #[rule]
    fn reload_a_write_for_a_later_mark(&mut self, tc: TestCase) {
        let marks_ahead = tc.draw(gs::integers::<i64>().min_value(1).max_value(3));
        let request = request_id(self.next_seed);
        self.next_seed += 1;
        let open = self.state.receipt_mark(at(self.now)).unix_millis();
        let flush_at = at(open + marks_ahead * FLUSH_INTERVAL_MILLIS);
        self.flush_at.insert(request.clone(), flush_at);
        self.stored.insert(request.clone());
        self.apply(FlushEvent::Stored { request, flush_at });
    }

    #[rule]
    fn time_passes(&mut self, tc: TestCase) {
        self.now += tc.draw(gs::integers::<i64>().min_value(0).max_value(75_000));
    }

    #[rule(weight = 3)]
    fn wake_on_a_mark(&mut self, tc: TestCase) {
        let jitter = tc.draw(
            gs::integers::<i64>()
                .min_value(-FLUSH_JITTER_MILLIS)
                .max_value(FLUSH_JITTER_MILLIS),
        );
        self.tick_on_next_mark(jitter);
    }

    #[rule(weight = 3)]
    fn host_wakes_on_its_armed_mark(&mut self, tc: TestCase) {
        tc.assume(self.armed_wake.is_some());
        let jitter = tc.draw(
            gs::integers::<i64>()
                .min_value(-FLUSH_JITTER_MILLIS)
                .max_value(FLUSH_JITTER_MILLIS),
        );
        self.wake_as_host(jitter);
    }

    #[rule]
    fn wake_at_any_time(&mut self, _: TestCase) {
        self.apply(FlushEvent::Tick { now: at(self.now) });
    }

    #[rule]
    fn link_connects(&mut self, _: TestCase) {
        self.apply(FlushEvent::LinkConnected);
    }

    #[rule]
    fn link_drops(&mut self, _: TestCase) {
        self.apply(FlushEvent::LinkLost);
    }

    #[rule(weight = 3)]
    fn central_answers_a_member(&mut self, tc: TestCase) {
        let members = self.in_flight();
        tc.assume(!members.is_empty());
        let request = tc.draw(gs::sampled_from(members));
        let outcome = MemberOutcome::Refused(RefusalReason::MachineRevoked);
        self.apply(FlushEvent::MemberAnswered { request, outcome });
    }

    #[rule]
    fn batch_deadline_passes(&mut self, tc: TestCase) {
        tc.assume(!self.in_flight().is_empty());
        self.apply(FlushEvent::BatchExpired);
    }

    #[rule]
    fn stray_answers_are_refused_without_changing_state(&mut self, _: TestCase) {
        let before = self.state.clone();
        let stray = request_id((1 << 40) + self.next_seed);

        let result = transition(
            self.state.clone(),
            FlushEvent::MemberAnswered {
                request: stray,
                outcome: MemberOutcome::Refused(RefusalReason::MachineRevoked),
            },
        );

        assert!(matches!(
            result,
            Err(FlushTransitionError::NotInFlight { .. } | FlushTransitionError::NoBatchInFlight)
        ));
        assert_eq!(self.state, before);
    }

    #[invariant(always_run)]
    fn nothing_is_lost(&self, _: TestCase) {
        let waiting: BTreeSet<RequestId> = self.state.waiting().map(|(_, id)| id.clone()).collect();
        let in_flight: BTreeSet<RequestId> = self.in_flight().into_iter().collect();
        assert!(waiting.is_disjoint(&in_flight));
        assert!(waiting.is_disjoint(&self.answered));
        assert!(in_flight.is_disjoint(&self.answered));
        let held: BTreeSet<RequestId> = waiting
            .union(&in_flight)
            .chain(self.answered.iter())
            .cloned()
            .collect();
        assert_eq!(
            held, self.stored,
            "every stored write is waiting, in flight or answered"
        );
        assert_eq!(in_flight, self.awaiting_answer);
        for (flush_at, request) in self.state.waiting() {
            assert_eq!(
                self.flush_at[request], *flush_at,
                "a write keeps its receipt mark"
            );
        }
    }
}

#[hegel::test]
fn the_flush_loses_nothing_sends_nothing_twice_and_nothing_early(tc: TestCase) {
    hegel::stateful::machine(FlushModel::new()).run(tc);
}

#[hegel::test]
fn a_connected_link_drains_the_outbox_64_per_mark(tc: TestCase) {
    let writes = tc.draw(gs::integers::<usize>().min_value(0).max_value(300));
    let mut model = FlushModel::new();
    model.apply(FlushEvent::LinkConnected);
    for _ in 0..writes {
        model.store_next();
    }

    let mut marks = 0;
    while model.answered.len() < writes {
        let jitter = tc.draw(
            gs::integers::<i64>()
                .min_value(-FLUSH_JITTER_MILLIS)
                .max_value(FLUSH_JITTER_MILLIS),
        );
        let effects = model.wake_as_host(jitter);
        marks += 1;
        assert!(
            marks <= writes.div_ceil(MAX_BATCH_WRITES) + 1,
            "outbox did not drain"
        );
        for effect in effects {
            if let FlushEffect::SendBatch { requests, .. } = effect {
                for request in requests {
                    let outcome = MemberOutcome::Refused(RefusalReason::MachineRevoked);
                    model.apply(FlushEvent::MemberAnswered { request, outcome });
                }
            }
        }
    }

    assert_eq!(model.answered, model.stored);
    assert_eq!(model.state.waiting().count(), 0);
}

#[test]
fn a_tick_outside_any_window_sends_nothing_and_rearms_for_the_open_mark() {
    let mark = START + 40_000;
    let mut model = FlushModel::new();
    model.apply(FlushEvent::LinkConnected);
    model.store_next();
    model.now = mark - 10_000;

    let effects = model.apply(FlushEvent::Tick { now: at(model.now) });

    assert_eq!(effects, vec![FlushEffect::WakeAt { mark: at(mark) }]);
}
