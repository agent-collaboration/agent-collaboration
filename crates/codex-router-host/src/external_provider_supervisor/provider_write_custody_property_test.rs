use super::*;
use crate::operation_machines::operation_record_machine::{AttemptEnding, OperationRecord};
use crate::provider_operation_custody::LaneWriteCustody;
use collaboration_protocol::{
    ConversationOperationFailure, ConversationOperationFailureKind,
    ConversationOperationFailureStage, ProviderOperationEffect,
};
use futures_util::FutureExt as _;
use router_clock::{ClockInstant, ManualClock, RuntimeClock as _};
use std::time::Duration;
use tokio_util::task::{TaskTracker, task_tracker::TaskTrackerToken};

use super::super::{
    MAX_RETAINED_SETTLEMENTS, failure, provider_operation_finish::Retention,
    provider_operation_handoff::mark_receipt_is_timely,
    provider_owner_prompt::prompt_failure_answer,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Row {
    Absent,
    Admitted,
    Marked,
    Terminal(ProviderOperationEffect),
    NotReconcilable(ProviderOperationEffect),
}

impl Row {
    fn effect(self) -> ProviderOperationEffect {
        match self {
            Self::Absent | Self::Admitted => ProviderOperationEffect::None,
            Self::Marked => ProviderOperationEffect::Unknown,
            Self::Terminal(effect) | Self::NotReconcilable(effect) => effect,
        }
    }
}

struct OperationTrace {
    operation: OperationId,
    target: SessionRef,
    live: Arc<LiveOperation>,
    row: Row,
    // Oracle: account for the Host and each accepted lane entry separately.
    host_open: bool,
    task: Option<TaskTrackerToken>,
    lane: [Option<LaneWriteCustody>; 2],
    handed_over: bool,
    observed: Option<ProviderOperationEffect>,
    answer: Option<serde_json::Value>,
}

impl OperationTrace {
    fn new(
        index: &mut LiveOperations,
        binding: &ProviderBindingIdentity,
        name: &str,
        tracker: &TaskTracker,
    ) -> Self {
        let operation = OperationId::generate();
        let target = target(binding, name);
        let request = AdmissionRequest {
            operation: &operation,
            kind: ProviderOperationKind::ConversationLoad,
            target: Some(&target),
            mode: AdmissionMode::HANDOFF,
        };
        assert!(matches!(
            reserve(index, binding, request),
            Ok(Reservation::Acquired)
        ));
        let live = Arc::clone(&index.operations[&operation]);
        Self {
            operation,
            target,
            live,
            row: Row::Absent,
            host_open: true,
            task: Some(tracker.token()),
            lane: [None, None],
            handed_over: false,
            observed: None,
            answer: None,
        }
    }

    fn owns_work(&self) -> bool {
        self.host_open || self.lane.iter().any(Option::is_some)
    }

    fn admit(&mut self) {
        if self.host_open && self.row == Row::Absent {
            self.live.admitted();
            self.row = Row::Admitted;
        }
    }

    fn drop_unadmitted(&mut self, index: &mut LiveOperations) {
        if self.row == Row::Absent && self.host_open {
            self.live.dropped(false, CustodyEnd::Dropped);
            index.operations.remove(&self.operation);
            self.host_open = false;
        }
    }

    fn acknowledge_mark(&mut self, matching: bool, arrival: u8) {
        if self.row != Row::Admitted || self.observed.is_some() || !matching {
            return;
        }
        self.row = Row::Marked;
        let deadline = ClockInstant::ZERO.after(Duration::from_millis(1));
        let observed = ClockInstant::ZERO.after(Duration::from_millis(u64::from(arrival)));
        self.handed_over = mark_receipt_is_timely(observed, deadline);
        assert_eq!(
            self.handed_over,
            arrival == 0,
            "I-c: only a matching receipt strictly before L authorizes handoff"
        );
        if self.handed_over {
            self.live.dispatch_marked();
        } else {
            // A late committed mark can remain conservative until the
            // retained writer settles the known-unsent result.
            self.decide(ProviderOperationEffect::None);
        }
    }

    fn decide(&mut self, effect: ProviderOperationEffect) {
        if self.observed.is_some()
            || !self.host_open
            || self.row == Row::Absent
            || effect != ProviderOperationEffect::None && !self.handed_over
        {
            return;
        }
        self.observed = Some(effect);
        self.live.completed(match effect {
            ProviderOperationEffect::None => AttemptEnding::Refused,
            ProviderOperationEffect::Applied => AttemptEnding::Applied,
            ProviderOperationEffect::Unknown => AttemptEnding::Unknown,
        });
        if effect == ProviderOperationEffect::Unknown {
            self.live.uncertainty_recorded();
        }
    }

    fn publish(&mut self, index: &mut LiveOperations) {
        if let Some(effect) = self.observed
            && self.answer.is_none()
        {
            let answer = failure(
                ConversationOperationFailureKind::Unavailable,
                ConversationOperationFailureStage::Settlement,
                effect,
                "fixture answer",
                self.operation.clone(),
                Some(self.target.clone()),
            );
            *self.live.result.try_lock().unwrap() = Some(Err(answer.clone()));
            self.answer = Some(failure_wire(&answer));
            index.retain_result(self.operation.clone(), Retention::PersistenceFailed);
        }
    }

    fn offer_lane(
        &mut self,
        index: &LiveOperations,
        binding: &ProviderBindingIdentity,
        slot: usize,
        matching: bool,
    ) {
        if !self.handed_over || !self.host_open || self.lane[slot].is_some() {
            return;
        }
        let mut generation = binding.generation.clone();
        if !matching {
            generation.generation = 2.try_into().unwrap();
        }
        self.lane[slot] =
            index.begin_lane_write(&self.operation, &generation, &self.target, Arc::new(|| {}));
        assert_eq!(
            self.lane[slot].is_some(),
            matching,
            "I-g: wrong generation changed writer custody"
        );
    }

    fn write_receipt(&mut self, end: CustodyEnd) {
        if let Some(observed) = self.observed
            && end == CustodyEnd::Written
        {
            assert!(
                !matches!(self.row, Row::NotReconcilable(_)),
                "I-a: reconcile took the row before the writer finished"
            );
            self.row = Row::Terminal(observed);
        }
    }

    fn end_host(&mut self, end: CustodyEnd) {
        if self.host_open && self.observed.is_some() {
            self.write_receipt(end);
            self.live.finish_write(end);
            self.host_open = false;
        }
    }

    fn end_lane(&mut self, slot: usize, end: CustodyEnd) {
        if self.observed.is_some()
            && let Some(ticket) = self.lane[slot].take()
        {
            self.write_receipt(end);
            ticket.settle(end);
        }
    }

    fn reconcile(&mut self, index: &LiveOperations) {
        let before = self.row;
        if !index.holds_custody_of(&self.operation)
            && matches!(
                before,
                Row::Admitted | Row::Marked | Row::Terminal(ProviderOperationEffect::Unknown)
            )
        {
            // Model only the durable projection of the SQL update. The real
            // Host witnesses exercise this guarded write against SQLite.
            self.row = Row::NotReconcilable(before.effect());
        }
        assert_eq!(
            self.row.effect(),
            before.effect(),
            "I-d: reconcile strengthened the effect"
        );
        if self.owns_work() {
            assert_eq!(
                self.row, before,
                "I-a: reconcile rewrote a row with a retained writer"
            );
        }
    }

    fn check(&self, index: &LiveOperations) {
        assert_eq!(
            self.live.holds_custody(),
            self.owns_work(),
            "I-g: release needs a definitive writer receipt"
        );
        assert_eq!(
            index.holds_custody_of(&self.operation),
            self.owns_work(),
            "I-a: answer visibility is independent of row custody"
        );
        if self.owns_work() {
            assert!(
                index.operations.contains_key(&self.operation),
                "I-g: writer evicted"
            );
        }
        if matches!(self.row, Row::Terminal(ProviderOperationEffect::Applied)) {
            assert!(
                self.handed_over,
                "I-d: an undispatched operation saved Applied"
            );
        }
        if self.row == Row::Admitted {
            assert!(!matches!(
                self.live.record(),
                OperationRecord::MayHaveDispatched { .. }
            ));
        }
        let retained = self.live.result.try_lock().unwrap();
        assert_eq!(
            retained
                .as_ref()
                .and_then(|result| result.as_ref().err())
                .map(failure_wire),
            self.answer,
            "I-e: late evidence changed the published answer"
        );
    }
}

fn failure_wire(failure: &ConversationOperationFailure) -> serde_json::Value {
    serde_json::to_value(failure).expect("serialize the operation failure")
}

#[hegel::test(seed = Some(29114), test_cases = 100)]
fn generated_reconcile_and_drain_follow_writer_custody(case: hegel::TestCase) {
    let binding = binding();
    let mut index = LiveOperations::default();
    let tracker = TaskTracker::new();
    let mut operations = [
        OperationTrace::new(&mut index, &binding, "first", &tracker),
        OperationTrace::new(&mut index, &binding, "second", &tracker),
    ];
    tracker.close();
    let first = &mut operations[0];
    first.admit();
    if case.draw(gs::booleans()) {
        first.acknowledge_mark(true, 0);
        first.offer_lane(&index, &binding, 0, true);
        first.decide(ProviderOperationEffect::Applied);
        first.end_host(CustodyEnd::NoWrite);
    } else {
        first.decide(ProviderOperationEffect::None);
    }
    first.publish(&mut index);
    first.reconcile(&index);
    first.check(&index);

    let mut pressured = false;
    for _ in 0..case.draw(gs::integers::<u8>().min_value(1).max_value(40)) {
        let operation = &mut operations[usize::from(case.draw(gs::booleans()))];
        let receipt = [
            CustodyEnd::Written,
            CustodyEnd::Failed,
            CustodyEnd::Released,
        ][usize::from(case.draw(gs::integers::<u8>().max_value(2)))];
        let slot = usize::from(case.draw(gs::booleans()));
        match case.draw(gs::integers::<u8>().max_value(11)) {
            0 => operation.admit(),
            1 => operation.acknowledge_mark(
                case.draw(gs::booleans()),
                case.draw(gs::integers::<u8>().max_value(2)),
            ),
            2 => operation.offer_lane(&index, &binding, slot, case.draw(gs::booleans())),
            3 => operation.decide(
                [
                    ProviderOperationEffect::None,
                    ProviderOperationEffect::Applied,
                    ProviderOperationEffect::Unknown,
                ][usize::from(case.draw(gs::integers::<u8>().max_value(2)))],
            ),
            4 => operation.publish(&mut index),
            5 => operation.end_host(receipt),
            6 => operation.end_lane(slot, receipt),
            7 => operation.reconcile(&index),
            8 if !operation.host_open => drop(operation.task.take()),
            9 if !pressured => {
                fill_result_window(&mut index);
                pressured = true;
            }
            10 => operation.drop_unadmitted(&mut index),
            _ => index.trim_results(),
        }
        for operation in &operations {
            operation.check(&index);
        }
        let ready = index.custodians().next().is_none() && tracker.wait().now_or_never().is_some();
        assert_eq!(
            ready,
            operations
                .iter()
                .all(|operation| !operation.owns_work() && operation.task.is_none()),
            "I-f: drain needs both writer receipts and joined work"
        );
    }
    for operation in &mut operations {
        operation.drop_unadmitted(&mut index);
        operation.decide(ProviderOperationEffect::None);
        operation.end_host(CustodyEnd::Released);
        operation.end_lane(0, CustodyEnd::Released);
        operation.end_lane(1, CustodyEnd::Released);
        operation.check(&index);
        drop(operation.task.take());
    }
    assert!(index.custodians().next().is_none() && tracker.wait().now_or_never().is_some());
}

fn fill_result_window(index: &mut LiveOperations) {
    for _ in 0..MAX_RETAINED_SETTLEMENTS {
        let operation = OperationId::generate();
        let ended = Arc::new(LiveOperation::pending());
        ended.finish_write(CustodyEnd::Failed);
        index.operations.insert(operation.clone(), ended);
        index.retain_result(operation, Retention::PersistenceFailed);
    }
}

#[hegel::test(seed = Some(29116), test_cases = 100)]
fn generated_known_prompt_failure_keeps_its_original_answer(case: hegel::TestCase) {
    let clock = ManualClock::new(chrono::DateTime::UNIX_EPOCH);
    let live = LiveOperation::pending();
    let decided = failure(
        ConversationOperationFailureKind::Unavailable,
        ConversationOperationFailureStage::Admission,
        ProviderOperationEffect::None,
        "fixture refusal",
        OperationId::generate(),
        None,
    );
    let budget = Duration::from_millis(case.draw(gs::integers::<u64>().min_value(1).max_value(20)));
    let deadline = clock.monotonic_now().after(budget);
    let answer = prompt_failure_answer(&clock, deadline, &live, decided.clone());
    tokio::pin!(answer);
    assert!(answer.as_mut().now_or_never().is_none());
    if case.draw(gs::booleans()) {
        let mut persisted = decided.clone();
        persisted.stage = ConversationOperationFailureStage::Settlement;
        *live.result.try_lock().unwrap() = Some(Err(persisted));
        live.finish_write(CustodyEnd::Failed);
        live.changed.notify_waiters();
    } else {
        clock.advance(budget).expect("advance to L");
    }
    assert_eq!(
        failure_wire(&answer.now_or_never().expect("answer is ready")),
        failure_wire(&decided),
        "I-h: the reply at or before L is the exact known answer"
    );
}
