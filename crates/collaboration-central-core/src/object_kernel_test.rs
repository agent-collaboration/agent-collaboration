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
