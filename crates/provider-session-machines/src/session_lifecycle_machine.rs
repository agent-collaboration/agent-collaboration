//! One provider session's lifecycle on a connection, as a pure machine
//! (D1 state machines, d1b specification §3.1, M3, M8; design note §12–§16).
//!
//! Close follows ACP: it cancels the running turn, then frees the session.
//! It never deletes, so a closed session ends NotLoaded and can be loaded or
//! resumed again. Each accepted create or load starts a new generation; a
//! result from an older generation is Stale and changes nothing (M4).

use crate::SessionGeneration;
use state_machine_core::{
    EventTag, RefusalTag, StateMachine, StateMachineResult, StateTag, Transition, TransitionError,
};

/// One load or resume attempt's identity, so a late reply from an earlier
/// attempt is recognised (a UUIDv7 minted by the caller).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LoadAttempt(pub uuid::Uuid);

/// One close attempt's identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CloseAttempt(pub uuid::Uuid);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestoreVia {
    Load,
    Resume,
}

/// Where a close started: a failed close returns there (Live), or to
/// NotLoaded when the close had already made a pending load stale.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseOrigin {
    Live,
    Loading,
}

/// A close first ends the running turn, then frees the session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClosePhase {
    EndingTurn,
    Freeing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionLifecycle {
    NotLoaded {
        last: SessionGeneration,
    },
    Loading {
        generation: SessionGeneration,
        attempt: LoadAttempt,
        via: RestoreVia,
    },
    Live {
        generation: SessionGeneration,
    },
    Closing {
        generation: SessionGeneration,
        from: CloseOrigin,
        phase: ClosePhase,
        attempt: CloseAttempt,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionLifecycleEvent {
    CreateAdmitted,
    LoadRequested {
        via: RestoreVia,
        attempt: LoadAttempt,
    },
    Loaded {
        attempt: LoadAttempt,
    },
    LoadFailed {
        attempt: LoadAttempt,
    },
    CloseRequested {
        attempt: CloseAttempt,
        turn_running: bool,
    },
    TurnEnded {
        generation: SessionGeneration,
    },
    CloseDone {
        attempt: CloseAttempt,
    },
    CloseFailed {
        attempt: CloseAttempt,
    },
    ActorExited {
        generation: SessionGeneration,
    },
    PromptRequested,
    Publish {
        generation: SessionGeneration,
    },
}

/// What the cell's runner does after a transition commits. Terminal facts
/// (Closed, Unloaded) are always applied (design note §11.1).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionLifecycleEffect {
    StartRestore {
        generation: SessionGeneration,
        attempt: LoadAttempt,
        via: RestoreVia,
    },
    RegisterActor {
        generation: SessionGeneration,
    },
    PublishActivation {
        generation: SessionGeneration,
    },
    EndLine {
        generation: SessionGeneration,
    },
    CancelTurn {
        generation: SessionGeneration,
    },
    StartFree {
        generation: SessionGeneration,
        attempt: CloseAttempt,
    },
    PublishClosed {
        generation: SessionGeneration,
    },
    PublishUnloaded {
        generation: SessionGeneration,
    },
    ReopenLine {
        generation: SessionGeneration,
    },
    PublishToHub {
        generation: SessionGeneration,
    },
}

/// Why the lifecycle refused an event. Each names the current state; the
/// caller-facing answer adds the valid next steps (M8, [`refusal_answer`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionLifecycleRefusal {
    DuplicateIdentity,
    SessionLoading,
    AlreadyLoaded,
    SessionClosing,
    NotLoaded,
    NothingToClose,
    CloseInProgress,
}

impl RefusalTag for SessionLifecycleRefusal {
    fn refusal_tag(&self) -> &'static str {
        match self {
            Self::DuplicateIdentity => "duplicate_identity",
            Self::SessionLoading => "session_loading",
            Self::AlreadyLoaded => "already_loaded",
            Self::SessionClosing => "session_closing",
            Self::NotLoaded => "not_loaded",
            Self::NothingToClose => "nothing_to_close",
            Self::CloseInProgress => "close_in_progress",
        }
    }
}

impl StateTag for SessionLifecycle {
    fn state_tag(&self) -> &'static str {
        match self {
            Self::NotLoaded { .. } => "not_loaded",
            Self::Loading { .. } => "loading",
            Self::Live { .. } => "live",
            Self::Closing {
                phase: ClosePhase::EndingTurn,
                ..
            } => "closing_ending_turn",
            Self::Closing {
                phase: ClosePhase::Freeing,
                ..
            } => "closing_freeing",
        }
    }

    fn is_terminal(&self) -> bool {
        match self {
            Self::NotLoaded { .. }
            | Self::Loading { .. }
            | Self::Live { .. }
            | Self::Closing { .. } => false,
        }
    }
}

impl EventTag for SessionLifecycleEvent {
    fn event_tag(&self) -> &'static str {
        match self {
            Self::CreateAdmitted => "create_admitted",
            Self::LoadRequested {
                via: RestoreVia::Load,
                ..
            } => "load_requested",
            Self::LoadRequested {
                via: RestoreVia::Resume,
                ..
            } => "resume_requested",
            Self::Loaded { .. } => "loaded",
            Self::LoadFailed { .. } => "load_failed",
            Self::CloseRequested { .. } => "close_requested",
            Self::TurnEnded { .. } => "turn_ended",
            Self::CloseDone { .. } => "close_done",
            Self::CloseFailed { .. } => "close_failed",
            Self::ActorExited { .. } => "actor_exited",
            Self::PromptRequested => "prompt_requested",
            Self::Publish { .. } => "publish",
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
    edge("not_loaded", "create_admitted", "live"),
    edge("not_loaded", "load_requested", "loading"),
    edge("not_loaded", "resume_requested", "loading"),
    edge("loading", "loaded", "live"),
    edge("loading", "load_failed", "not_loaded"),
    edge("loading", "close_requested", "closing_freeing"),
    edge("loading", "publish", "loading"),
    edge("live", "close_requested", "closing_ending_turn"),
    edge("live", "close_requested", "closing_freeing"),
    edge("live", "actor_exited", "not_loaded"),
    edge("live", "prompt_requested", "live"),
    edge("live", "publish", "live"),
    edge("closing_ending_turn", "turn_ended", "closing_freeing"),
    edge("closing_freeing", "close_done", "not_loaded"),
    edge("closing_freeing", "close_failed", "live"),
    edge("closing_freeing", "close_failed", "not_loaded"),
];

pub struct SessionLifecycleMachine;

fn next_generation(last: SessionGeneration) -> SessionGeneration {
    SessionGeneration::from_number(last.number().wrapping_add(1))
}

fn refused(refusal: SessionLifecycleRefusal) -> StateMachineResult<SessionLifecycleMachine> {
    Err(TransitionError::Domain(refusal))
}

impl StateMachine for SessionLifecycleMachine {
    type State = SessionLifecycle;
    type Event = SessionLifecycleEvent;
    type Effect = SessionLifecycleEffect;
    type Error = SessionLifecycleRefusal;

    fn transition(
        state: &SessionLifecycle,
        event: SessionLifecycleEvent,
    ) -> StateMachineResult<Self> {
        use SessionLifecycle as S;
        use SessionLifecycleEffect as E;
        use SessionLifecycleEvent as V;
        use SessionLifecycleRefusal as R;
        match (*state, event) {
            (S::NotLoaded { last }, V::CreateAdmitted) => {
                let generation = next_generation(last);
                Ok((
                    S::Live { generation },
                    vec![
                        E::RegisterActor { generation },
                        E::PublishActivation { generation },
                    ],
                ))
            }
            (S::Loading { .. } | S::Live { .. } | S::Closing { .. }, V::CreateAdmitted) => {
                refused(R::DuplicateIdentity)
            }
            (S::NotLoaded { last }, V::LoadRequested { via, attempt }) => {
                let generation = next_generation(last);
                Ok((
                    S::Loading {
                        generation,
                        attempt,
                        via,
                    },
                    vec![E::StartRestore {
                        generation,
                        attempt,
                        via,
                    }],
                ))
            }
            (S::Loading { .. }, V::LoadRequested { .. }) => refused(R::SessionLoading),
            (S::Live { .. }, V::LoadRequested { .. }) => refused(R::AlreadyLoaded),
            (S::Closing { .. }, V::LoadRequested { .. }) => refused(R::SessionClosing),
            (
                S::Loading {
                    generation,
                    attempt: current,
                    ..
                },
                V::Loaded { attempt },
            ) if current == attempt => Ok((
                S::Live { generation },
                vec![
                    E::RegisterActor { generation },
                    E::PublishActivation { generation },
                ],
            )),
            (
                S::Loading {
                    generation,
                    attempt: current,
                    ..
                },
                V::LoadFailed { attempt },
            ) if current == attempt => Ok((S::NotLoaded { last: generation }, Vec::new())),
            (
                S::NotLoaded { .. } | S::Loading { .. } | S::Live { .. } | S::Closing { .. },
                V::Loaded { .. } | V::LoadFailed { .. },
            ) => Err(TransitionError::Stale),
            (
                S::Live { generation },
                V::CloseRequested {
                    attempt,
                    turn_running: true,
                },
            ) => Ok((
                S::Closing {
                    generation,
                    from: CloseOrigin::Live,
                    phase: ClosePhase::EndingTurn,
                    attempt,
                },
                vec![E::EndLine { generation }, E::CancelTurn { generation }],
            )),
            (
                S::Live { generation },
                V::CloseRequested {
                    attempt,
                    turn_running: false,
                },
            ) => Ok((
                S::Closing {
                    generation,
                    from: CloseOrigin::Live,
                    phase: ClosePhase::Freeing,
                    attempt,
                },
                vec![
                    E::EndLine { generation },
                    E::StartFree {
                        generation,
                        attempt,
                    },
                ],
            )),
            (S::Loading { generation, .. }, V::CloseRequested { attempt, .. }) => Ok((
                S::Closing {
                    generation,
                    from: CloseOrigin::Loading,
                    phase: ClosePhase::Freeing,
                    attempt,
                },
                vec![
                    E::EndLine { generation },
                    E::StartFree {
                        generation,
                        attempt,
                    },
                ],
            )),
            (S::NotLoaded { .. }, V::CloseRequested { .. }) => refused(R::NothingToClose),
            (S::Closing { .. }, V::CloseRequested { .. }) => refused(R::CloseInProgress),
            (
                S::Closing {
                    generation,
                    from,
                    phase: ClosePhase::EndingTurn,
                    attempt,
                },
                V::TurnEnded { generation: ended },
            ) if ended == generation => Ok((
                S::Closing {
                    generation,
                    from,
                    phase: ClosePhase::Freeing,
                    attempt,
                },
                vec![E::StartFree {
                    generation,
                    attempt,
                }],
            )),
            (
                S::NotLoaded { .. } | S::Loading { .. } | S::Live { .. } | S::Closing { .. },
                V::TurnEnded { .. },
            ) => Err(TransitionError::Stale),
            (
                S::Closing {
                    generation,
                    phase: ClosePhase::Freeing,
                    attempt: current,
                    ..
                },
                V::CloseDone { attempt },
            ) if current == attempt => Ok((
                S::NotLoaded { last: generation },
                vec![E::PublishClosed { generation }],
            )),
            (
                S::Closing {
                    generation,
                    from: CloseOrigin::Live,
                    phase: ClosePhase::Freeing,
                    attempt: current,
                },
                V::CloseFailed { attempt },
            ) if current == attempt => {
                Ok((S::Live { generation }, vec![E::ReopenLine { generation }]))
            }
            (
                S::Closing {
                    generation,
                    from: CloseOrigin::Loading,
                    phase: ClosePhase::Freeing,
                    attempt: current,
                },
                V::CloseFailed { attempt },
            ) if current == attempt => Ok((S::NotLoaded { last: generation }, Vec::new())),
            (
                S::NotLoaded { .. } | S::Loading { .. } | S::Live { .. } | S::Closing { .. },
                V::CloseDone { .. } | V::CloseFailed { .. },
            ) => Err(TransitionError::Stale),
            (S::Live { generation }, V::ActorExited { generation: exited })
                if exited == generation =>
            {
                Ok((
                    S::NotLoaded { last: generation },
                    vec![E::PublishUnloaded { generation }],
                ))
            }
            (
                S::NotLoaded { .. } | S::Loading { .. } | S::Live { .. } | S::Closing { .. },
                V::ActorExited { .. },
            ) => Err(TransitionError::Stale),
            (S::Live { generation }, V::PromptRequested) => {
                Ok((S::Live { generation }, Vec::new()))
            }
            (S::NotLoaded { .. } | S::Loading { .. }, V::PromptRequested) => refused(R::NotLoaded),
            (S::Closing { .. }, V::PromptRequested) => refused(R::SessionClosing),
            (
                S::Loading { generation, .. } | S::Live { generation },
                V::Publish { generation: from },
            ) if from == generation => Ok((*state, vec![E::PublishToHub { generation }])),
            (
                S::NotLoaded { .. } | S::Loading { .. } | S::Live { .. } | S::Closing { .. },
                V::Publish { .. },
            ) => Err(TransitionError::Stale),
        }
    }

    fn transitions() -> &'static [Transition] {
        TRANSITIONS
    }
}

#[cfg(test)]
#[path = "session_lifecycle_machine_test.rs"]
mod tests;
