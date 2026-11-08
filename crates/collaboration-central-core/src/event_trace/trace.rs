//! One event's trace and the poll-scoped context that lets a shell find it.

use super::{
    AlarmRecord, AlarmWriteOp, AlarmWriteRecord, BarrierPoint, BatchRecord, CallOutcomeRecord,
    CallRecord, EventKindRecord, EventRecord, PipelineStep, ReplyRecord, SendRecord, UnitRecord,
};
use crate::ObjectAddress;
use durable_steps::{BoxFuture, CallFailure, Uuid};
use fleet_protocol::ConnectionId;
use runtime_primitives::SyncTime;
use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};

/// The shell's side of the record: where records go and how barriers hold.
pub trait KernelObserver: Send + Sync + 'static {
    fn object(&self) -> ObjectAddress;
    /// The kernel instance this observer was built for.
    fn construction(&self) -> Uuid;
    fn emit(&self, record: EventRecord);
    /// Resolves at once unless an armed barrier captures this event here.
    fn barrier(
        &self,
        point: BarrierPoint,
        unit_kind: Option<&'static str>,
        event: Uuid,
    ) -> BoxFuture<'static, ()>;
}

/// What the shell knows about an event that the event itself doesn't carry.
#[derive(Clone, Copy, Debug, Default)]
pub struct EventContext {
    /// How many times the platform already delivered this alarm; null if it can't say.
    pub redelivery: Option<u32>,
}

pub(crate) struct EventTrace {
    observer: Arc<dyn KernelObserver>,
    event_id: Uuid,
    state: Mutex<TraceState>,
}

struct TraceState {
    step: PipelineStep,
    units: Vec<UnitRecord>,
    sends: Vec<SendRecord>,
    calls: Vec<CallRecord>,
    writes: Vec<AlarmWriteRecord>,
    batch: Option<BatchRecord>,
}

/// Everything the kernel measured around the event itself.
pub(crate) struct EventBounds {
    pub(crate) sequence: u64,
    pub(crate) event: EventKindRecord,
    pub(crate) entered_at: SyncTime,
    pub(crate) exited_at: SyncTime,
    pub(crate) alarm_before: Option<SyncTime>,
    pub(crate) alarm_after: Option<SyncTime>,
    pub(crate) reply: ReplyRecord,
}

impl EventTrace {
    pub(crate) fn new(observer: Arc<dyn KernelObserver>) -> Self {
        Self {
            observer,
            event_id: Uuid::now_v7(),
            state: Mutex::new(TraceState {
                step: PipelineStep::Open,
                units: Vec::new(),
                sends: Vec::new(),
                calls: Vec::new(),
                writes: Vec::new(),
                batch: None,
            }),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, TraceState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn step(&self, step: PipelineStep) {
        self.state().step = step;
    }

    pub(crate) fn unit(&self, unit: UnitRecord) {
        self.state().units.push(unit);
    }

    pub(crate) fn batch(&self, batch: BatchRecord) {
        self.state().batch = Some(batch);
    }

    pub(crate) fn exact(&self, at: Option<SyncTime>) {
        let mut state = self.state();
        let unit_kind = Some(state.step.unit_kind().to_owned());
        state.writes.push(AlarmWriteRecord {
            op: AlarmWriteOp::Exact,
            at: at.map(SyncTime::unix_millis),
            unit_kind,
        });
    }

    pub(crate) fn barrier(
        &self,
        point: BarrierPoint,
        unit_kind: Option<&'static str>,
    ) -> BoxFuture<'static, ()> {
        self.observer.barrier(point, unit_kind, self.event_id)
    }

    pub(crate) fn finish(&self, bounds: EventBounds) {
        let mut state = self.state();
        let record = EventRecord {
            object: self.observer.object(),
            construction: self.observer.construction(),
            sequence: bounds.sequence,
            event_id: self.event_id,
            event: bounds.event,
            entered_at: bounds.entered_at.unix_millis(),
            exited_at: bounds.exited_at.unix_millis(),
            units: std::mem::take(&mut state.units),
            sends: std::mem::take(&mut state.sends),
            notices: Vec::new(),
            calls: std::mem::take(&mut state.calls),
            alarm: AlarmRecord {
                before: bounds.alarm_before.map(SyncTime::unix_millis),
                after: bounds.alarm_after.map(SyncTime::unix_millis),
                writes: std::mem::take(&mut state.writes),
            },
            batch: state.batch.take(),
            reply: bounds.reply,
        };
        drop(state);
        self.observer.emit(record);
    }

    fn unit_kind(&self) -> String {
        self.state().step.unit_kind().to_owned()
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Arc<EventTrace>>> = const { RefCell::new(None) };
}

fn current() -> Option<Arc<EventTrace>> {
    CURRENT.with(|current| current.borrow().clone())
}

/// Runs `inner` with `trace` as the current event for every poll. Nested
/// scopes (an object call served inline by another object) restore the outer
/// event when they return.
pub(crate) fn scoped<F: Future>(trace: Arc<EventTrace>, inner: F) -> Scoped<F> {
    Scoped {
        trace,
        inner: Box::pin(inner),
    }
}

pub(crate) struct Scoped<F> {
    trace: Arc<EventTrace>,
    inner: Pin<Box<F>>,
}

impl<F: Future> Future for Scoped<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let previous = CURRENT.with(|current| current.replace(Some(this.trace.clone())));
        let _restore = RestoreOuter(previous);
        this.inner.as_mut().poll(context)
    }
}

struct RestoreOuter(Option<Arc<EventTrace>>);

impl Drop for RestoreOuter {
    fn drop(&mut self) {
        let outer = self.0.take();
        CURRENT.with(|current| *current.borrow_mut() = outer);
    }
}

/// A frame the shell put on a socket for the running event.
pub fn record_send(connection: ConnectionId, frame: &[u8]) {
    let Some(trace) = current() else {
        return;
    };
    trace.state().sends.push(SendRecord {
        attempt: None,
        attempt_kind: None,
        subject: None,
        send_id: None,
        connection,
        session: None,
        request_kind: top_level_kind(frame),
        sent_version: None,
        batch: None,
        frame: frame.to_vec(),
    });
}

/// A unit lowered the alarm to `at` for the running event. While long
/// operations run, an `afterStepLower` barrier may hold the event here.
pub async fn record_lower(at: SyncTime) {
    let Some(trace) = current() else {
        return;
    };
    let step = {
        let mut state = trace.state();
        let step = state.step;
        state.writes.push(AlarmWriteRecord {
            op: AlarmWriteOp::Lower,
            at: Some(at.unix_millis()),
            unit_kind: Some(step.unit_kind().to_owned()),
        });
        step
    };
    if step == PipelineStep::LongOperations {
        trace
            .barrier(BarrierPoint::AfterStepLower, Some(step.unit_kind()))
            .await;
    }
}

/// Runs one object call and records its outcome against the running event. A
/// call the event stopped waiting for is recorded as `deadlineExceeded`.
pub async fn traced_call<F>(
    to: &ObjectAddress,
    body: &[u8],
    call: F,
) -> Result<Vec<u8>, CallFailure>
where
    F: Future<Output = Result<Vec<u8>, CallFailure>>,
{
    let Some(trace) = current() else {
        return call.await;
    };
    let mut pending = PendingCall {
        record: Some(CallRecord {
            to: to.clone(),
            call_kind: top_level_kind(body),
            unit_kind: Some(trace.unit_kind()),
            subject: None,
            outcome: CallOutcomeRecord::DeadlineExceeded,
        }),
        trace,
    };
    let answer = call.await;
    pending.settle(if answer.is_ok() {
        CallOutcomeRecord::Answered
    } else {
        CallOutcomeRecord::Failed
    });
    answer
}

struct PendingCall {
    trace: Arc<EventTrace>,
    record: Option<CallRecord>,
}

impl PendingCall {
    fn settle(&mut self, outcome: CallOutcomeRecord) {
        if let Some(record) = self.record.as_mut() {
            record.outcome = outcome;
        }
    }
}

impl Drop for PendingCall {
    fn drop(&mut self) {
        if let Some(record) = self.record.take() {
            self.trace.state().calls.push(record);
        }
    }
}

/// The `kind` of a JSON object body, which names calls and protocol frames.
fn top_level_kind(bytes: &[u8]) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Kinded {
        kind: String,
    }
    serde_json::from_slice::<Kinded>(bytes)
        .ok()
        .map(|kinded| kinded.kind)
}
