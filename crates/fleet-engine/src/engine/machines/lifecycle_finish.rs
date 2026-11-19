#![deny(clippy::wildcard_enum_match_arm)]
//! The finishing view admin Discard runs (P8): a subject's lifecycle facts only,
//! never its execution payload, so a retired subject whose timing_rule, due_at or
//! target won't decode can still be finished. The same rule CancelWake follows.
use super::timer_effects::TimerRefusal;
use central_engine_model::{
    Context, EngineTime, EventTag, Machine, MachineKind, StateTag, Transition, TransitionError,
};
use std::marker::PhantomData;

#[cfg(test)]
#[path = "lifecycle_finish_test.rs"]
mod tests;

/// Which machine's subjects this view finishes: it shares that machine's kind,
/// so its failure record and compare-and-set are the subject's own.
pub(crate) trait FinishKind: Send + Sync + 'static {
    const KIND: MachineKind;
}
pub(crate) struct WakeKind;
impl FinishKind for WakeKind {
    const KIND: MachineKind = MachineKind("wake_timer");
}
pub(crate) struct AutomationKind;
impl FinishKind for AutomationKind {
    const KIND: MachineKind = MachineKind("automation_timer");
}

pub(crate) struct LifecycleFinish<K: FinishKind>(PhantomData<K>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Lifecycle {
    Active,
    Done,
    Disabled,
}
impl StateTag for Lifecycle {
    fn state_tag(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Done => "done",
            Self::Disabled => "disabled",
        }
    }
    fn is_terminal(&self) -> bool {
        match self {
            Self::Active => false,
            Self::Done | Self::Disabled => true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FinishEvent {
    Discard,
}
impl EventTag for FinishEvent {
    fn event_tag(&self) -> &'static str {
        match self {
            Self::Discard => "discard",
        }
    }
}

/// Finishing never has effects; the subject only becomes disabled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NoEffect {}

impl<K: FinishKind> Machine for LifecycleFinish<K> {
    const KIND: MachineKind = K::KIND;
    type State = Lifecycle;
    type Event = FinishEvent;
    type Effect = NoEffect;
    type Error = TimerRefusal;
    fn transition(
        state: &Lifecycle,
        event: FinishEvent,
        _context: &mut Context<'_>,
    ) -> Result<(Lifecycle, Vec<NoEffect>), TransitionError<TimerRefusal>> {
        match (state, event) {
            (Lifecycle::Active, FinishEvent::Discard) => Ok((Lifecycle::Disabled, vec![])),
            (Lifecycle::Done | Lifecycle::Disabled, FinishEvent::Discard) => {
                Err(TransitionError::Domain(TimerRefusal::AlreadyFinished))
            }
        }
    }
    fn transitions() -> &'static [Transition] {
        &[Transition {
            from: "active",
            event: "discard",
            to: "disabled",
            terminal: true,
        }]
    }
    fn next_due(_state: &Lifecycle) -> Option<EngineTime> {
        None
    }
}
