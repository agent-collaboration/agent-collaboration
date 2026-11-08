//! One entry for every event an object receives, the same on both shells.
//!
//! Every event opens the object once per construction, repairs a stale alarm,
//! handles its own work, runs the due batch when the cadence allows (frames and
//! alarms only), and recomputes the alarm. Nothing the object needs lives only
//! in memory: the claim and the opened flag are hints whose loss costs at most
//! one extra batch or one repeated open.

use crate::alarm_scheduler::{BatchClaim, ClaimDeclined};
use crate::event_trace::{BarrierPoint, PipelineStep};
use crate::object_event::{EventReply, ObjectEvent};
use durable_steps::{CallFailure, StoreError};
use runtime_primitives::{SyncTime, WakeFailure};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};

mod probe;
use probe::Probe;

/// The object-specific half of the pipeline, implemented by the Fleet and Copy runtimes.
pub trait KernelFace: Send + Sync {
    /// The alarm the platform will deliver next.
    fn stored_alarm(&self) -> impl Future<Output = Result<Option<SyncTime>, WakeFailure>> + Send;
    fn now(&self) -> SyncTime;
    /// Migrations and handler loading; idempotent.
    fn open(&self) -> impl Future<Output = Result<(), StoreError>> + Send;
    /// The event's own work. Failures become the event's answer, never a panic.
    fn handle(&self, event: ObjectEvent) -> impl Future<Output = EventReply> + Send;
    /// Every cadence-held due source, each failing alone.
    fn run_due_sources(&self, now: SyncTime) -> impl Future<Output = ()> + Send;
    /// Due work the cadence never holds because its deadline is a protocol
    /// answer, not cleanup (Fleet forward expiry). Runs on every frame and alarm.
    fn run_deadline_sources(&self, now: SyncTime) -> impl Future<Output = ()> + Send;
    /// The long-operation engine, run last and outside the batch claim.
    fn drive_long_operations(&self) -> impl Future<Output = ()> + Send;
    /// Read every due source and set the alarm exactly.
    fn recompute(
        &self,
        next_batch_at: Option<SyncTime>,
    ) -> impl Future<Output = Result<Option<SyncTime>, StoreError>> + Send;
}

pub struct ObjectKernel<Face: KernelFace> {
    face: Face,
    claim: BatchClaim,
    opened: AtomicBool,
    #[cfg(feature = "conformance")]
    observer: Option<std::sync::Arc<dyn crate::event_trace::KernelObserver>>,
    /// Records are numbered per construction as each event completes.
    #[cfg(feature = "conformance")]
    sequence: std::sync::atomic::AtomicU64,
}

impl<Face: KernelFace> ObjectKernel<Face> {
    pub fn new(face: Face) -> Self {
        Self {
            face,
            claim: BatchClaim::default(),
            opened: AtomicBool::new(false),
            #[cfg(feature = "conformance")]
            observer: None,
            #[cfg(feature = "conformance")]
            sequence: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// A kernel whose every event emits one record to `observer`.
    #[cfg(feature = "conformance")]
    pub fn observed(
        face: Face,
        observer: std::sync::Arc<dyn crate::event_trace::KernelObserver>,
    ) -> Self {
        Self {
            observer: Some(observer),
            ..Self::new(face)
        }
    }

    /// The wrapped runtime, for reads and controls that must not enter the pipeline.
    pub fn face(&self) -> &Face {
        &self.face
    }

    pub async fn on_event(&self, event: ObjectEvent) -> EventReply {
        #[cfg(feature = "conformance")]
        {
            self.on_event_with(event, crate::event_trace::EventContext::default())
                .await
        }
        #[cfg(not(feature = "conformance"))]
        {
            self.run(event, Probe::off()).await
        }
    }

    /// `on_event` with what only the shell knows about the event, recorded in S1.
    #[cfg(feature = "conformance")]
    pub async fn on_event_with(
        &self,
        event: ObjectEvent,
        context: crate::event_trace::EventContext,
    ) -> EventReply {
        use crate::event_trace::{EventBounds, EventTrace, scoped};
        let Some(observer) = self.observer.clone() else {
            return self.run(event, Probe::off()).await;
        };
        let trace = std::sync::Arc::new(EventTrace::new(observer));
        let entered_at = self.face.now();
        let alarm_before = self.face.stored_alarm().await.ok().flatten();
        let kind = event_kind_record(&event, context);
        let reply = scoped(trace.clone(), self.run(event, Probe::on(trace.clone()))).await;
        let alarm_after = self.face.stored_alarm().await.ok().flatten();
        trace.finish(EventBounds {
            sequence: self.sequence.fetch_add(1, Ordering::AcqRel),
            event: kind,
            entered_at,
            exited_at: self.face.now(),
            alarm_before,
            alarm_after,
            reply: reply_record(&reply),
        });
        reply
    }

    async fn run(&self, event: ObjectEvent, probe: Probe) -> EventReply {
        let is_alarm = matches!(event, ObjectEvent::Alarm);
        let runs_due_batch = event.runs_due_batch();
        // 0. Open.
        probe.step(PipelineStep::Open);
        if !self.opened.load(Ordering::Acquire) {
            if let Err(failure) = self.face.open().await {
                eprintln!("central object open failed: {failure}");
                return unavailable(&event);
            }
            self.opened.store(true, Ordering::Release);
        }
        // 1. Repair: a stored alarm in the past is replaced before any work runs.
        if !is_alarm {
            probe.step(PipelineStep::Repair);
            self.repair_stale_alarm(&probe).await;
        }
        // 2. Handle.
        probe.step(PipelineStep::Handle);
        let reply = self.face.handle(event).await;
        probe.handler_outcome(&reply);
        probe
            .barrier(BarrierPoint::AfterUnitCommit, Some("handler"))
            .await;
        // 3. Due batch.
        if runs_due_batch {
            self.run_due_batch(&probe).await;
        }
        // 4. Recompute.
        probe.step(PipelineStep::Recompute);
        probe.barrier(BarrierPoint::BeforeRecomputeTx, None).await;
        match self.face.recompute(self.claim.next_batch_at()).await {
            Ok(at) => {
                probe.exact(at);
                reply
            }
            Err(failure) => {
                eprintln!("central alarm recompute failed: {failure}");
                // Every unit that created due work lowered the alarm in its own
                // commit, so a frame or call answer still goes out.
                if is_alarm {
                    EventReply::AlarmRetry
                } else {
                    reply
                }
            }
        }
    }

    async fn repair_stale_alarm(&self, probe: &Probe) {
        let now = self.face.now();
        match self.face.stored_alarm().await {
            Ok(Some(stored)) if stored < now => {
                match self.face.recompute(self.claim.next_batch_at()).await {
                    Ok(at) => probe.exact(at),
                    Err(failure) => eprintln!("central stale-alarm repair failed: {failure}"),
                }
            }
            Ok(_) => {}
            Err(failure) => eprintln!("central alarm readback failed: {failure}"),
        }
    }

    async fn run_due_batch(&self, probe: &Probe) {
        probe.step(PipelineStep::DueBatch);
        probe.barrier(BarrierPoint::BeforeClaim, None).await;
        let now = self.face.now();
        match self.claim.try_claim(now) {
            Ok(claim) => {
                self.face.run_due_sources(now).await;
                // Released before the engine runs, so a frame arriving while a
                // step waits on a silent holder can run the next batch.
                drop(claim);
                probe.batch(true, Some(now), Some(self.face.now()));
            }
            Err(ClaimDeclined::Running | ClaimDeclined::Cadence { .. }) => {
                probe.batch(false, None, None);
            }
        }
        probe.step(PipelineStep::DeadlineSources);
        self.face.run_deadline_sources(now).await;
        // Until slice 4 moves request and owed work off the engine, its drive is
        // not held by the cadence: a Hello's Welcome or a push can't wait 30 s.
        probe.step(PipelineStep::LongOperations);
        self.face.drive_long_operations().await;
    }
}

#[cfg(feature = "conformance")]
fn event_kind_record(
    event: &ObjectEvent,
    context: crate::event_trace::EventContext,
) -> crate::event_trace::EventKindRecord {
    use crate::event_trace::{CloseCause, EventKindRecord};
    #[derive(serde::Deserialize)]
    struct Kinded {
        kind: String,
    }
    match event {
        ObjectEvent::SocketOpened { binding } => EventKindRecord::SocketOpened {
            connection: binding.connection,
        },
        ObjectEvent::Frame { binding, frame } => EventKindRecord::Frame {
            connection: binding.connection,
            frame: frame.clone(),
        },
        ObjectEvent::SocketClosed { binding } => EventKindRecord::SocketClosed {
            connection: binding.connection,
            cause: CloseCause::Callback,
        },
        ObjectEvent::Call { body, .. } => EventKindRecord::Call {
            call_kind: serde_json::from_slice::<Kinded>(body)
                .ok()
                .map(|kinded| kinded.kind),
        },
        ObjectEvent::Alarm => EventKindRecord::Alarm {
            redelivery: context.redelivery,
        },
    }
}

#[cfg(feature = "conformance")]
const fn reply_record(reply: &EventReply) -> crate::event_trace::ReplyRecord {
    use crate::event_trace::ReplyRecord;
    match reply {
        EventReply::None => ReplyRecord::None,
        EventReply::SocketFailed => ReplyRecord::SocketFailed,
        EventReply::CallAnswer(_) => ReplyRecord::CallAnswer,
        EventReply::CallFailure(_) => ReplyRecord::CallFailure,
        EventReply::AlarmDone => ReplyRecord::AlarmDone,
        EventReply::AlarmRetry => ReplyRecord::AlarmRetry,
    }
}

fn unavailable(event: &ObjectEvent) -> EventReply {
    match event {
        ObjectEvent::Call { .. } => EventReply::CallFailure(CallFailure::AnswerLost),
        ObjectEvent::Alarm => EventReply::AlarmRetry,
        ObjectEvent::SocketOpened { .. } | ObjectEvent::Frame { .. } => EventReply::SocketFailed,
        ObjectEvent::SocketClosed { .. } => EventReply::None,
    }
}

#[cfg(test)]
#[path = "object_kernel_test.rs"]
mod tests;

macro_rules! delegate_kernel_face {
    ($($wrapper:ty),*) => {$(
        impl<Face: KernelFace> KernelFace for $wrapper {
            fn stored_alarm(&self) -> impl Future<Output = Result<Option<SyncTime>, WakeFailure>> + Send {
                (**self).stored_alarm()
            }
            fn now(&self) -> SyncTime {
                (**self).now()
            }
            fn open(&self) -> impl Future<Output = Result<(), StoreError>> + Send {
                (**self).open()
            }
            fn handle(&self, event: ObjectEvent) -> impl Future<Output = EventReply> + Send {
                (**self).handle(event)
            }
            fn run_due_sources(&self, now: SyncTime) -> impl Future<Output = ()> + Send {
                (**self).run_due_sources(now)
            }
            fn run_deadline_sources(&self, now: SyncTime) -> impl Future<Output = ()> + Send {
                (**self).run_deadline_sources(now)
            }
            fn drive_long_operations(&self) -> impl Future<Output = ()> + Send {
                (**self).drive_long_operations()
            }
            fn recompute(
                &self,
                next_batch_at: Option<SyncTime>,
            ) -> impl Future<Output = Result<Option<SyncTime>, StoreError>> + Send {
                (**self).recompute(next_batch_at)
            }
        }
    )*};
}
// A kernel can wrap a shared runtime, so a fixture holding the runtime drives real events.
delegate_kernel_face!(&Face, std::sync::Arc<Face>);
