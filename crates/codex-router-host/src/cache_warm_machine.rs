//! One Claude session's cache-warm cycle, as a pure machine (D1 state
//! machines; design note §3.7). The same rules as today's module: a turn end
//! arms one warm prompt while the last real turn is inside the keep-warm
//! window; a turn started by the module's own warm input isn't real work; a
//! refusal means "not now" and waits for the next turn end.

use router_clock::ClockInstant;
use session_event_model::InputId;
use state_machine_core::{
    EventTag, RefusalTag, StateMachine, StateMachineResult, StateTag, Transition, TransitionError,
};
use std::time::Duration;

/// The endpoint's cadence: how often to warm, and for how long after the
/// last real turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WarmCadence {
    pub(crate) every: Duration,
    pub(crate) keep_warm_for: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CacheWarm {
    Cold {
        last_real_end: Option<ClockInstant>,
    },
    Busy {
        real: bool,
        last_real_end: Option<ClockInstant>,
        /// The warm prompt this turn runs, while it is the module's own.
        warm: Option<InputId>,
    },
    Armed {
        last_real_end: ClockInstant,
        fires_at: ClockInstant,
    },
    Warming {
        input: InputId,
        last_real_end: ClockInstant,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CacheWarmEvent {
    TurnStarted {
        input: Option<InputId>,
    },
    TurnEnded {
        at: ClockInstant,
        cadence: WarmCadence,
    },
    TimerFired {
        input: InputId,
        at: ClockInstant,
        cadence: WarmCadence,
    },
    /// An input joined the running turn (a steer, or the prompt itself).
    InputJoined {
        input: InputId,
    },
    WarmRefused,
    SessionGone,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CacheWarmEffect {
    ArmTimer { at: ClockInstant },
    Disarm,
    SubmitWarm { input: InputId },
}

/// Cache warm never refuses an event to a caller; it only ignores stale ones.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CacheWarmRefusal {}

impl RefusalTag for CacheWarmRefusal {
    fn refusal_tag(&self) -> &'static str {
        match *self {}
    }
}

impl StateTag for CacheWarm {
    fn state_tag(&self) -> &'static str {
        match self {
            Self::Cold { .. } => "cold",
            Self::Busy { .. } => "busy",
            Self::Armed { .. } => "armed",
            Self::Warming { .. } => "warming",
        }
    }

    fn is_terminal(&self) -> bool {
        match self {
            Self::Cold { .. } | Self::Busy { .. } | Self::Armed { .. } | Self::Warming { .. } => {
                false
            }
        }
    }
}

impl EventTag for CacheWarmEvent {
    fn event_tag(&self) -> &'static str {
        match self {
            Self::TurnStarted { .. } => "turn_started",
            Self::TurnEnded { .. } => "turn_ended",
            Self::TimerFired { .. } => "timer_fired",
            Self::InputJoined { .. } => "input_joined",
            Self::WarmRefused => "warm_refused",
            Self::SessionGone => "session_gone",
        }
    }
}

const fn edge(from: &'static str, event: &'static str, to: &'static str) -> Transition {
    Transition {
        from,
        event,
        to,
        terminal: false,
    }
}

const TRANSITIONS: &[Transition] = &[
    edge("cold", "turn_started", "busy"),
    edge("armed", "turn_started", "busy"),
    edge("warming", "turn_started", "busy"),
    edge("busy", "turn_ended", "armed"),
    edge("busy", "turn_ended", "cold"),
    edge("armed", "timer_fired", "warming"),
    edge("armed", "timer_fired", "cold"),
    edge("busy", "input_joined", "busy"),
    edge("warming", "warm_refused", "cold"),
    edge("cold", "session_gone", "cold"),
    edge("busy", "session_gone", "cold"),
    edge("armed", "session_gone", "cold"),
    edge("warming", "session_gone", "cold"),
];

pub(crate) struct CacheWarmMachine;

fn last_real(state: &CacheWarm) -> Option<ClockInstant> {
    match state {
        CacheWarm::Cold { last_real_end } | CacheWarm::Busy { last_real_end, .. } => *last_real_end,
        CacheWarm::Armed { last_real_end, .. } | CacheWarm::Warming { last_real_end, .. } => {
            Some(*last_real_end)
        }
    }
}

impl StateMachine for CacheWarmMachine {
    type State = CacheWarm;
    type Event = CacheWarmEvent;
    type Effect = CacheWarmEffect;
    type Error = CacheWarmRefusal;

    fn transition(state: &CacheWarm, event: CacheWarmEvent) -> StateMachineResult<Self> {
        use CacheWarm as S;
        use CacheWarmEffect as E;
        use CacheWarmEvent as V;
        match (state, event) {
            (S::Cold { .. } | S::Armed { .. } | S::Warming { .. }, V::TurnStarted { input }) => {
                let warm = match (state, &input) {
                    (S::Warming { input: warm, .. }, Some(started)) if warm == started => {
                        Some(warm.clone())
                    }
                    (S::Cold { .. } | S::Armed { .. } | S::Warming { .. } | S::Busy { .. }, _) => {
                        None
                    }
                };
                let effects = match state {
                    S::Armed { .. } => vec![E::Disarm],
                    S::Cold { .. } | S::Warming { .. } | S::Busy { .. } => Vec::new(),
                };
                Ok((
                    S::Busy {
                        real: warm.is_none(),
                        last_real_end: last_real(state),
                        warm,
                    },
                    effects,
                ))
            }
            (S::Busy { .. }, V::TurnStarted { .. }) => Err(TransitionError::Stale),
            (
                S::Busy {
                    real,
                    last_real_end,
                    ..
                },
                V::TurnEnded { at, cadence },
            ) => {
                let last = if *real { Some(at) } else { *last_real_end };
                match last {
                    Some(last_real_end)
                        if at.saturating_duration_since(last_real_end) < cadence.keep_warm_for =>
                    {
                        let fires_at = at + cadence.every;
                        Ok((
                            S::Armed {
                                last_real_end,
                                fires_at,
                            },
                            vec![E::ArmTimer { at: fires_at }],
                        ))
                    }
                    Some(_) | None => Ok((
                        S::Cold {
                            last_real_end: last,
                        },
                        Vec::new(),
                    )),
                }
            }
            (S::Cold { .. } | S::Armed { .. } | S::Warming { .. }, V::TurnEnded { .. }) => {
                Err(TransitionError::Stale)
            }
            (S::Armed { last_real_end, .. }, V::TimerFired { input, at, cadence })
                if at.saturating_duration_since(*last_real_end) < cadence.keep_warm_for =>
            {
                Ok((
                    S::Warming {
                        input: input.clone(),
                        last_real_end: *last_real_end,
                    },
                    vec![E::SubmitWarm { input }],
                ))
            }
            // The window passed while the timer waited: nothing to warm.
            (S::Armed { last_real_end, .. }, V::TimerFired { .. }) => Ok((
                S::Cold {
                    last_real_end: Some(*last_real_end),
                },
                Vec::new(),
            )),
            // An input other than the warm prompt joined: the turn is real.
            (
                S::Busy {
                    real: false,
                    last_real_end,
                    warm: Some(warm),
                },
                V::InputJoined { input },
            ) if &input != warm => Ok((
                S::Busy {
                    real: true,
                    last_real_end: *last_real_end,
                    warm: None,
                },
                Vec::new(),
            )),
            (
                S::Cold { .. } | S::Busy { .. } | S::Armed { .. } | S::Warming { .. },
                V::InputJoined { .. },
            ) => Err(TransitionError::Stale),
            (S::Cold { .. } | S::Busy { .. } | S::Warming { .. }, V::TimerFired { .. }) => {
                Err(TransitionError::Stale)
            }
            (S::Warming { last_real_end, .. }, V::WarmRefused) => Ok((
                S::Cold {
                    last_real_end: Some(*last_real_end),
                },
                Vec::new(),
            )),
            (S::Cold { .. } | S::Busy { .. } | S::Armed { .. }, V::WarmRefused) => {
                Err(TransitionError::Stale)
            }
            (
                S::Cold { .. } | S::Busy { .. } | S::Armed { .. } | S::Warming { .. },
                V::SessionGone,
            ) => Ok((
                S::Cold {
                    last_real_end: None,
                },
                vec![E::Disarm],
            )),
        }
    }

    fn transitions() -> &'static [Transition] {
        TRANSITIONS
    }
}

#[cfg(test)]
#[path = "cache_warm_machine_test.rs"]
mod tests;
