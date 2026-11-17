//! The engine's Hegel stateful (model-based) test on the native host. Random
//! sequences of requests, alarm firings, clock advances across 5-minute marks,
//! injected faults, admin repair and restarts drive the real engine; the five
//! invariants are checked after every step.
//!
//! hegeltest 0.52.6 `src/stateful.rs`: `#[hegel::state_machine]` turns `#[rule]`
//! methods into actions and `#[invariant(always_run)]` methods into checks run
//! after every rule; `hegel::stateful::machine(model).steps(n).run(tc)` runs it.
//! Rules are synchronous `fn(&mut self, TestCase)`, so the model owns one
//! current-thread Tokio runtime and blocks on it; the host's alarm driver is a
//! task on that runtime and only the injected `ManualClock` ever moves time.
// Test-only module: clippy's test allowances (clippy.toml) apply to it.
#![cfg(test)]
use super::engine_admin_support::*;
use super::support::*;
use central_storage::SqlValue;
use collaboration_central_core::{
    EngineCommitFault, EngineCommitFaultKind,
    engine_probe::{
        EngineReceipt, EngineRequestOutcome, InstanceKey, ProbeEvent, ProbeRequest, SeedProbe,
    },
};
use hegel::{TestCase, generators as gs};
use router_clock::RuntimeClock;
use workspace_model::central_mail::{CentralMailAnswer, CentralMailOperation, CentralMailResult};

const SPACING_MS: i64 = 30_000;
const MARK_MS: i64 = 300_000;

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
    /// Cancelled (wake) or stopped (probe) after this many firings.
    Finished {
        fired: i64,
    },
    /// Discarded by admin while retired.
    Discarded,
}

#[derive(Debug)]
struct Subject {
    kind: Kind,
    key: String,
    group: Option<String>,
    due: Time,
    /// When the subject last became due to the engine: its due time, or the
    /// moment an admin retry handed it back (V13: the next due batch works it).
    eligible_at: Time,
    fate: Fate,
    /// The original target while a corruption keeps the wake from decoding.
    corrupted_target: Option<SqlValue>,
}

struct EngineModel {
    runtime: tokio::runtime::Runtime,
    engine: Option<EngineOrg>,
    subjects: Vec<Subject>,
    /// Batch starts seen in this process lifetime (the floor is in memory).
    batch_starts: Vec<i64>,
    /// The key of the last armed one-shot commit fault (arming replaces it).
    armed_fault: Option<String>,
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
    /// Lets the host's own driver finish everything due at the current time.
    fn settle(&mut self) {
        let engine = self.engine();
        self.runtime.block_on(settle(engine));
        let starts = self.block(engine.runtime()).due_unit_starts();
        for start in starts {
            if !self.batch_starts.contains(&start.batch_start_unix_millis) {
                self.batch_starts.push(start.batch_start_unix_millis);
            }
        }
    }
    fn fired(&self, subject: &Subject) -> i64 {
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
        self.armed_fault.as_deref() == Some(key)
            && self.block(async {
                self.engine()
                    .runtime()
                    .await
                    .statement_host()
                    .engine_commit_fault_pending()
            })
    }
    fn record(&self, subject: &Subject) -> Option<(String, i64, Option<i64>)> {
        let rows = self.block(self.engine().record(subject.kind.machine(), &subject.key));
        rows.first().map(|row| {
            let state = match &row[0] {
                SqlValue::Text(state) => state.clone(),
                other => panic!("record state {other:?}"),
            };
            let count = match &row[1] {
                SqlValue::Integer(count) => count.get(),
                other => panic!("record count {other:?}"),
            };
            let next_try = match &row[2] {
                SqlValue::Text(at) => Some(at.parse::<Time>().unwrap().timestamp_millis()),
                SqlValue::Null => None,
                other => panic!("record next_try_at {other:?}"),
            };
            (state, count, next_try)
        })
    }
}

/// Waits on the host's actual alarm driver: until it completes a due alarm, or
/// re-checks a future alarm at the current manual time. A failed drive parks the
/// driver on a next-mark retry, so it fails the case instead of hanging.
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
        let now = engine.now().timestamp_millis();
        match runtime.statement_host().alarm() {
            Some(alarm) if alarm.unix_millis() <= now => tokio::select! {
                () = runtime.until_alarm_completed(completed + 1) => {}
                () = runtime.until_alarm_failed(failures + 1) => panic!(
                    "the alarm driver failed: {:?}; due batch {:?}",
                    runtime.alarm_failures(),
                    runtime.due_batch_snapshot()
                ),
            },
            Some(alarm) => {
                engine
                    .f
                    .clock
                    .not_due_check_now(engine.f.clock.wall_deadline(alarm))
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
            corrupted_target: None,
        });
        self.settle();
    }

    #[rule]
    fn create_probe(&mut self, tc: TestCase) {
        tc.assume(self.subjects.len() < 16);
        let seconds = tc.draw(gs::integers::<i64>().min_value(1).max_value(1_800));
        let due = self.engine().now() + chrono::Duration::seconds(seconds);
        let key = uuid::Uuid::now_v7();
        let group = uuid::Uuid::now_v7();
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
        let fired = self.fired(&self.subjects[index]);
        let wake_id = self.subjects[index].key.clone();
        let answer = self.block(self.engine().central(CentralMailOperation::CancelWake {
            wake_id: wake_id.clone().try_into().unwrap(),
        }));
        tc.note(&format!("cancel wake {wake_id}: {answer:?}"));
        if matches!(
            answer,
            CentralMailResult::Done(CentralMailAnswer::WakeCancelled { .. })
        ) {
            self.subjects[index].fate = Fate::Finished { fired };
        }
        self.settle();
    }

    #[rule]
    fn stop_probe(&mut self, tc: TestCase) {
        let Some(index) = self.pick(&tc, |s| s.kind == Kind::Probe && s.fate == Fate::Live) else {
            return;
        };
        let loss = tc.draw(gs::integers::<u8>().min_value(0).max_value(2));
        let fired = self.fired(&self.subjects[index]);
        let key = self.subjects[index].key.clone();
        if loss != 0 {
            self.armed_fault = Some(key.clone());
        }
        let request_id = self.engine().fresh();
        let outcome = self.block(async {
            let runtime = self.engine().runtime().await;
            let kind = match loss {
                1 => Some(EngineCommitFaultKind::AppliedAnswerLost),
                2 => Some(EngineCommitFaultKind::NotAppliedAnswerLost),
                _ => None,
            };
            if let Some(kind) = kind {
                runtime
                    .statement_host()
                    .inject_engine_commit_fault(EngineCommitFault {
                        key: InstanceKey(key.clone()),
                        kind,
                    });
            }
            runtime
                .request_probe(
                    &self.engine().caller,
                    ProbeRequest {
                        request_id: request_id.clone().try_into().unwrap(),
                        key: InstanceKey(key.clone()),
                        event: ProbeEvent::Stop,
                    },
                )
                .await
                .unwrap()
        });
        tc.note(&format!("stop probe {key} (loss {loss}): {outcome:?}"));
        // Invariant 3: the answer is stored with its effects, or neither.
        let stopped = self.block(self.engine().count(
            "SELECT COUNT(*) FROM engine_probe_items WHERE item_id=? AND state='stopped'",
            vec![text(&key)],
        )) == 1;
        let stored = self.block(self.engine().rows(
            "SELECT answer FROM request_log WHERE request_id=?",
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
            self.subjects[index].fate = Fate::Finished { fired };
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
        self.armed_fault = Some(key.clone());
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
        tc.assume(self.fired(&self.subjects[index]) == 0);
        tc.assume(!self.fault_armed_on(&key));
        let original = self.block(
            self.engine()
                .rows("SELECT target FROM wakes WHERE wake_id=?", vec![text(&key)]),
        )[0][0]
            .clone();
        self.block(self.engine().raw(
            "UPDATE wakes SET target='not-json' WHERE wake_id=?",
            vec![text(&key)],
        ));
        tc.note(&format!("corrupt wake {key}"));
        self.subjects[index].corrupted_target = Some(original);
    }

    #[rule(weight = 3.0)]
    fn repair_retired(&mut self, tc: TestCase) {
        let retired: Vec<usize> = (0..self.subjects.len())
            // The admin routes hide the test machine's kind (P8 contract).
            .filter(|index| {
                self.subjects[*index].kind == Kind::Wake
                    && self.subjects[*index].fate == Fate::Live
                    && self
                        .record(&self.subjects[*index])
                        .is_some_and(|(state, ..)| state == "retired")
            })
            .collect();
        tc.assume(!retired.is_empty());
        let index = retired[tc.draw(
            gs::integers::<usize>()
                .min_value(0)
                .max_value(retired.len() - 1),
        )];
        let (kind, key) = (self.subjects[index].kind, self.subjects[index].key.clone());
        tc.assume(!self.fault_armed_on(&key));
        let attempt = self.block(self.engine().attempt_id(kind.machine(), &key));
        if tc.draw(gs::booleans()) {
            if let Some(original) = self.subjects[index].corrupted_target.take() {
                self.block(self.engine().raw(
                    "UPDATE wakes SET target=? WHERE wake_id=?",
                    vec![original, text(&key)],
                ));
            }
            let answer = self.block(self.engine().retry(&attempt));
            self.subjects[index].eligible_at = self.engine().now();
            tc.note(&format!("retry {key}: {answer}"));
            assert_eq!(answer, serde_json::json!({"result":"engineRetried"}));
        } else {
            let answer = self.block(self.engine().discard(&attempt));
            tc.note(&format!("discard {key}: {answer}"));
            assert_eq!(answer, serde_json::json!({"result":"engineDiscarded"}));
            self.subjects[index].fate = Fate::Discarded;
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
        self.engine()
            .f
            .clock
            .advance(std::time::Duration::from_secs(seconds))
            .unwrap();
        self.settle();
    }

    #[rule(weight = 2.0)]
    fn advance_minutes(&mut self, tc: TestCase) {
        let minutes = tc.draw(gs::integers::<u64>().min_value(5).max_value(120));
        self.engine()
            .f
            .clock
            .advance(std::time::Duration::from_secs(minutes * 60))
            .unwrap();
        self.settle();
    }

    #[rule(weight = 0.3)]
    fn restart(&mut self, _: TestCase) {
        let mut engine = self.engine.take().expect("engine open");
        self.runtime.block_on(engine.f.reopen()).unwrap();
        self.engine = Some(engine);
        self.batch_starts.clear();
        self.block(self.engine().after_event());
        self.settle();
    }

    #[invariant(always_run)]
    fn pending_work_keeps_a_future_alarm(&self, _: TestCase) {
        let now = self.now_ms();
        let pending = self.subjects.iter().any(|subject| {
            subject.fate == Fate::Live
                && self.fired(subject) == 0
                && !self
                    .record(subject)
                    .is_some_and(|(state, ..)| state == "retired")
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
            let fired = self.fired(subject);
            assert!(fired <= 1, "{} fired {fired} times", subject.key);
            let record = self.record(subject);
            match subject.fate {
                Fate::Finished { fired: before } => {
                    assert_eq!(fired, before, "{} fired after it was finished", subject.key);
                }
                Fate::Discarded => assert_eq!(fired, 0, "{} fired after discard", subject.key),
                Fate::Live => {
                    let overdue = subject.eligible_at.timestamp_millis() + SPACING_MS <= now;
                    if overdue && record.is_none() && subject.corrupted_target.is_none() {
                        assert_eq!(fired, 1, "{} due {} was lost", subject.key, subject.due);
                    }
                }
            }
        }
    }

    #[invariant(always_run)]
    fn failure_records_match_the_subjects_and_retry_on_marks(&self, _: TestCase) {
        let rows = self.block(self.engine().rows(
            "SELECT machine_kind,subject_key,state,failure_count,next_try_at FROM central_work_attempts",
            vec![],
        ));
        for row in rows {
            let [
                SqlValue::Text(kind),
                SqlValue::Text(key),
                SqlValue::Text(state),
                SqlValue::Integer(count),
                next,
            ] = row.as_slice()
            else {
                panic!("failure record shape {row:?}")
            };
            assert!(
                self.subjects
                    .iter()
                    .any(|subject| subject.kind.machine() == kind && &subject.key == key),
                "a record for a subject the test never faulted: {kind} {key}"
            );
            assert!(count.get() >= 1);
            match (state.as_str(), next) {
                ("failing", SqlValue::Text(at)) => {
                    let at = at.parse::<Time>().unwrap().timestamp_millis();
                    assert_eq!(at % MARK_MS, 0, "{key} retries off a 5-minute mark: {at}");
                }
                ("retired", SqlValue::Null) => {}
                other => panic!("record {key} in state {other:?}"),
            }
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
            self.runtime.block_on(engine.close());
        }
    }
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
        armed_fault: None,
    };
    hegel::stateful::machine(model).steps(50).run(tc);
}
