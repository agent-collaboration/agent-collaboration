//! The kernel's hooks into the event record and the conformance barriers.
//! Without the `conformance` feature, every hook is empty.

use crate::event_trace::{BarrierPoint, PipelineStep};
use runtime_primitives::SyncTime;

#[cfg(feature = "conformance")]
pub(super) struct Probe(Option<std::sync::Arc<crate::event_trace::EventTrace>>);

#[cfg(not(feature = "conformance"))]
pub(super) struct Probe;

#[cfg(not(feature = "conformance"))]
impl Probe {
    pub(super) const fn off() -> Self {
        Self
    }
    pub(super) fn step(&self, _step: PipelineStep) {}
    pub(super) fn barrier(
        &self,
        _point: BarrierPoint,
        _unit_kind: Option<&'static str>,
    ) -> std::future::Ready<()> {
        std::future::ready(())
    }
    pub(super) fn handler_outcome(&self, _reply: &crate::EventReply) {}
    pub(super) fn batch(
        &self,
        _claimed: bool,
        _started: Option<SyncTime>,
        _ended: Option<SyncTime>,
    ) {
    }
    pub(super) fn exact(&self, _at: Option<SyncTime>) {}
}

#[cfg(feature = "conformance")]
impl Probe {
    pub(super) const fn off() -> Self {
        Self(None)
    }

    pub(super) const fn on(trace: std::sync::Arc<crate::event_trace::EventTrace>) -> Self {
        Self(Some(trace))
    }

    pub(super) fn step(&self, step: PipelineStep) {
        if let Some(trace) = &self.0 {
            trace.step(step);
        }
    }

    pub(super) async fn barrier(&self, point: BarrierPoint, unit_kind: Option<&'static str>) {
        if let Some(trace) = &self.0 {
            trace.barrier(point, unit_kind).await;
        }
    }

    /// Until slice 3, the event's handler is its one unit.
    pub(super) fn handler_outcome(&self, reply: &crate::EventReply) {
        use crate::EventReply;
        use crate::event_trace::{UnitOutcomeRecord, UnitRecord};
        let Some(trace) = &self.0 else {
            return;
        };
        let outcome = match reply {
            EventReply::CallFailure(failure) => UnitOutcomeRecord::Failed {
                error_class: "callFailure".into(),
                message: format!("{failure:?}"),
            },
            EventReply::SocketFailed => UnitOutcomeRecord::Failed {
                error_class: "socketFailed".into(),
                message: "the frame's handler failed".into(),
            },
            EventReply::AlarmRetry => UnitOutcomeRecord::Failed {
                error_class: "alarmRetry".into(),
                message: "the alarm's handler asked for redelivery".into(),
            },
            EventReply::None | EventReply::CallAnswer(_) | EventReply::AlarmDone => {
                UnitOutcomeRecord::Done
            }
        };
        trace.unit(UnitRecord {
            unit_kind: PipelineStep::Handle.unit_kind().into(),
            subject: None,
            outcome,
        });
    }

    pub(super) fn batch(&self, claimed: bool, started: Option<SyncTime>, ended: Option<SyncTime>) {
        if let Some(trace) = &self.0 {
            trace.batch(crate::event_trace::BatchRecord {
                claimed,
                started_at: started.map(SyncTime::unix_millis),
                ended_at: ended.map(SyncTime::unix_millis),
                sources: Vec::new(),
            });
        }
    }

    pub(super) fn exact(&self, at: Option<SyncTime>) {
        if let Some(trace) = &self.0 {
            trace.exact(at);
        }
    }
}
