//! One setting kind (mode, model, effort or fast) of one session run, as a
//! pure machine (D1 state machines; design note §11.6, §12.6). An observation
//! the agent sends on its own never clears a pending change or a hold; only
//! that change's own result does, or the caller accepting a refused change.

use state_machine_core::{
    EventTag, RefusalTag, StateMachine, StateMachineResult, StateTag, Transition, TransitionError,
};

/// A setting's value as the agent names it (an option or mode ID).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingValueId(pub String);

/// One change attempt's identity (a UUIDv7), so an older change's result
/// is recognised.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SettingApplication(pub uuid::Uuid);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UnresolvedCause {
    DefiniteFailure,
    OutcomeUnknown,
    /// A restore couldn't put back the session's recorded named mode, and
    /// the agent's mode allows more (Lead's mode-mismatch rule under owner
    /// Decision 3): only setting the mode, or closing, unblocks it.
    ModeNotRestored {
        recorded: SettingValueId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionSetting {
    Current {
        value: SettingValueId,
    },
    Changing {
        to: SettingValueId,
        application: SettingApplication,
        observed: SettingValueId,
    },
    Unresolved {
        cause: UnresolvedCause,
        observed: SettingValueId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionSettingEvent {
    SetRequested {
        to: SettingValueId,
        application: SettingApplication,
    },
    Applied {
        application: SettingApplication,
        value: SettingValueId,
    },
    /// The agent refused the change, and the kind holds prompts (fast, or a
    /// setting a create couldn't apply).
    Failed {
        application: SettingApplication,
    },
    /// The agent refused the change, and the kind holds nothing (mode, model
    /// or effort changed live, as today): it keeps the value it had.
    Refused {
        application: SettingApplication,
    },
    /// The change's reply never came back: its outcome is unknown, whatever
    /// the kind was before.
    Lost,
    OutcomeUnknown {
        application: SettingApplication,
    },
    Observed {
        value: SettingValueId,
    },
    Accept,
    /// A restore couldn't put back `recorded`, and the agent runs `observed`,
    /// which allows more.
    ModeNotRestored {
        recorded: SettingValueId,
        observed: SettingValueId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionSettingEffect {
    SendChange {
        to: SettingValueId,
        application: SettingApplication,
    },
    /// The run's settings snapshot changed; publish it (and recompute the
    /// grant when this is the mode, design note §12.6).
    PublishSettings,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionSettingRefusal {
    ChangeInProgress,
    OutcomeUnknown,
    NothingToAccept,
    /// The recorded mode wasn't put back: accepting the agent's mode isn't
    /// allowed.
    ModeMustBeSet,
}

impl RefusalTag for SessionSettingRefusal {
    fn refusal_tag(&self) -> &'static str {
        match self {
            Self::ChangeInProgress => "change_in_progress",
            Self::OutcomeUnknown => "outcome_unknown",
            Self::NothingToAccept => "nothing_to_accept",
            Self::ModeMustBeSet => "mode_must_be_set",
        }
    }
}

impl SessionSettingRefusal {
    pub const fn human_phrase(self) -> &'static str {
        match self {
            Self::ChangeInProgress => "a setting change is in progress; wait for it",
            Self::OutcomeUnknown => "a setting change's outcome is unknown; set a value explicitly",
            Self::NothingToAccept => "the setting is current; nothing to accept",
            Self::ModeMustBeSet => "set the mode or close the session",
        }
    }
}

/// Why a prompt must wait, if this setting holds prompts (design note §12.6:
/// any Changing or Unresolved holds them).
pub fn prompt_hold(state: &SessionSetting) -> Option<&'static str> {
    match state {
        SessionSetting::Current { .. } => None,
        SessionSetting::Changing { .. } => Some("settings are changing; wait for them"),
        SessionSetting::Unresolved {
            cause: UnresolvedCause::DefiniteFailure,
            ..
        } => Some("a setting change was refused; accept the current value or set it again"),
        SessionSetting::Unresolved {
            cause: UnresolvedCause::OutcomeUnknown,
            ..
        } => Some("a setting change's outcome is unknown; set a value explicitly"),
        SessionSetting::Unresolved {
            cause: UnresolvedCause::ModeNotRestored { .. },
            ..
        } => Some("the session's mode wasn't put back; set the mode or close the session"),
    }
}

impl StateTag for SessionSetting {
    fn state_tag(&self) -> &'static str {
        match self {
            Self::Current { .. } => "current",
            Self::Changing { .. } => "changing",
            Self::Unresolved {
                cause: UnresolvedCause::DefiniteFailure,
                ..
            } => "unresolved_refused",
            Self::Unresolved {
                cause: UnresolvedCause::OutcomeUnknown,
                ..
            } => "unresolved_unknown",
            Self::Unresolved {
                cause: UnresolvedCause::ModeNotRestored { .. },
                ..
            } => "unresolved_mode_not_restored",
        }
    }

    fn is_terminal(&self) -> bool {
        match self {
            Self::Current { .. } | Self::Changing { .. } | Self::Unresolved { .. } => false,
        }
    }
}

impl EventTag for SessionSettingEvent {
    fn event_tag(&self) -> &'static str {
        match self {
            Self::SetRequested { .. } => "set_requested",
            Self::Applied { .. } => "applied",
            Self::Failed { .. } => "failed",
            Self::Refused { .. } => "refused",
            Self::Lost => "lost",
            Self::OutcomeUnknown { .. } => "outcome_unknown",
            Self::Observed { .. } => "observed",
            Self::Accept => "accept",
            Self::ModeNotRestored { .. } => "mode_not_restored",
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
    edge("current", "set_requested", "changing"),
    edge("unresolved_refused", "set_requested", "changing"),
    edge("unresolved_unknown", "set_requested", "changing"),
    edge("unresolved_mode_not_restored", "set_requested", "changing"),
    edge("changing", "applied", "current"),
    edge("changing", "failed", "unresolved_refused"),
    edge("changing", "refused", "current"),
    edge("current", "lost", "unresolved_unknown"),
    edge("changing", "lost", "unresolved_unknown"),
    edge("unresolved_refused", "lost", "unresolved_unknown"),
    edge("unresolved_unknown", "lost", "unresolved_unknown"),
    edge("unresolved_mode_not_restored", "lost", "unresolved_unknown"),
    edge("changing", "outcome_unknown", "unresolved_unknown"),
    edge("current", "observed", "current"),
    edge("changing", "observed", "changing"),
    edge("unresolved_refused", "observed", "unresolved_refused"),
    edge("unresolved_unknown", "observed", "unresolved_unknown"),
    edge(
        "unresolved_mode_not_restored",
        "observed",
        "unresolved_mode_not_restored",
    ),
    edge("unresolved_refused", "accept", "current"),
    edge(
        "current",
        "mode_not_restored",
        "unresolved_mode_not_restored",
    ),
    edge(
        "changing",
        "mode_not_restored",
        "unresolved_mode_not_restored",
    ),
    edge(
        "unresolved_refused",
        "mode_not_restored",
        "unresolved_mode_not_restored",
    ),
    edge(
        "unresolved_unknown",
        "mode_not_restored",
        "unresolved_mode_not_restored",
    ),
    edge(
        "unresolved_mode_not_restored",
        "mode_not_restored",
        "unresolved_mode_not_restored",
    ),
];

pub struct SessionSettingMachine;

fn refused(refusal: SessionSettingRefusal) -> StateMachineResult<SessionSettingMachine> {
    Err(TransitionError::Domain(refusal))
}

impl StateMachine for SessionSettingMachine {
    type State = SessionSetting;
    type Event = SessionSettingEvent;
    type Effect = SessionSettingEffect;
    type Error = SessionSettingRefusal;

    fn transition(state: &SessionSetting, event: SessionSettingEvent) -> StateMachineResult<Self> {
        use SessionSetting as S;
        use SessionSettingEffect as E;
        use SessionSettingEvent as V;
        use SessionSettingRefusal as R;
        let observed_now = |state: &SessionSetting| match state {
            S::Current { value } => value.clone(),
            S::Changing { observed, .. } | S::Unresolved { observed, .. } => observed.clone(),
        };
        match (state, event) {
            (S::Current { .. } | S::Unresolved { .. }, V::SetRequested { to, application }) => {
                Ok((
                    S::Changing {
                        to: to.clone(),
                        application,
                        observed: observed_now(state),
                    },
                    vec![E::SendChange { to, application }],
                ))
            }
            (S::Changing { .. }, V::SetRequested { .. }) => refused(R::ChangeInProgress),
            (
                S::Changing {
                    application: current,
                    ..
                },
                V::Applied { application, value },
            ) if *current == application => Ok((S::Current { value }, vec![E::PublishSettings])),
            (
                S::Changing {
                    application: current,
                    observed,
                    ..
                },
                V::Failed { application },
            ) if *current == application => Ok((
                S::Unresolved {
                    cause: UnresolvedCause::DefiniteFailure,
                    observed: observed.clone(),
                },
                Vec::new(),
            )),
            (
                S::Changing {
                    application: current,
                    observed,
                    ..
                },
                V::Refused { application },
            ) if *current == application => Ok((
                S::Current {
                    value: observed.clone(),
                },
                Vec::new(),
            )),
            (S::Current { .. } | S::Changing { .. } | S::Unresolved { .. }, V::Lost) => Ok((
                S::Unresolved {
                    cause: UnresolvedCause::OutcomeUnknown,
                    observed: observed_now(state),
                },
                Vec::new(),
            )),
            (
                S::Changing {
                    application: current,
                    observed,
                    ..
                },
                V::OutcomeUnknown { application },
            ) if *current == application => Ok((
                S::Unresolved {
                    cause: UnresolvedCause::OutcomeUnknown,
                    observed: observed.clone(),
                },
                Vec::new(),
            )),
            (
                S::Current { .. } | S::Changing { .. } | S::Unresolved { .. },
                V::Applied { .. } | V::Failed { .. } | V::Refused { .. } | V::OutcomeUnknown { .. },
            ) => Err(TransitionError::Stale),
            (S::Current { .. }, V::Observed { value }) => {
                Ok((S::Current { value }, vec![E::PublishSettings]))
            }
            (
                S::Changing {
                    to, application, ..
                },
                V::Observed { value },
            ) => Ok((
                S::Changing {
                    to: to.clone(),
                    application: *application,
                    observed: value,
                },
                vec![E::PublishSettings],
            )),
            (S::Unresolved { cause, .. }, V::Observed { value }) => Ok((
                S::Unresolved {
                    cause: cause.clone(),
                    observed: value,
                },
                vec![E::PublishSettings],
            )),
            (
                S::Unresolved {
                    cause: UnresolvedCause::DefiniteFailure,
                    observed,
                },
                V::Accept,
            ) => Ok((
                S::Current {
                    value: observed.clone(),
                },
                vec![E::PublishSettings],
            )),
            (
                S::Unresolved {
                    cause: UnresolvedCause::OutcomeUnknown,
                    ..
                },
                V::Accept,
            ) => refused(R::OutcomeUnknown),
            (
                S::Unresolved {
                    cause: UnresolvedCause::ModeNotRestored { .. },
                    ..
                },
                V::Accept,
            ) => refused(R::ModeMustBeSet),
            (_, V::ModeNotRestored { recorded, observed }) => Ok((
                S::Unresolved {
                    cause: UnresolvedCause::ModeNotRestored { recorded },
                    observed,
                },
                vec![E::PublishSettings],
            )),
            (S::Changing { .. }, V::Accept) => refused(R::ChangeInProgress),
            (S::Current { .. }, V::Accept) => refused(R::NothingToAccept),
        }
    }

    fn transitions() -> &'static [Transition] {
        TRANSITIONS
    }
}

#[cfg(test)]
#[path = "session_setting_machine_test.rs"]
mod tests;
