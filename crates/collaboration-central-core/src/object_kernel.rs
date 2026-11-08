//! One entry for every event an object receives, the same on both shells.
//!
//! Every event opens the object once per construction, repairs a stale alarm,
//! handles its own work, runs the due batch when the cadence allows (frames and
//! alarms only), and recomputes the alarm. Nothing the object needs lives only
//! in memory: the claim and the opened flag are hints whose loss costs at most
//! one extra batch or one repeated open.

use crate::alarm_scheduler::{BatchClaim, ClaimDeclined};
use crate::object_event::{EventReply, ObjectEvent};
use durable_steps::{CallFailure, StoreError};
use runtime_primitives::{SyncTime, WakeFailure};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};

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
}

impl<Face: KernelFace> ObjectKernel<Face> {
    pub fn new(face: Face) -> Self {
        Self {
            face,
            claim: BatchClaim::default(),
            opened: AtomicBool::new(false),
        }
    }

    /// The wrapped runtime, for reads and controls that must not enter the pipeline.
    pub fn face(&self) -> &Face {
        &self.face
    }

    pub async fn on_event(&self, event: ObjectEvent) -> EventReply {
        let is_alarm = matches!(event, ObjectEvent::Alarm);
        let runs_due_batch = event.runs_due_batch();
        // 0. Open.
        if !self.opened.load(Ordering::Acquire) {
            if let Err(failure) = self.face.open().await {
                eprintln!("central object open failed: {failure}");
                return unavailable(&event);
            }
            self.opened.store(true, Ordering::Release);
        }
        // 1. Repair: a stored alarm in the past is replaced before any work runs.
        if !is_alarm {
            self.repair_stale_alarm().await;
        }
        // 2. Handle.
        let reply = self.face.handle(event).await;
        // 3. Due batch.
        if runs_due_batch {
            self.run_due_batch().await;
        }
        // 4. Recompute.
        match self.face.recompute(self.claim.next_batch_at()).await {
            Ok(_) => reply,
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

    async fn repair_stale_alarm(&self) {
        let now = self.face.now();
        match self.face.stored_alarm().await {
            Ok(Some(stored)) if stored < now => {
                if let Err(failure) = self.face.recompute(self.claim.next_batch_at()).await {
                    eprintln!("central stale-alarm repair failed: {failure}");
                }
            }
            Ok(_) => {}
            Err(failure) => eprintln!("central alarm readback failed: {failure}"),
        }
    }

    async fn run_due_batch(&self) {
        let now = self.face.now();
        match self.claim.try_claim(now) {
            Ok(claim) => {
                self.face.run_due_sources(now).await;
                // Released before the engine runs, so a frame arriving while a
                // step waits on a silent holder can run the next batch.
                drop(claim);
            }
            Err(ClaimDeclined::Running | ClaimDeclined::Cadence { .. }) => {}
        }
        self.face.run_deadline_sources(now).await;
        // Until slice 4 moves request and owed work off the engine, its drive is
        // not held by the cadence: a Hello's Welcome or a push can't wait 30 s.
        self.face.drive_long_operations().await;
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
