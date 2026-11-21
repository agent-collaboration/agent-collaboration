#![deny(clippy::wildcard_enum_match_arm)]
//! Shared seven-day timer lifetime and state-only discard policy.
//!
//! Discard uses the stored lifecycle tag and raw comparison values, so a retired
//! timer whose execution payload cannot decode can still be finished.
use central_engine_model::{
    Context, EngineTime, EventTag, Machine, MachineKind, RefusalTag, StateTag, Transition,
    TransitionError,
};
use std::marker::PhantomData;

/// The finishing view shares the subject's machine kind,
/// so its failure record and compare-and-set are the subject's own.
pub trait FinishKind: Send + Sync + 'static {
    const KIND: MachineKind;
}
pub struct LifecycleFinish<K: FinishKind>(PhantomData<K>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lifecycle {
    Active,
    Done,
    Disabled,
}
impl Lifecycle {
    pub fn from_stored_tag(tag: &str) -> Option<Self> {
        match tag {
            "active" => Some(Self::Active),
            "done" => Some(Self::Done),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
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
pub enum FinishEvent {
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
pub enum NoEffect {}

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

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TimerRefusal {
    AlreadyFinished,
    InvalidTiming,
    InvalidIdentifier,
}
impl RefusalTag for TimerRefusal {
    fn refusal_tag(&self) -> &'static str {
        match self {
            Self::AlreadyFinished => "already_finished",
            Self::InvalidTiming => "invalid_timing",
            Self::InvalidIdentifier => "invalid_identifier",
        }
    }
}

pub const LIFETIME_SECONDS: i64 = 7 * 24 * 60 * 60;

pub fn lifetime_end(now: EngineTime) -> Option<EngineTime> {
    now.checked_add_signed(chrono::Duration::seconds(LIFETIME_SECONDS))
        .filter(|time| central_engine_model::supported_time(*time))
}
