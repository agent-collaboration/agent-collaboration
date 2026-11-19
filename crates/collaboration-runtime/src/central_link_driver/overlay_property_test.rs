//! The overlay invariant (Lead A, option C) against the real driver, store and
//! flush, with a scripted central that keeps a request log and checks every
//! replay's caller, target and content the way central's gate does. Each
//! flush member is answered, recorded with its answer lost, lost before
//! central, or not applied (machine revoked); links drop and reconnect; store
//! calls fail, before the call or after it committed, in the phase that
//! reaches them, and every injected fault must fire (step 9 C). After every
//! step the caller's own view (central's records plus `pending_for`) shows:
//! - an unsent write once, and central does not have it;
//! - an answered write once, with the id central created first;
//! - a sent write whose answer was lost at most twice (pending plus central).
//!
//! When link and store heal and no new work arrives, every accepted write is
//! answered, by id, within a bounded number of marks.
use super::driver_test_support::*;
use super::scripted_central::{MemberFate, ScriptedCentral, created_post, request_id, take_batch};
use super::store_faults::{FaultKind, FaultPlan, FaultyOutbox};
use central_link_client::{LinkCalls, PendingWrite, PendingWriteState, Submission};
use hegel::{TestCase, generators as gs};
use runtime_primitives::Clock;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

/// Marks a healed Router may take to answer everything it accepted.
const HEALED_WITHIN_MARKS: usize = 4;
/// Marks a tick-phase fault may wait for the tick that reaches it.
const TICK_WITHIN_MARKS: usize = 3;

struct OverlayModel {
    runtime: tokio::runtime::Runtime,
    harness: Harness,
    central: Option<CentralEnd>,
    scripted: ScriptedCentral,
    /// Writes the outbox holds, whatever their submitter was told.
    accepted: BTreeSet<String>,
    /// When each accepted write was stored, for its 15-minute deadline.
    accepted_at: std::collections::BTreeMap<String, i64>,
    next_seed: u16,
    faults: FaultPlan,
}

fn fault_kind(tc: &TestCase) -> FaultKind {
    tc.draw(gs::sampled_from(vec![
        FaultKind::BeforeCall,
        FaultKind::AfterCommit,
    ]))
}

impl OverlayModel {
    fn settle(&self) {
        for _ in 0..3 {
            let _ = self.runtime.block_on(self.harness.calls.link_status());
        }
    }

    /// After dropping central's end: round trips until the driver has read
    /// every queued reply and seen the close (bounded).
    fn settle_until_disconnected(&self) {
        for _ in 0..64 {
            let status = self.runtime.block_on(self.harness.calls.link_status());
            if !matches!(
                status,
                Some(central_link_client::LinkStatus::Connected { .. })
            ) {
                break;
            }
        }
        self.settle();
    }

    fn advance(&self, millis: u64) {
        self.harness
            .clock
            .advance(Duration::from_millis(millis))
            .unwrap();
        self.settle();
    }

    fn advance_to_next_mark(&self) {
        let now = self.harness.clock.now().unix_millis();
        let next = (now.div_euclid(60_000) + 1) * 60_000;
        self.advance(u64::try_from(next - now).unwrap());
    }

    fn pending(&self) -> Vec<PendingWrite> {
        self.runtime
            .block_on(self.harness.calls.pending_for(caller()))
            .unwrap()
    }

    fn has_waiting_work(&self) -> bool {
        self.pending().iter().any(|write| {
            matches!(
                write.state,
                PendingWriteState::Unsent { .. } | PendingWriteState::SentAnswerLost { .. }
            )
        })
    }

    /// Plants one fault; `reach` must make the store call it names.
    fn inject_and_require(
        &mut self,
        site: &'static str,
        kind: FaultKind,
        reach: impl FnOnce(&mut Self),
    ) {
        let before = self.faults.fired().len();
        self.faults.fail(site, kind);
        reach(self);
        // Let the driver read every queued frame; the clock does not move.
        for _ in 0..64 {
            if !self.faults.pending().contains(&site) {
                break;
            }
            let _ = self.runtime.block_on(self.harness.calls.link_status());
        }
        let fired = self.faults.fired();
        assert!(
            fired[before..].contains(&(site, kind)),
            "{site} {kind:?} never fired; still planned {:?}",
            self.faults.pending()
        );
    }

    /// Plays central for a batch in flight with drawn fates; an answer-phase
    /// fault may be planted on the first member whose fate reaches it.
    fn resolve_batch(&mut self, tc: &TestCase) {
        let Some(members) = self.central.as_mut().and_then(take_batch) else {
            return;
        };
        let mut every_answered = true;
        let mut planted = false;
        for member in &members {
            assert!(
                self.accepted.contains(&request_id(member)),
                "only accepted writes are sent"
            );
            let fate = tc.draw(gs::sampled_from(vec![
                MemberFate::Answered,
                MemberFate::RecordedAnswerLost,
                MemberFate::LostBeforeCentral,
                MemberFate::Unapplied,
            ]));
            let site = match fate {
                MemberFate::Answered => Some("record_outcome"),
                MemberFate::Unapplied => Some("return_unapplied"),
                MemberFate::RecordedAnswerLost | MemberFate::LostBeforeCentral => None,
            };
            if let Some(site) = site
                && !planted
                && tc.draw(gs::booleans())
            {
                planted = true;
                let kind = fault_kind(tc);
                let central = self.central.take();
                self.inject_and_require(site, kind, |model| {
                    model.scripted.play(central.as_ref(), member, fate);
                    model.settle();
                });
                self.central = central;
            } else {
                self.scripted.play(self.central.as_ref(), member, fate);
            }
            if matches!(
                fate,
                MemberFate::RecordedAnswerLost | MemberFate::LostBeforeCentral
            ) {
                every_answered = false;
            }
        }
        if !every_answered {
            // The rest of the batch's answers never arrive: the link drops,
            // and the unanswered members are returned, maybe with a fault.
            if tc.draw(gs::booleans()) {
                let kind = fault_kind(tc);
                self.inject_and_require("return_unanswered", kind, |model| {
                    model.central = None;
                    model.settle_until_disconnected();
                });
            } else {
                self.central = None;
                self.settle_until_disconnected();
            }
        }
        // pending_for waits until the batch in flight has ended.
        let _ = self.pending();
    }
}

#[hegel::state_machine]
impl OverlayModel {
    #[rule(weight = 3)]
    fn a_session_posts(&mut self, tc: TestCase) {
        let id = uuid_v7(1_000 + self.next_seed);
        self.next_seed += 1;
        let mut submission = None;
        let mut submit = |model: &mut Self| {
            submission = Some(
                model
                    .runtime
                    .block_on(model.harness.calls.submit(caller(), thread_post(&id))),
            );
            model.settle();
        };
        let fault = if tc.draw(gs::integers::<u8>().max_value(3)) == 0 {
            let kind = fault_kind(&tc);
            self.inject_and_require("insert", kind, submit);
            Some(kind)
        } else {
            submit(self);
            None
        };
        // request_status reads the store at once; pending_for would wait for a
        // batch this submit may have started.
        let held = self.runtime.block_on(self.harness.local_status(&id))
            != central_link_client::RequestStatus::NotFound;
        match (fault, submission) {
            (None, Some(Submission::InOutbox(_))) => {
                assert!(held, "a receipt means the write is stored");
            }
            (
                Some(FaultKind::BeforeCall),
                Some(Submission::Refused(central_link_client::SubmitRefusal::OutboxUnavailable)),
            ) => assert!(!held, "a definite refusal holds no row"),
            (Some(FaultKind::AfterCommit), Some(Submission::Uncertain { request, scope })) => {
                assert!(held, "the answer was lost after the write committed");
                assert_eq!(request.as_str(), id);
                assert_eq!(scope, thread_post(&id).request_scope());
            }
            other => panic!("unexpected submission {other:?}"),
        }
        if held {
            self.accepted_at
                .insert(id.clone(), self.harness.clock.now().unix_millis());
            self.accepted.insert(id);
        }
        // A write stored inside a mark's window that passed while the link was
        // down can start a batch at once; central answers it in this step.
        self.resolve_batch(&tc);
    }

    #[rule(weight = 3)]
    fn the_next_mark_flushes(&mut self, tc: TestCase) {
        tc.assume(self.central.is_some());
        let tick_fault = self.has_waiting_work() && tc.draw(gs::booleans());
        if tick_fault {
            let site = tc.draw(gs::sampled_from(vec![
                "earliest_waiting",
                "end_overdue",
                "delete_ended_before_retention",
                "claim_next_batch",
            ]));
            let kind = fault_kind(&tc);
            let fates = tc.clone();
            self.inject_and_require(site, kind, |model| {
                for _ in 0..TICK_WITHIN_MARKS {
                    model.advance_to_next_mark();
                    model.resolve_batch(&fates);
                    if !model.faults.pending().contains(&site) || model.central.is_none() {
                        break;
                    }
                }
            });
        } else {
            self.advance_to_next_mark();
            self.resolve_batch(&tc);
        }
    }

    #[rule]
    fn the_link_drops(&mut self, tc: TestCase) {
        tc.assume(self.central.is_some());
        self.central = None;
        self.settle_until_disconnected();
    }

    #[rule(weight = 2)]
    fn the_link_reconnects(&mut self, tc: TestCase) {
        tc.assume(self.central.is_none());
        // Welcome reconciles sent rows: sent_without_answer is always read.
        let fault = tc.draw(gs::booleans()).then(|| fault_kind(&tc));
        let connect = |model: &mut Self| {
            // The client's backoff runs on the injected clock (at most 30 s).
            for _ in 0..40 {
                model.advance(1_000);
                if let Ok(central) = model.harness.connections.try_recv() {
                    central.welcome();
                    model.central = Some(central);
                    model.settle();
                    return;
                }
            }
            panic!("the link did not reconnect within its backoff");
        };
        match fault {
            Some(kind) => self.inject_and_require("sent_without_answer", kind, connect),
            None => connect(self),
        }
        self.resolve_batch(&tc);
    }

    /// Link and store healthy, no new work: every accepted write is answered,
    /// by id, within a bounded number of marks.
    #[rule]
    fn the_link_and_store_heal_and_everything_drains(&mut self, tc: TestCase) {
        assert!(
            self.faults.pending().is_empty(),
            "every injected fault fired"
        );
        for _ in 0..HEALED_WITHIN_MARKS {
            if self.central.is_none() {
                self.the_link_reconnects_healthy();
                // Welcome may send a due batch at once; answer it before time moves.
                self.answer_batch_on_the_wire();
            }
            self.advance_to_next_mark();
            self.answer_batch_on_the_wire();
        }
        let pending = self.pending();
        for id in &self.accepted {
            let write = pending.iter().find(|write| write.request_id.as_str() == id);
            // A write past its 15 minutes may end instead (checked by the invariant).
            if matches!(
                write.map(|write| &write.state),
                Some(PendingWriteState::Cancelled { .. })
            ) {
                continue;
            }
            let Some(PendingWriteState::Answered(reply)) = write.map(|write| &write.state) else {
                panic!("{id} not answered within {HEALED_WITHIN_MARKS} healthy marks: {write:?}");
            };
            assert_eq!(
                created_post(reply),
                self.scripted.log.get(id).map(|record| record.post.clone()),
                "{id} names the post central created first"
            );
        }
        let _ = tc;
    }

    #[invariant(always_run)]
    fn each_own_write_shows_as_option_c_allows(&self, _: TestCase) {
        if self.central.is_some() {
            assert!(
                matches!(
                    self.runtime.block_on(self.harness.calls.link_status()),
                    Some(central_link_client::LinkStatus::Connected { .. })
                ),
                "a store error never changes the link"
            );
        }
        let pending = self.pending();
        let mut seen = BTreeSet::new();
        for write in &pending {
            let id = write.request_id.as_str().to_owned();
            assert!(seen.insert(id.clone()), "{id} is pending twice");
            assert!(self.accepted.contains(&id), "{id} was never accepted");
            let in_central = self.scripted.log.get(&id).map(|record| &record.post);
            match &write.state {
                PendingWriteState::Unsent { .. } => {
                    assert!(in_central.is_none(), "an unsent write is not in central");
                }
                // Shown by pending and possibly by central: at most twice.
                PendingWriteState::SentAnswerLost { .. } => {}
                PendingWriteState::Answered(reply) => {
                    assert_eq!(
                        created_post(reply).as_ref(),
                        in_central,
                        "the answer names the post central created first"
                    );
                }
                PendingWriteState::Cancelled { reason } => {
                    let stored = self.accepted_at[&id];
                    assert!(
                        self.harness.clock.now().unix_millis() >= stored + 15 * 60 * 1000,
                        "{id} ended before its 15 minutes: {reason:?}"
                    );
                    if *reason == central_link_client::OutboxCancellation::NotSentWithin15Minutes {
                        assert!(
                            in_central.is_none(),
                            "{id} never left the Mac, yet central has it"
                        );
                    }
                }
            }
        }
        assert_eq!(seen, self.accepted, "every accepted write is in the view");
    }
}

impl OverlayModel {
    fn answer_batch_on_the_wire(&mut self) {
        if let Some(members) = self.central.as_mut().and_then(take_batch) {
            for member in &members {
                self.scripted
                    .play(self.central.as_ref(), member, MemberFate::Answered);
            }
            self.settle();
        }
    }

    fn the_link_reconnects_healthy(&mut self) {
        for _ in 0..40 {
            self.advance(1_000);
            if let Ok(central) = self.harness.connections.try_recv() {
                central.welcome();
                self.central = Some(central);
                self.settle();
                return;
            }
        }
        panic!("the link did not reconnect within its backoff");
    }
}

/// 250 cases: what the suite affords in CI (Verifier step 9: 40 was too few
/// to be evidence; the old oracle failed at 400 on seed 41).
#[hegel::test(test_cases = 250)]
fn each_write_shows_once_except_a_lost_answer_until_its_replay(tc: TestCase) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let faults = FaultPlan::default();
    let inner = runtime
        .block_on(open_outbox(&root.path().join("machine_local.sqlite")))
        .unwrap();
    let store = FaultyOutbox {
        inner: Box::new(inner),
        plan: faults.clone(),
    };
    let entered = runtime.enter();
    let mut harness = Harness::start_with_store(
        root,
        clock_at(MARK - 20_000),
        Arc::new(tokio::sync::Mutex::new(store)),
    );
    drop(entered);
    let central = runtime.block_on(harness.connected());
    let model = OverlayModel {
        runtime,
        harness,
        central: Some(central),
        scripted: ScriptedCentral::default(),
        accepted: BTreeSet::new(),
        accepted_at: std::collections::BTreeMap::new(),
        next_seed: 0,
        faults,
    };
    hegel::stateful::machine(model).run(tc);
}
