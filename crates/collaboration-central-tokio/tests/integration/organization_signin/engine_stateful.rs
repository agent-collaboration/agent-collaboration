//! The engine's Hegel stateful (model-based) test on the native host. Random
//! sequences of requests, alarm firings, clock advances across 5-minute marks,
//! injected faults, admin repair and restarts drive the real engine; the
//! invariants are checked after every step.
//!
//! hegeltest 0.52.6 `src/stateful.rs`: `#[hegel::state_machine]` turns `#[rule]`
//! methods into actions and `#[invariant(always_run)]` methods into checks run
//! after every rule; `hegel::stateful::machine(model).steps(n).run(tc)` runs it.
//! Rules are synchronous `fn(&mut self, TestCase)`, so the model owns one
//! current-thread Tokio runtime and blocks on it; the host's alarm driver is a
//! task on that runtime and only the injected `ManualClock` ever moves time.
//!
//! The model predicts, independently of the store, what every due-batch unit
//! did to each subject (fired, failed, not applied, stale) and the failure
//! record spec R4 then requires; the invariants compare the store exactly.
// Test-only module: clippy's test allowances (clippy.toml) apply to it.
#![cfg(test)]
use super::engine_admin_support::*;
use super::engine_stateful_attempts::{AttemptOutcome, ExpectedRecord};
use super::support::*;
use central_storage::SqlValue;
use collaboration_central_core::{
    EngineCommitFault, EngineCommitFaultKind,
    engine_probe::{
        EngineReceipt, EngineRequestOutcome, InstanceKey, ProbeEvent, ProbeRefusal, ProbeRequest,
        ProbeState, SeedProbe,
    },
};
use hegel::{TestCase, generators as gs};
use router_clock::RuntimeClock;
use workspace_model::central_mail::{CentralMailAnswer, CentralMailOperation, CentralMailResult};

const SPACING_MS: i64 = 30_000;
/// Scheduler turns `settle` gives the alarm driver to make visible progress.
/// A live driver needs a handful per store round trip; a stalled or dead one
/// never progresses, so the budget turns a hang into a failing case without
/// reading any clock.
const PROGRESS_TURNS: u32 = 5_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Wake,
    Probe,
}
impl Kind {
    fn machine(self) -> &'static str {
        match self {
            Self::Wake => "wake_timer",
            Self::Probe => "engine_probe",
        }
    }
}

/// What the test did to a subject; the engine's answers are read back from the
/// store and compared with what these facts allow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Fate {
    Live,
    /// Cancelled (wake) or stopped (probe).
    Finished,
    /// Discarded by admin while retired.
    Discarded,
}

/// A one-shot commit fault armed on one subject's next commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArmedFault {
    /// `DatabaseCode(19)`: the commit is rejected, a recorded failure.
    Rejected,
    AppliedAnswerLost,
    NotAppliedAnswerLost,
}

#[derive(Debug)]
struct Subject {
    kind: Kind,
    key: String,
    group: Option<String>,
    due: Time,
    /// When the subject last became due to the engine: its due time, the
    /// moment an admin retry handed it back (V13), or the batch whose commit
    /// answer was lost before it applied.
    eligible_at: Time,
    fate: Fate,
    /// The model's own record of whether the occurrence fired.
    fired: bool,
    /// The failure record R4 requires, predicted from the attempts.
    expected: ExpectedRecord,
    /// The original target while a corruption keeps the wake from decoding.
    corrupted_target: Option<SqlValue>,
}

struct EngineModel {
    runtime: tokio::runtime::Runtime,
    engine: Option<EngineOrg>,
    subjects: Vec<Subject>,
    /// Every due batch claimed in this process lifetime (the floor is in
    /// memory), one entry per batch even at the same millisecond.
    batch_starts: Vec<i64>,
    /// How many unit starts and batch claims of this runtime were observed.
    seen_units: usize,
    seen_batches: usize,
    /// The armed one-shot commit fault and its subject (arming replaces it).
    armed_fault: Option<(String, ArmedFault)>,
    /// Whether the armed fault was still pending when last observed.
    fault_pending: bool,
}

impl EngineModel {
    fn engine(&self) -> &EngineOrg {
        self.engine.as_ref().expect("engine open")
    }
    fn block<T>(&self, work: impl std::future::Future<Output = T>) -> T {
        self.runtime.block_on(work)
    }
    fn now_ms(&self) -> i64 {
        self.engine().now().timestamp_millis()
    }
    fn pick(&self, tc: &TestCase, eligible: impl Fn(&Subject) -> bool) -> Option<usize> {
        let indexes: Vec<usize> = (0..self.subjects.len())
            .filter(|index| eligible(&self.subjects[*index]))
            .collect();
        tc.assume(!indexes.is_empty());
        let choice = tc.draw(
            gs::integers::<usize>()
                .min_value(0)
                .max_value(indexes.len() - 1),
        );
        indexes.get(choice).copied()
    }
    fn commit_fault_pending(&self) -> bool {
        self.block(async {
            self.engine()
                .runtime()
                .await
                .statement_host()
                .engine_commit_fault_pending()
        })
    }
    fn arm(&mut self, key: String, fault: ArmedFault) {
        self.armed_fault = Some((key, fault));
        self.fault_pending = true;
    }
    /// A request on `key` used up the armed fault with its own commit.
    fn request_consumed_fault(&mut self, key: &str) {
        if self
            .armed_fault
            .as_ref()
            .is_some_and(|(armed, _)| armed == key)
            && !self.commit_fault_pending()
        {
            self.armed_fault = None;
            self.fault_pending = false;
        }
    }
    /// Lets the host's own driver finish everything due at the current time,
    /// then predicts what each new due-batch unit did.
    fn settle(&mut self) {
        let engine = self.engine();
        self.runtime.block_on(settle(engine));
        self.observe();
    }
    fn observe(&mut self) {
        let runtime = self.block(self.engine().runtime());
        let batches = runtime.due_batch_starts();
        self.batch_starts
            .extend_from_slice(&batches[self.seen_batches.min(batches.len())..]);
        self.seen_batches = batches.len();
        let units = runtime.due_unit_starts();
        let new_units = units[self.seen_units.min(units.len())..].to_vec();
        self.seen_units = units.len();
        let pending_now = runtime.statement_host().engine_commit_fault_pending();
        // The armed fault was used up during this step by one of its attempts.
        let mut consumed = self
            .armed_fault
            .clone()
            .filter(|_| self.fault_pending && !pending_now);
        for unit in new_units {
            let Some(index) = self.subjects.iter().position(|subject| {
                subject.kind.machine() == unit.machine_kind && subject.key == unit.subject_key
            }) else {
                continue;
            };
            let outcome = self.attempt_outcome(index, unit.batch_start_unix_millis, &mut consumed);
            let subject = &mut self.subjects[index];
            let earliest = subject
                .expected
                .earliest_batch_ms(&subject.key, subject.due.timestamp_millis());
            assert!(
                unit.batch_start_unix_millis >= earliest,
                "{} was worked by the batch at {} before {earliest} (due {})",
                subject.key,
                unit.batch_start_unix_millis,
                subject.due
            );
            let batch = unit.batch_start_unix_millis;
            match outcome {
                AttemptOutcome::Fired => {
                    subject.fired = true;
                    subject.expected = ExpectedRecord::None;
                }
                AttemptOutcome::Failed => {
                    subject.expected = subject.expected.after_failure(&subject.key, batch);
                }
                AttemptOutcome::NotApplied => {
                    subject.eligible_at = chrono::DateTime::from_timestamp_millis(batch).unwrap();
                }
                AttemptOutcome::Stale | AttemptOutcome::Skipped => {}
            }
        }
        if !pending_now {
            self.armed_fault = None;
        }
        self.fault_pending = pending_now && self.armed_fault.is_some();
    }
    fn attempt_outcome(
        &self,
        index: usize,
        batch_ms: i64,
        consumed: &mut Option<(String, ArmedFault)>,
    ) -> AttemptOutcome {
        let subject = &self.subjects[index];
        if subject.fate != Fate::Live || subject.fired {
            return AttemptOutcome::Stale;
        }
        // The gate runs before decoding or committing, so a skip uses no fault.
        if let ExpectedRecord::Failing { next_try_ms } = subject.expected
            && batch_ms < next_try_ms
        {
            return AttemptOutcome::Skipped;
        }
        // A corrupt row fails before its commit, so it never uses a fault.
        if subject.corrupted_target.is_some() {
            return AttemptOutcome::Failed;
        }
        match consumed.take_if(|(key, _)| *key == subject.key) {
            Some((_, ArmedFault::Rejected)) => AttemptOutcome::Failed,
            Some((_, ArmedFault::NotAppliedAnswerLost)) => AttemptOutcome::NotApplied,
            Some((_, ArmedFault::AppliedAnswerLost)) | None => AttemptOutcome::Fired,
        }
    }
    fn fired_in_store(&self, subject: &Subject) -> i64 {
        let engine = self.engine();
        match subject.kind {
            Kind::Wake => self.block(engine.wake_messages(&subject.key)),
            Kind::Probe => self.block(engine.count(
                "SELECT effect_count FROM engine_probe_groups WHERE group_id=?",
                vec![text(subject.group.as_deref().expect("probe group"))],
            )),
        }
    }
    /// A raw fixture statement on `key` would consume a commit fault armed on it.
    fn fault_armed_on(&self, key: &str) -> bool {
        self.armed_fault
            .as_ref()
            .is_some_and(|(armed, _)| armed == key)
            && self.commit_fault_pending()
    }
}

/// Polls `check` between scheduler turns; a driver that stops making progress
/// fails the case with `what` instead of hanging it.
async fn within_progress_budget<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    for _ in 0..PROGRESS_TURNS {
        if let Some(value) = check() {
            return value;
        }
        tokio::task::yield_now().await;
    }
    panic!("the alarm driver made no progress toward {what} in {PROGRESS_TURNS} scheduler turns");
}

/// Waits on the host's actual alarm driver: until it completes a due alarm, or
/// re-checks a future alarm at the current manual time. A failed, stalled or
/// dead driver fails the case.
async fn settle(engine: &EngineOrg) {
    let runtime = engine.runtime().await;
    assert!(
        runtime.alarm_failures().is_empty(),
        "the alarm driver failed: {:?}",
        runtime.alarm_failures()
    );
    for _ in 0..16 {
        let completed = runtime.alarm_completion_count();
        let failures = runtime.alarm_failures().len();
        let failed = || {
            panic!(
                "the alarm driver failed: {:?}; due batch {:?}",
                runtime.alarm_failures(),
                runtime.due_batch_snapshot()
            )
        };
        let now = engine.now().timestamp_millis();
        match runtime.statement_host().alarm() {
            Some(alarm) if alarm.unix_millis() <= now => {
                within_progress_budget("completing the due alarm", || {
                    if runtime.alarm_failures().len() > failures {
                        failed();
                    }
                    (runtime.alarm_completion_count() > completed).then_some(())
                })
                .await;
            }
            Some(alarm) => {
                let deadline = engine.f.clock.wall_deadline(alarm);
                within_progress_budget("re-checking the future alarm", || {
                    if runtime.alarm_failures().len() > failures {
                        failed();
                    }
                    engine.f.clock.has_not_due_check_now(deadline).then_some(())
                })
                .await;
                return;
            }
            None => return,
        }
    }
    panic!("the driver never settled: its alarm stays at or before now");
}

#[hegel::state_machine]
impl EngineModel {
    #[rule]
    fn create_wake(&mut self, tc: TestCase) {
        tc.assume(self.subjects.len() < 16);
        let marks = tc.draw(gs::integers::<i32>().min_value(0).max_value(11));
        let due = next_mark(self.engine().now()) + minutes(5 * i64::from(marks));
        let key = self.block(self.engine().wake(due, "stateful wake"));
        tc.note(&format!("wake {key} due {due}"));
        self.subjects.push(Subject {
            kind: Kind::Wake,
            key,
            group: None,
            due,
            eligible_at: due,
            fate: Fate::Live,
            fired: false,
            expected: ExpectedRecord::None,
            corrupted_target: None,
        });
        self.settle();
    }

    #[rule]
    fn create_probe(&mut self, tc: TestCase) {
        tc.assume(self.subjects.len() < 16);
        let seconds = tc.draw(gs::integers::<i64>().min_value(1).max_value(1_800));
        let due = self.engine().now() + chrono::Duration::seconds(seconds);
        // Minted from the held clock, never the system clock.
        let key = uuid::Uuid::parse_str(&self.engine().fresh()).unwrap();
        let group = uuid::Uuid::parse_str(&self.engine().fresh()).unwrap();
        let seeded = self.block(async {
            self.engine()
                .runtime()
                .await
                .seed_probe(SeedProbe {
                    key,
                    group,
                    due: Some(due),
                    // The probe table keeps completed claims unique.
                    claim: key.to_string(),
                })
                .await
                .unwrap()
        });
        assert!(matches!(
            seeded,
            central_storage::BatchOutcome::Applied { .. }
        ));
        tc.note(&format!("probe {key} due {due}"));
        self.subjects.push(Subject {
            kind: Kind::Probe,
            key: key.to_string(),
            group: Some(group.to_string()),
            due,
            eligible_at: due,
            fate: Fate::Live,
            fired: false,
            expected: ExpectedRecord::None,
            corrupted_target: None,
        });
        // The seed is not an event; the next event or alarm publishes it.
        self.block(self.engine().after_event());
        self.settle();
    }

    #[rule]
    fn cancel_wake(&mut self, tc: TestCase) {
        let Some(index) = self.pick(&tc, |s| {
            s.kind == Kind::Wake && s.fate == Fate::Live && s.corrupted_target.is_none()
        }) else {
            return;
        };
        let wake_id = self.subjects[index].key.clone();
        let answer = self.block(self.engine().central(CentralMailOperation::CancelWake {
            wake_id: wake_id.clone().try_into().unwrap(),
        }));
        tc.note(&format!("cancel wake {wake_id}: {answer:?}"));
        if matches!(
            answer,
            CentralMailResult::Done(CentralMailAnswer::WakeCancelled { .. })
        ) {
            // R4: a finished subject has no failure record.
            self.subjects[index].fate = Fate::Finished;
            self.subjects[index].expected = ExpectedRecord::None;
        } else {
            self.request_consumed_fault(&wake_id);
        }
        self.settle();
    }

    #[rule]
    fn stop_probe(&mut self, tc: TestCase) {
        let Some(index) = self.pick(&tc, |s| s.kind == Kind::Probe && s.fate == Fate::Live) else {
            return;
        };
        let loss = tc.draw(gs::integers::<u8>().min_value(0).max_value(2));
        let key = self.subjects[index].key.clone();
        let kind = match loss {
            1 => Some((
                EngineCommitFaultKind::AppliedAnswerLost,
                ArmedFault::AppliedAnswerLost,
            )),
            2 => Some((
                EngineCommitFaultKind::NotAppliedAnswerLost,
                ArmedFault::NotAppliedAnswerLost,
            )),
            _ => None,
        };
        if let Some((_, armed)) = kind {
            self.arm(key.clone(), armed);
        }
        let request_id = self.engine().fresh();
        let outcome = self.block(async {
            let runtime = self.engine().runtime().await;
            if let Some((kind, _)) = kind {
                runtime
                    .statement_host()
                    .inject_engine_commit_fault(EngineCommitFault {
                        key: InstanceKey(key.clone()),
                        kind,
                    });
            }
            request_probe_as_engine_admin(
                self.engine(),
                &runtime,
                ProbeRequest {
                    request_id: request_id.clone().try_into().unwrap(),
                    key: InstanceKey(key.clone()),
                    event: ProbeEvent::Stop,
                },
            )
            .await
        });
        tc.note(&format!("stop probe {key} (loss {loss}): {outcome:?}"));
        self.request_consumed_fault(&key);
        // The answer is stored with its effects, or neither.
        let stopped = self.block(self.engine().count(
            "SELECT COUNT(*) FROM engine_probe_items WHERE item_id=? AND state='stopped'",
            vec![text(&key)],
        )) == 1;
        let stored = self.block(self.engine().rows(
            "SELECT answer FROM runtime_request_log WHERE request_id=?",
            vec![text(&request_id)],
        ));
        let stored_refusal = match stored.first().map(|row| &row[0]) {
            None => None,
            Some(SqlValue::Text(answer)) => Some(answer.contains("\"refused\"")),
            Some(other) => panic!("stored answer {other:?}"),
        };
        // The stop and its stored answer commit together; a stored answer
        // without the stop is a refusal, which has no effect.
        match (stopped, stored_refusal) {
            (true, Some(false)) | (false, Some(true) | None) => {}
            other => panic!("stop effect and stored answer disagree: {other:?} for {outcome:?}"),
        }
        match &outcome {
            EngineRequestOutcome::Answer {
                receipt: EngineReceipt::Applied { .. },
            } => assert!(stopped, "an applied stop has its effect"),
            EngineRequestOutcome::Answer {
                receipt: EngineReceipt::Refused { .. },
            } => assert_eq!(stored_refusal, Some(true), "a refusal is stored"),
            _ => {}
        }
        if stopped {
            self.subjects[index].fate = Fate::Finished;
            self.subjects[index].expected = ExpectedRecord::None;
        }
        self.settle();
    }

    #[rule]
    fn fault_next_commit(&mut self, tc: TestCase) {
        let Some(index) = self.pick(&tc, |s| s.fate == Fate::Live) else {
            return;
        };
        let key = self.subjects[index].key.clone();
        tc.note(&format!("reject the next commit of {key}"));
        self.arm(key.clone(), ArmedFault::Rejected);
        self.block(async {
            self.engine()
                .runtime()
                .await
                .statement_host()
                .inject_engine_commit_fault(EngineCommitFault {
                    key: InstanceKey(key),
                    kind: EngineCommitFaultKind::DatabaseCode(19),
                });
        });
    }

    #[rule(weight = 2.0)]
    fn corrupt_wake(&mut self, tc: TestCase) {
        let Some(index) = self.pick(&tc, |s| {
            s.kind == Kind::Wake && s.fate == Fate::Live && s.corrupted_target.is_none()
        }) else {
            return;
        };
        let key = self.subjects[index].key.clone();
        tc.assume(!self.subjects[index].fired);
        tc.assume(!self.fault_armed_on(&key));
        let original = self.block(self.engine().rows(
            "SELECT target FROM automation_wakes WHERE wake_id=?",
            vec![text(&key)],
        ))[0][0]
            .clone();
        self.block(self.engine().raw(
            "UPDATE automation_wakes SET target='not-json' WHERE wake_id=?",
            vec![text(&key)],
        ));
        tc.note(&format!("corrupt wake {key}"));
        self.subjects[index].corrupted_target = Some(original);
    }

    #[rule(weight = 3.0)]
    fn repair_retired(&mut self, tc: TestCase) {
        // The model's own expectation picks the subject, not the store's record.
        // The admin routes hide the test machine's kind (P8 contract).
        let index = self.pick(&tc, |s| {
            s.kind == Kind::Wake && s.fate == Fate::Live && s.expected == ExpectedRecord::Retired
        });
        let Some(index) = index else {
            return;
        };
        let (kind, key) = (self.subjects[index].kind, self.subjects[index].key.clone());
        tc.assume(!self.fault_armed_on(&key));
        let attempt = self.block(self.engine().attempt_id(kind.machine(), &key));
        if tc.draw(gs::booleans()) {
            if let Some(original) = self.subjects[index].corrupted_target.take() {
                self.block(self.engine().raw(
                    "UPDATE automation_wakes SET target=? WHERE wake_id=?",
                    vec![original, text(&key)],
                ));
            }
            let answer = self.block(self.engine().retry(&attempt));
            self.subjects[index].eligible_at = self.engine().now();
            self.subjects[index].expected = ExpectedRecord::None;
            tc.note(&format!("retry {key}: {answer}"));
            assert_eq!(answer, serde_json::json!({"result":"engineRetried"}));
        } else {
            let answer = self.block(self.engine().discard(&attempt));
            tc.note(&format!("discard {key}: {answer}"));
            assert_eq!(answer, serde_json::json!({"result":"engineDiscarded"}));
            self.subjects[index].fate = Fate::Discarded;
            self.subjects[index].expected = ExpectedRecord::None;
        }
        self.settle();
    }

    #[rule]
    fn fire_alarm_event(&mut self, _: TestCase) {
        self.block(self.engine().after_event());
        self.settle();
    }

    #[rule]
    fn advance_seconds(&mut self, tc: TestCase) {
        let seconds = tc.draw(gs::integers::<u64>().min_value(1).max_value(120));
        let clock = self.engine().f.clock.clone();
        self.block(clock.advance(std::time::Duration::from_secs(seconds)))
            .unwrap();
        self.settle();
    }

    #[rule(weight = 2.0)]
    fn advance_minutes(&mut self, tc: TestCase) {
        let minutes = tc.draw(gs::integers::<u64>().min_value(5).max_value(120));
        let clock = self.engine().f.clock.clone();
        self.block(clock.advance(std::time::Duration::from_secs(minutes * 60)))
            .unwrap();
        self.settle();
    }

    #[rule(weight = 0.3)]
    fn restart(&mut self, _: TestCase) {
        let mut engine = self.engine.take().expect("engine open");
        self.runtime.block_on(engine.f.reopen()).unwrap();
        self.engine = Some(engine);
        // A new process: the floor, the receipts and armed faults are in memory.
        self.batch_starts.clear();
        self.seen_units = 0;
        self.seen_batches = 0;
        self.armed_fault = None;
        self.fault_pending = false;
        self.block(self.engine().after_event());
        self.settle();
    }

    #[invariant(always_run)]
    fn pending_work_keeps_a_future_alarm(&self, _: TestCase) {
        let now = self.now_ms();
        let pending = self.subjects.iter().any(|subject| {
            subject.fate == Fate::Live
                && !subject.fired
                && subject.expected != ExpectedRecord::Retired
        });
        let alarm = self.block(self.engine().alarm_ms());
        if pending {
            assert!(
                alarm.is_some_and(|alarm| alarm > now),
                "pending timed work needs a physical alarm after now {now}: {alarm:?}"
            );
        }
    }

    #[invariant(always_run)]
    fn every_occurrence_fires_once_and_none_is_lost(&self, _: TestCase) {
        let now = self.now_ms();
        for subject in &self.subjects {
            let fired = self.fired_in_store(subject);
            assert_eq!(
                fired,
                i64::from(subject.fired),
                "{} fired {fired} times; the model expects {}",
                subject.key,
                i64::from(subject.fired)
            );
            if subject.fate != Fate::Live || subject.fired {
                continue;
            }
            // When the next due batch must have worked it, unless retired.
            let deadline = match subject.expected {
                ExpectedRecord::None => Some(subject.eligible_at.timestamp_millis()),
                ExpectedRecord::Failing { next_try_ms } => Some(next_try_ms),
                ExpectedRecord::Retired => None,
            };
            if let Some(deadline) = deadline {
                assert!(
                    deadline + SPACING_MS > now,
                    "{} due {} ({:?}) was never worked by {now}",
                    subject.key,
                    subject.due,
                    subject.expected
                );
            }
        }
    }

    #[invariant(always_run)]
    fn failure_records_follow_the_retirement_lifecycle(&self, _: TestCase) {
        for subject in &self.subjects {
            let stored = self.block(self.engine().record(subject.kind.machine(), &subject.key));
            subject.expected.assert_stored(&subject.key, &stored);
        }
        let rows = self.block(self.engine().rows(
            "SELECT machine_kind,subject_key FROM engine_work_attempts",
            vec![],
        ));
        for row in rows {
            let [SqlValue::Text(kind), SqlValue::Text(key)] = row.as_slice() else {
                panic!("failure record shape {row:?}")
            };
            assert!(
                self.subjects
                    .iter()
                    .any(|subject| subject.kind.machine() == kind && &subject.key == key),
                "a failure record for no subject of this test: {kind} {key}"
            );
        }
    }

    #[invariant(always_run)]
    fn the_thirty_second_floor_holds(&self, _: TestCase) {
        let mut starts = self.batch_starts.clone();
        starts.sort_unstable();
        for pair in starts.windows(2) {
            assert!(
                pair[1] - pair[0] >= SPACING_MS,
                "due batches started {} ms apart: {pair:?}",
                pair[1] - pair[0]
            );
        }
        let snapshot = self.block(self.engine().runtime()).due_batch_snapshot();
        if let (Some(alarm), Some(start)) = (
            self.block(self.engine().alarm_ms()),
            snapshot.last_start_unix_millis,
        ) {
            assert!(
                alarm >= start + SPACING_MS,
                "the alarm {alarm} is inside the floor after the batch at {start}"
            );
        }
    }
}

impl Drop for EngineModel {
    fn drop(&mut self) {
        if let Some(engine) = self.engine.take() {
            if std::thread::panicking() {
                // A failed case may have a stalled driver that close would wait
                // on forever; dropping the runtime cancels its tasks instead.
                std::mem::forget(engine);
                return;
            }
            self.runtime.block_on(engine.close());
        }
    }
}

async fn request_probe_as_engine_admin(
    engine: &EngineOrg,
    runtime: &Runtime,
    request: ProbeRequest,
) -> EngineRequestOutcome<ProbeState, ProbeRefusal> {
    let caller = engine.f.machine_caller("engine-admin").await.unwrap();
    runtime.request_probe(&caller, request).await.unwrap()
}

#[tokio::test]
async fn probe_stop_uses_fresh_authority_after_the_fixture_clock_advances() {
    let engine = EngineOrg::new().await;
    let runtime = engine.runtime().await;
    let key = engine.fresh().parse().unwrap();
    let seeded = runtime
        .seed_probe(SeedProbe {
            key,
            group: engine.fresh().parse().unwrap(),
            due: None,
            claim: key.to_string(),
        })
        .await
        .unwrap();
    assert!(matches!(
        seeded,
        central_storage::BatchOutcome::Applied { .. }
    ));
    engine
        .set(engine.now() + chrono::Duration::minutes(10))
        .await;
    let outcome = request_probe_as_engine_admin(
        &engine,
        &runtime,
        ProbeRequest {
            request_id: engine.fresh().try_into().unwrap(),
            key: InstanceKey(key.to_string()),
            event: ProbeEvent::Stop,
        },
    )
    .await;
    assert!(
        matches!(
            outcome,
            EngineRequestOutcome::Answer {
                receipt: EngineReceipt::Applied { .. }
            }
        ),
        "{outcome:?}",
    );
    engine.f.close().await.unwrap();
}

#[hegel::test(test_cases = 50)]
fn the_engine_keeps_its_invariants_under_random_requests_faults_and_restarts(tc: TestCase) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let engine = runtime.block_on(EngineOrg::new());
    let model = EngineModel {
        runtime,
        engine: Some(engine),
        subjects: Vec::new(),
        batch_starts: Vec::new(),
        seen_units: 0,
        seen_batches: 0,
        armed_fault: None,
        fault_pending: false,
    };
    hegel::stateful::machine(model).steps(50).run(tc);
}
