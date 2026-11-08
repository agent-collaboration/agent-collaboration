//! One object's conformance seams, held by its slot outside the kernel so they
//! survive a rebuild: the S1 record ring, S7 latches, the S2 and S4 switches
//! and the pending-work counters.

use collaboration_central_conformance::{
    CursoredEvent, RuntimeControlAnswer, RuntimeControlFailure,
};
use collaboration_central_core::ObjectAddress;
use collaboration_central_core::event_trace::{
    AlarmDroppedLine, BarrierPoint, ConformanceLine, ConstructionCause, ConstructionLine,
    EventRecord, KernelObserver,
};
use durable_steps::{BoxFuture, Uuid};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, watch};

/// Records kept per object; older ones are reported as truncated.
const RING_CAPACITY: usize = 4096;
/// Bounds a seam wait whose causal signal never arrives.
pub(super) const SEAM_WAIT_BOUND: Duration = Duration::from_secs(30);

/// Minted once per process: a Tokio shell is one isolate.
fn isolate() -> Uuid {
    static ISOLATE: OnceLock<Uuid> = OnceLock::new();
    *ISOLATE.get_or_init(Uuid::now_v7)
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) struct ObjectSeams {
    address: ObjectAddress,
    ring: Mutex<EventRing>,
    latches: Mutex<Vec<Arc<Latch>>>,
    alarm_delivery: AtomicBool,
    rebuild_at_quiescence: AtomicBool,
    deadlines: AtomicU64,
    waiters: AtomicU64,
    alarm_events: AtomicU64,
    duplicates: mpsc::UnboundedSender<oneshot::Sender<()>>,
    duplicate_requests: Mutex<Option<mpsc::UnboundedReceiver<oneshot::Sender<()>>>>,
}

struct EventRing {
    records: VecDeque<CursoredEvent>,
    next_cursor: u64,
}

struct Latch {
    id: Uuid,
    point: BarrierPoint,
    unit_kind: Option<String>,
    captured: watch::Sender<Option<Uuid>>,
    released: watch::Sender<bool>,
}

impl ObjectSeams {
    pub(super) fn new(address: ObjectAddress) -> Self {
        let (duplicates, requests) = mpsc::unbounded_channel();
        Self {
            address,
            ring: Mutex::new(EventRing {
                records: VecDeque::new(),
                next_cursor: 1,
            }),
            latches: Mutex::default(),
            alarm_delivery: AtomicBool::new(true),
            rebuild_at_quiescence: AtomicBool::new(false),
            deadlines: AtomicU64::new(0),
            waiters: AtomicU64::new(0),
            alarm_events: AtomicU64::new(0),
            duplicates,
            duplicate_requests: Mutex::new(Some(requests)),
        }
    }

    /// The observer for one kernel construction; emits its construction line.
    pub(super) fn observer(self: &Arc<Self>, cause: ConstructionCause) -> Arc<dyn KernelObserver> {
        let construction = Uuid::now_v7();
        ConformanceLine::CentralConstruction(&ConstructionLine {
            object: self.address.clone(),
            construction,
            cause,
            isolate: isolate(),
        })
        .emit();
        Arc::new(ConstructionObserver {
            seams: self.clone(),
            construction,
        })
    }

    fn push(&self, record: EventRecord) {
        let mut ring = locked(&self.ring);
        let cursor = ring.next_cursor;
        ring.next_cursor += 1;
        if ring.records.len() == RING_CAPACITY {
            ring.records.pop_front();
        }
        ring.records.push_back(CursoredEvent { cursor, record });
    }

    pub(super) fn read_events(&self, after: u64) -> RuntimeControlAnswer {
        let ring = locked(&self.ring);
        RuntimeControlAnswer::Events {
            records: ring
                .records
                .iter()
                .filter(|entry| entry.cursor > after)
                .cloned()
                .collect(),
            first_retained: ring
                .records
                .front()
                .map_or(ring.next_cursor, |entry| entry.cursor),
            through: ring.next_cursor - 1,
        }
    }

    pub(super) fn arm_barrier(&self, point: BarrierPoint, unit_kind: Option<String>) -> Uuid {
        let id = Uuid::now_v7();
        locked(&self.latches).push(Arc::new(Latch {
            id,
            point,
            unit_kind,
            captured: watch::channel(None).0,
            released: watch::channel(false).0,
        }));
        id
    }

    fn latch(&self, id: Uuid) -> Result<Arc<Latch>, RuntimeControlFailure> {
        locked(&self.latches)
            .iter()
            .find(|latch| latch.id == id)
            .cloned()
            .ok_or(RuntimeControlFailure::UnknownBarrier)
    }

    pub(super) async fn await_barrier(&self, id: Uuid) -> Result<Uuid, RuntimeControlFailure> {
        let mut captured = self.latch(id)?.captured.subscribe();
        let held = tokio::time::timeout(SEAM_WAIT_BOUND, captured.wait_for(Option::is_some))
            .await
            .map_err(|_| RuntimeControlFailure::WaitBoundExceeded)?
            .map_err(|_| RuntimeControlFailure::UnknownBarrier)?;
        held.ok_or(RuntimeControlFailure::UnknownBarrier)
    }

    pub(super) fn release_barrier(&self, id: Uuid) -> Result<(), RuntimeControlFailure> {
        self.latch(id)?.released.send_replace(true);
        Ok(())
    }

    /// Holds `event` if an armed, uncaptured latch matches; otherwise resolves at once.
    fn hold(
        &self,
        point: BarrierPoint,
        unit_kind: Option<&'static str>,
        event: Uuid,
    ) -> BoxFuture<'static, ()> {
        // Find and capture under one lock, so two events never take one latch.
        let latches = locked(&self.latches);
        let Some(latch) = latches.iter().find(|latch| {
            latch.point == point
                // A released latch is spent, even if it never captured an event.
                && !*latch.released.borrow()
                && latch.captured.borrow().is_none()
                && latch
                    .unit_kind
                    .as_deref()
                    .is_none_or(|kind| unit_kind == Some(kind))
        }) else {
            return Box::pin(std::future::ready(()));
        };
        // Subscribe before announcing the capture, so a release that follows
        // at once is still seen.
        let mut released = latch.released.subscribe();
        latch.captured.send_replace(Some(event));
        drop(latches);
        Box::pin(async move {
            let _ = released.wait_for(|released| *released).await;
        })
    }

    pub(super) fn set_alarm_delivery(&self, enabled: bool) {
        self.alarm_delivery.store(enabled, Ordering::SeqCst);
    }

    /// Counts one alarm event, and reports whether it may be delivered.
    pub(super) fn admit_alarm(&self, at: runtime_primitives::SyncTime) -> bool {
        self.alarm_events.fetch_add(1, Ordering::SeqCst);
        if self.alarm_delivery.load(Ordering::SeqCst) {
            return true;
        }
        ConformanceLine::CentralAlarmDropped(&AlarmDroppedLine {
            object: self.address.clone(),
            at: at.unix_millis(),
        })
        .emit();
        false
    }

    pub(super) fn alarm_events(&self) -> u64 {
        self.alarm_events.load(Ordering::SeqCst)
    }

    pub(super) fn set_rebuild_at_quiescence(&self, enabled: bool) {
        self.rebuild_at_quiescence.store(enabled, Ordering::SeqCst);
    }

    pub(super) fn rebuild_at_quiescence(&self) -> bool {
        self.rebuild_at_quiescence.load(Ordering::SeqCst)
    }

    /// Asks the alarm loop for one extra Alarm event; resolves when it has completed.
    pub(super) fn request_duplicate_alarm(&self) -> oneshot::Receiver<()> {
        let (done, completed) = oneshot::channel();
        // A loop that has stopped drops the request, which the caller sees.
        let _ = self.duplicates.send(done);
        completed
    }

    /// Taken once by the object's alarm loop.
    pub(super) fn take_duplicate_requests(
        &self,
    ) -> Option<mpsc::UnboundedReceiver<oneshot::Sender<()>>> {
        locked(&self.duplicate_requests).take()
    }

    pub(super) fn pending(&self, tasks: u64) -> RuntimeControlAnswer {
        RuntimeControlAnswer::Pending {
            tasks,
            deadlines: self.deadlines.load(Ordering::SeqCst),
            waiters: self.waiters.load(Ordering::SeqCst),
        }
    }

    pub(super) fn deadline_started(self: &Arc<Self>) -> PendingGuard {
        PendingGuard::count(self.clone(), PendingKind::Deadline)
    }

    pub(super) fn call_started(self: &Arc<Self>) -> PendingGuard {
        PendingGuard::count(self.clone(), PendingKind::Waiter)
    }

    fn counter(&self, kind: PendingKind) -> &AtomicU64 {
        match kind {
            PendingKind::Deadline => &self.deadlines,
            PendingKind::Waiter => &self.waiters,
        }
    }
}

#[derive(Clone, Copy)]
enum PendingKind {
    Deadline,
    Waiter,
}

/// Counts one live deadline or call waiter until dropped.
pub(super) struct PendingGuard {
    seams: Arc<ObjectSeams>,
    kind: PendingKind,
}

impl PendingGuard {
    fn count(seams: Arc<ObjectSeams>, kind: PendingKind) -> Self {
        seams.counter(kind).fetch_add(1, Ordering::SeqCst);
        Self { seams, kind }
    }
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        self.seams.counter(self.kind).fetch_sub(1, Ordering::SeqCst);
    }
}

struct ConstructionObserver {
    seams: Arc<ObjectSeams>,
    construction: Uuid,
}

impl KernelObserver for ConstructionObserver {
    fn object(&self) -> ObjectAddress {
        self.seams.address.clone()
    }
    fn construction(&self) -> Uuid {
        self.construction
    }
    fn emit(&self, record: EventRecord) {
        ConformanceLine::CentralEvent(&record).emit();
        self.seams.push(record);
    }
    fn barrier(
        &self,
        point: BarrierPoint,
        unit_kind: Option<&'static str>,
        event: Uuid,
    ) -> BoxFuture<'static, ()> {
        self.seams.hold(point, unit_kind, event)
    }
}

#[cfg(test)]
#[path = "event_seams_test.rs"]
mod tests;
