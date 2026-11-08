//! What one event did, collected while it runs (conformance builds only).
//!
//! The kernel opens one trace per event and runs the event inside it. The
//! shells report what only they see (a frame sent, an object call, a unit
//! lowering the alarm) through the free functions here, which find the running
//! event's trace in a scope set around each poll of the event's future. Work a
//! shell spawns onto another task leaves that scope and is not attributed.
//!
//! Without the `conformance` feature only the two names the kernel's hooks
//! take remain, and every hook is empty.

use serde::{Deserialize, Serialize};

/// The kernel step that is running. Until slice 3 gives each unit its own
/// kind, the step names whatever made a send, a call or an alarm write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PipelineStep {
    Open,
    Repair,
    Handle,
    DueBatch,
    DeadlineSources,
    LongOperations,
    Recompute,
}

/// A point in the pipeline where a conformance barrier can hold an event.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BarrierPoint {
    BeforeRecomputeTx,
    AfterUnitCommit,
    AfterStepLower,
    /// Just before the batch claim is attempted, outside its atomic check-and-set.
    BeforeClaim,
}

#[cfg(feature = "conformance")]
mod record;
#[cfg(feature = "conformance")]
pub use record::*;
#[cfg(feature = "conformance")]
mod trace;
#[cfg(feature = "conformance")]
pub(crate) use trace::{
    EventBounds, EventTrace, SendBinding, bind_send, current_trace, scoped, within_trace,
};
#[cfg(feature = "conformance")]
pub use trace::{
    EventClass, EventContext, KernelObserver, current_event, record_lower, record_send, traced_call,
};

#[cfg(feature = "conformance")]
impl PipelineStep {
    pub(crate) const fn unit_kind(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Repair => "repair",
            Self::Handle => "handler",
            Self::DueBatch => "due_batch",
            Self::DeadlineSources => "deadline_sources",
            Self::LongOperations => "long_operations",
            Self::Recompute => "recompute",
        }
    }
}

#[cfg(all(test, feature = "conformance"))]
#[path = "event_trace_test.rs"]
mod tests;
