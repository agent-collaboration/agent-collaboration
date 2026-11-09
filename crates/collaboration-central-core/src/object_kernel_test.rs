use super::{KernelFace, ObjectKernel};
use crate::object_event::{EventReply, ObjectEvent};
use durable_steps::{CallFailure, StoreError};
use runtime_primitives::{SyncTime, WakeFailure};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

/// A face that records each pipeline step it is asked to run.
#[derive(Default)]
struct RecordingFace {
    steps: Mutex<Vec<String>>,
    now_seconds: AtomicI64,
    stored_alarm_seconds: Mutex<Option<i64>>,
    open_fails: AtomicBool,
    recompute_fails: AtomicBool,
}

impl RecordingFace {
    fn record(&self, step: impl Into<String>) {
        self.steps
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(step.into());
    }
    fn take_steps(&self) -> Vec<String> {
        std::mem::take(
            &mut *self
                .steps
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }
    fn at(&self, seconds: i64) {
        self.now_seconds.store(seconds, Ordering::SeqCst);
    }
}

fn time(seconds: i64) -> SyncTime {
    SyncTime::from_unix_millis(seconds * 1000)
}

impl KernelFace for RecordingFace {
    async fn stored_alarm(&self) -> Result<Option<SyncTime>, WakeFailure> {
        Ok(self
            .stored_alarm_seconds
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .map(time))
    }
    fn now(&self) -> SyncTime {
        time(self.now_seconds.load(Ordering::SeqCst))
    }
    async fn open(&self) -> Result<(), StoreError> {
        self.record("open");
        if self.open_fails.load(Ordering::SeqCst) {
            return Err(StoreError::backend("open fixture failure"));
        }
        Ok(())
    }
    async fn handle(&self, event: ObjectEvent) -> EventReply {
        match event {
            ObjectEvent::Call { body, .. } => {
                self.record("handle:call");
                EventReply::CallAnswer(body)
            }
            ObjectEvent::Alarm => {
                self.record("handle:alarm");
                EventReply::AlarmDone
            }
            ObjectEvent::Frame { .. }
            | ObjectEvent::SocketOpened { .. }
            | ObjectEvent::SocketClosed { .. } => {
                self.record("handle:socket");
                EventReply::None
            }
        }
    }
    async fn run_due_sources(&self, now: SyncTime) {
        self.record(format!("due@{}", now.unix_millis() / 1000));
    }
    async fn run_deadline_sources(&self, now: SyncTime) {
        self.record(format!("deadlines@{}", now.unix_millis() / 1000));
    }
    async fn drive_long_operations(&self) {
        self.record("long-operations");
    }
    async fn finish_event(&self, _owed: crate::EventOwed) {}
    async fn recompute(
        &self,
        next_batch_at: Option<SyncTime>,
    ) -> Result<Option<SyncTime>, StoreError> {
        self.record(format!(
            "recompute(next_batch={:?})",
            next_batch_at.map(|at| at.unix_millis() / 1000)
        ));
        if self.recompute_fails.load(Ordering::SeqCst) {
            return Err(StoreError::backend("recompute fixture failure"));
        }
        Ok(None)
    }
}

fn call() -> ObjectEvent {
    ObjectEvent::Call {
        body: b"call".to_vec(),
        storage_bytes: None,
    }
}

#[tokio::test]
async fn a_call_is_answered_without_running_the_due_batch() {
    // Arrange
    let kernel = ObjectKernel::new(RecordingFace::default());
    kernel.face().at(100);

    // Act
    let reply = kernel.on_event(call()).await;

    // Assert: the call's answer must not wait for due work (R4); the alarm is still recomputed.
    assert!(matches!(reply, EventReply::CallAnswer(body) if body == b"call"));
    assert_eq!(
        kernel.face().take_steps(),
        ["open", "handle:call", "recompute(next_batch=None)"]
    );
}

#[tokio::test]
async fn an_alarm_runs_due_sources_then_long_operations_then_recomputes() {
    // Arrange
    let kernel = ObjectKernel::new(RecordingFace::default());
    kernel.face().at(100);

    // Act
    let reply = kernel.on_event(ObjectEvent::Alarm).await;

    // Assert: long operations run last; the recompute knows the next batch slot.
    assert!(matches!(reply, EventReply::AlarmDone));
    assert_eq!(
        kernel.face().take_steps(),
        [
            "open",
            "handle:alarm",
            "due@100",
            "deadlines@100",
            "long-operations",
            "recompute(next_batch=Some(130))"
        ]
    );
}

#[tokio::test]
async fn a_second_alarm_inside_the_cadence_runs_no_batch() {
    // Arrange: a batch ran at 100 s.
    let kernel = ObjectKernel::new(RecordingFace::default());
    kernel.face().at(100);
    kernel.on_event(ObjectEvent::Alarm).await;
    kernel.face().take_steps();

    // Act: a duplicate alarm 10 s later.
    kernel.face().at(110);
    kernel.on_event(ObjectEvent::Alarm).await;

    // Assert (R3, J5): no second batch of due sources inside 30 s while the
    // object stays resident. Until slice 4 the engine still runs every event.
    assert_eq!(
        kernel.face().take_steps(),
        [
            "handle:alarm",
            "deadlines@110",
            "long-operations",
            "recompute(next_batch=Some(130))"
        ]
    );
}

#[tokio::test]
async fn a_failed_recompute_retries_an_alarm_but_still_answers_a_call() {
    // Arrange
    let kernel = ObjectKernel::new(RecordingFace::default());
    kernel.face().recompute_fails.store(true, Ordering::SeqCst);

    // Act
    let alarm = kernel.on_event(ObjectEvent::Alarm).await;
    let call = kernel.on_event(call()).await;

    // Assert: only the alarm event propagates the failure, so the platform redelivers it.
    assert!(matches!(alarm, EventReply::AlarmRetry));
    assert!(matches!(call, EventReply::CallAnswer(_)));
}

#[tokio::test]
async fn a_failed_open_makes_the_object_unavailable_and_is_retried_next_event() {
    // Arrange
    let kernel = ObjectKernel::new(RecordingFace::default());
    kernel.face().open_fails.store(true, Ordering::SeqCst);

    // Act
    let refused = kernel.on_event(call()).await;
    kernel.face().open_fails.store(false, Ordering::SeqCst);
    let answered = kernel.on_event(call()).await;

    // Assert
    assert!(matches!(
        refused,
        EventReply::CallFailure(CallFailure::AnswerLost)
    ));
    assert!(matches!(answered, EventReply::CallAnswer(_)));
    assert_eq!(
        kernel.face().take_steps(),
        ["open", "open", "handle:call", "recompute(next_batch=None)"]
    );
}

#[tokio::test]
async fn a_stale_stored_alarm_is_recomputed_before_the_event_is_handled() {
    // Arrange: the stored alarm is 50 s in the past (the Nov 5 shape).
    let kernel = ObjectKernel::new(RecordingFace::default());
    kernel.face().at(100);
    *kernel
        .face()
        .stored_alarm_seconds
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(50);

    // Act
    kernel.on_event(call()).await;

    // Assert (R5, J2): repair runs first, so a failing handler can't leave the stale alarm.
    assert_eq!(
        kernel.face().take_steps(),
        [
            "open",
            "recompute(next_batch=None)",
            "handle:call",
            "recompute(next_batch=None)"
        ]
    );
}

#[cfg(feature = "conformance")]
mod observed {
    use super::{RecordingFace, call};
    use crate::ObjectAddress;
    use crate::event_trace::{
        AlarmWriteOp, BarrierPoint, EventKindRecord, EventRecord, KernelObserver, ReplyRecord,
        UnitOutcomeRecord,
    };
    use crate::object_event::ObjectEvent;
    use crate::object_kernel::ObjectKernel;
    use durable_steps::{BoxFuture, Uuid};
    use std::sync::{Arc, Mutex, PoisonError};
    use tokio::sync::{Notify, oneshot};

    /// Collects records; optionally holds the first event that reaches `hold_at`.
    #[derive(Default)]
    struct Observer {
        records: Mutex<Vec<EventRecord>>,
        hold_at: Option<BarrierPoint>,
        release: Mutex<Option<oneshot::Receiver<()>>>,
        reached: Notify,
    }

    impl KernelObserver for Observer {
        fn object(&self) -> ObjectAddress {
            ObjectAddress::Fleet
        }
        fn construction(&self) -> Uuid {
            Uuid::nil()
        }
        fn emit(&self, record: EventRecord) {
            self.records
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(record);
        }
        fn barrier(
            &self,
            point: BarrierPoint,
            _unit_kind: Option<&'static str>,
            _event: Uuid,
        ) -> BoxFuture<'static, ()> {
            let release = if self.hold_at == Some(point) {
                self.release
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take()
            } else {
                None
            };
            let Some(release) = release else {
                return Box::pin(std::future::ready(()));
            };
            self.reached.notify_one();
            Box::pin(async move {
                let _ = release.await;
            })
        }
    }

    fn records(observer: &Observer) -> Vec<EventRecord> {
        observer
            .records
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    #[tokio::test]
    async fn each_event_emits_one_record_numbered_as_it_completes() {
        // Arrange
        let observer = Arc::new(Observer::default());
        let kernel = ObjectKernel::observed(RecordingFace::default(), observer.clone());
        kernel.face().at(100);

        // Act: an alarm that claims the batch, one inside the cadence, then a call.
        kernel.on_event(ObjectEvent::Alarm).await;
        kernel.face().at(110);
        kernel.on_event(ObjectEvent::Alarm).await;
        kernel.on_event(call()).await;

        // Assert
        let records = records(&observer);
        let sequences: Vec<_> = records.iter().map(|record| record.sequence).collect();
        assert_eq!(sequences, [0, 1, 2]);
        let first = &records[0];
        assert_eq!(
            first.event,
            EventKindRecord::Alarm {
                redelivery: None,
                delivery: crate::event_trace::AlarmDelivery::Platform,
            }
        );
        assert_eq!(first.reply, ReplyRecord::AlarmDone);
        assert_eq!(first.entered_at, 100_000);
        let batch = first
            .batch
            .as_ref()
            .map(|batch| (batch.claimed, batch.started_at));
        assert_eq!(batch, Some((true, Some(100_000))));
        assert_eq!(first.units[0].unit_kind, "handler");
        assert_eq!(first.units[0].outcome, UnitOutcomeRecord::Done);
        assert_eq!(first.alarm.writes.len(), 1);
        assert_eq!(first.alarm.writes[0].op, AlarmWriteOp::Exact);
        assert_eq!(
            first.alarm.writes[0].unit_kind.as_deref(),
            Some("recompute")
        );
        // The second alarm's claim attempt lost to the cadence.
        assert_eq!(
            records[1].batch.as_ref().map(|batch| batch.claimed),
            Some(false)
        );
        // A call never attempts the batch, and is recorded with its envelope kind (none here).
        assert_eq!(records[2].batch, None);
        assert_eq!(records[2].event, EventKindRecord::Call { call_kind: None });
        assert_ne!(records[0].event_id, records[1].event_id);
    }

    #[tokio::test]
    async fn a_before_claim_barrier_holds_the_event_until_it_is_released() {
        // Arrange
        let (release, released) = oneshot::channel();
        let observer = Arc::new(Observer {
            hold_at: Some(BarrierPoint::BeforeClaim),
            release: Mutex::new(Some(released)),
            ..Observer::default()
        });
        let kernel = Arc::new(ObjectKernel::observed(
            RecordingFace::default(),
            observer.clone(),
        ));
        kernel.face().at(100);
        let reached = observer.reached.notified();

        // Act
        let running = tokio::spawn({
            let kernel = kernel.clone();
            async move { kernel.on_event(ObjectEvent::Alarm).await }
        });
        reached.await;
        let while_held = kernel.face().take_steps();
        let _ = release.send(());
        let _ = running.await;

        // Assert: held after its handler, before any due source; resumes once released.
        assert_eq!(while_held, ["open", "handle:alarm"]);
        assert_eq!(
            kernel.face().take_steps(),
            [
                "due@100",
                "deadlines@100",
                "long-operations",
                "recompute(next_batch=Some(130))"
            ]
        );
        assert_eq!(records(&observer).len(), 1);
    }
}
