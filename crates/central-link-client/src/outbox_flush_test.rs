use super::*;
use crate::flush_marks::{FLUSH_INTERVAL_MILLIS, FLUSH_JITTER_MILLIS};
use hegel::{TestCase, generators as gs};
use std::collections::BTreeMap;

const START: i64 = 1_763_380_800_000 - 40_000;

fn at(millis: i64) -> SyncTime {
    SyncTime::from_unix_millis(millis)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Row {
    Waiting,
    Sent,
    Answered,
}

/// Plays the Host with a model store: the store, not the reducer, holds every
/// write; the reducer only says when a batch may go.
struct FlushModel {
    state: OutboxFlushState,
    now: i64,
    next_seed: u64,
    rows: BTreeMap<u64, (SyncTime, Row)>,
    armed_wake: Option<SyncTime>,
    link_up: bool,
    sends: usize,
}

impl FlushModel {
    fn new() -> Self {
        Self {
            state: OutboxFlushState::default(),
            now: START,
            next_seed: 0,
            rows: BTreeMap::new(),
            armed_wake: None,
            link_up: false,
            sends: 0,
        }
    }

    fn earliest_due(&self) -> Option<SyncTime> {
        self.rows
            .values()
            .filter(|(_, row)| *row == Row::Waiting)
            .map(|(flush_at, _)| *flush_at)
            .min()
    }

    fn apply(&mut self, event: FlushEvent) -> Vec<FlushEffect> {
        let (state, effects) = transition(self.state.clone(), event).expect("valid event");
        self.state = state;
        for effect in &effects {
            match effect {
                FlushEffect::SendBatch { mark } => self.claim(*mark),
                FlushEffect::WakeAt { mark } => {
                    assert_eq!(mark.unix_millis().rem_euclid(FLUSH_INTERVAL_MILLIS), 0);
                    self.armed_wake = Some(*mark);
                }
            }
        }
        effects
    }

    /// The Host's claim: up to 64 waiting rows due by `mark`.
    fn claim(&mut self, mark: SyncTime) {
        assert!(self.link_up, "a batch only goes on a connected link");
        assert!(
            self.now.abs_diff(mark.unix_millis()) <= FLUSH_JITTER_MILLIS.unsigned_abs(),
            "a batch goes only inside its mark's window"
        );
        assert!(
            !self.rows.values().any(|(_, row)| *row == Row::Sent),
            "one batch in flight at a time"
        );
        let due: Vec<u64> = self
            .rows
            .iter()
            .filter(|(_, (flush_at, row))| *row == Row::Waiting && *flush_at <= mark)
            .map(|(seed, _)| *seed)
            .take(MAX_BATCH_WRITES)
            .collect();
        for seed in due {
            self.rows.get_mut(&seed).unwrap().1 = Row::Sent;
        }
        self.sends += 1;
    }

    fn store_next(&mut self) {
        let flush_at = self.state.receipt_mark(at(self.now));
        assert!(flush_at.unix_millis() + FLUSH_JITTER_MILLIS >= self.now);
        self.rows.insert(self.next_seed, (flush_at, Row::Waiting));
        self.next_seed += 1;
        self.apply(FlushEvent::Stored { flush_at });
    }

    fn end_batch(&mut self, answered: bool) {
        for (_, row) in self.rows.values_mut() {
            if *row == Row::Sent {
                *row = if answered {
                    Row::Answered
                } else {
                    Row::Waiting
                };
            }
        }
        let earliest_due = self.earliest_due();
        self.apply(FlushEvent::BatchEnded { earliest_due });
    }

    fn wake_as_host(&mut self, jitter: i64) -> Vec<FlushEffect> {
        let mark = self.armed_wake.take().expect("a wake is armed");
        self.now = self.now.max(mark.unix_millis() + jitter);
        let earliest_due = self.earliest_due();
        self.apply(FlushEvent::Tick {
            now: at(self.now),
            earliest_due,
        })
    }
}

#[hegel::state_machine]
impl FlushModel {
    #[rule(weight = 4)]
    fn store_a_write(&mut self, _: TestCase) {
        self.store_next();
    }

    #[rule]
    fn link_connects(&mut self, _: TestCase) {
        tc_link(self, true);
    }

    #[rule]
    fn link_drops(&mut self, _: TestCase) {
        tc_link(self, false);
    }

    #[rule(weight = 3)]
    fn time_reaches_the_armed_mark(&mut self, tc: TestCase) {
        tc.assume(self.armed_wake.is_some());
        let jitter = tc.draw(
            gs::integers::<i64>()
                .min_value(-FLUSH_JITTER_MILLIS)
                .max_value(FLUSH_JITTER_MILLIS),
        );
        self.wake_as_host(jitter);
    }

    #[rule(weight = 2)]
    fn the_batch_ends(&mut self, tc: TestCase) {
        tc.assume(self.state.batch_in_flight());
        let answered = tc.draw(gs::booleans());
        self.end_batch(answered);
    }

    #[invariant(always_run)]
    fn the_schedule_never_loses_a_waiting_write(&self, _: TestCase) {
        let waiting = self.earliest_due().is_some();
        let sent = self.rows.values().any(|(_, row)| *row == Row::Sent);
        assert_eq!(
            sent,
            self.state.batch_in_flight(),
            "in flight exactly while rows are sent"
        );
        if self.link_up && waiting && !self.state.batch_in_flight() {
            assert!(
                self.armed_wake.is_some(),
                "a waiting write on a live link has a wake armed"
            );
        }
    }
}

fn tc_link(model: &mut FlushModel, up: bool) {
    model.link_up = up;
    if up {
        let earliest_due = model.earliest_due();
        model.apply(FlushEvent::LinkConnected { earliest_due });
    } else {
        // The Host returns the batch on the wire to the store first.
        for (_, row) in model.rows.values_mut() {
            if *row == Row::Sent {
                *row = Row::Waiting;
            }
        }
        model.apply(FlushEvent::LinkLost);
        model.armed_wake = None;
    }
}

#[hegel::test]
fn the_flush_schedule_sends_nothing_early_twice_or_on_a_down_link(tc: TestCase) {
    hegel::stateful::machine(FlushModel::new()).run(tc);
}

#[test]
fn a_connected_link_drains_the_store_64_per_mark() {
    let mut model = FlushModel::new();
    tc_link(&mut model, true);
    for _ in 0..150 {
        model.store_next();
    }

    let mut marks = 0;
    while model.rows.values().any(|(_, row)| *row != Row::Answered) {
        model.wake_as_host(0);
        if model.state.batch_in_flight() {
            model.end_batch(true);
        }
        marks += 1;
        assert!(marks <= 4, "150 writes drain in 3 marks");
    }

    assert_eq!(model.sends, 3);
}

#[test]
fn a_write_past_its_mark_goes_on_the_next_open_mark_not_at_once() {
    let mut model = FlushModel::new();
    model.store_next();
    model.now = START + 40_000 + 20_000;
    tc_link(&mut model, true);
    let armed = model.armed_wake.expect("armed");

    let effects = model.wake_as_host(0);

    assert!(
        effects
            .iter()
            .all(|effect| !matches!(effect, FlushEffect::SendBatch { .. }))
    );
    assert_eq!(
        model.armed_wake,
        Some(at(START + 40_000 + 60_000)),
        "armed {armed:?} rearmed"
    );
}

fn workspace_reply(result: WorkspaceResult) -> MemberOutcome {
    MemberOutcome::Answered(ReplyPayload::Workspace(workspace_model::WorkspaceReply {
        result,
        actors: Vec::new(),
    }))
}

#[test]
fn only_domain_answers_are_final_and_link_admission_refusals_are_not_applied() {
    let cases = [
        (workspace_reply(WorkspaceResult::Conflict), true),
        (
            workspace_reply(WorkspaceResult::Refused(RefusalReason::WorkspaceNotAllowed)),
            true,
        ),
        (
            workspace_reply(WorkspaceResult::Refused(RefusalReason::MachineRevoked)),
            false,
        ),
        (
            MemberOutcome::Answered(ReplyPayload::Mail(CentralMailResult::Refused(
                CentralMailRefusal::Workspace(RefusalReason::MachineRevoked),
            ))),
            false,
        ),
        (
            MemberOutcome::Refused(crate::LinkAdmissionRefusal::CredentialRefused),
            false,
        ),
        (
            MemberOutcome::Refused(crate::LinkAdmissionRefusal::MachineRevoked),
            false,
        ),
        (
            MemberOutcome::Refused(crate::LinkAdmissionRefusal::RequestIdConflict),
            true,
        ),
    ];

    for (outcome, is_final) in cases {
        assert_eq!(outcome.is_final(), is_final, "{outcome:?}");
    }
}
