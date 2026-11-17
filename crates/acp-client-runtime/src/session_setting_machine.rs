//! One setting kind (mode, model, effort or fast) of one session run, as a
//! pure machine (D1 state machines; design note §11.6, §12.6). An observation
//! the agent sends on its own never clears a pending change or a hold; only
//! that change's own result does, or the caller accepting a refused change.

use state_machine_core::{
    EventTag, RefusalTag, StateMachine, StateMachineResult, StateTag, Transition, TransitionError,
};

/// A setting's value as the agent names it (an option or mode ID).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SettingValueId(pub(crate) String);

/// One change attempt's identity (a UUIDv7), so an older change's result
/// is recognised.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct SettingApplication(pub(crate) uuid::Uuid);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UnresolvedCause {
    DefiniteFailure,
    OutcomeUnknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SessionSetting {
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
pub(crate) enum SessionSettingEvent {
    SetRequested {
        to: SettingValueId,
        application: SettingApplication,
    },
    Applied {
        application: SettingApplication,
        value: SettingValueId,
    },
    Failed {
        application: SettingApplication,
    },
    OutcomeUnknown {
        application: SettingApplication,
    },
    Observed {
        value: SettingValueId,
    },
    Accept,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SessionSettingEffect {
    SendChange {
        to: SettingValueId,
        application: SettingApplication,
    },
    /// The run's settings snapshot changed; publish it (and recompute the
    /// grant when this is the mode, design note §12.6).
    PublishSettings,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SessionSettingRefusal {
    ChangeInProgress,
    OutcomeUnknown,
    NothingToAccept,
}

impl RefusalTag for SessionSettingRefusal {
    fn refusal_tag(&self) -> &'static str {
        match self {
            Self::ChangeInProgress => "change_in_progress",
            Self::OutcomeUnknown => "outcome_unknown",
            Self::NothingToAccept => "nothing_to_accept",
        }
    }
}

impl SessionSettingRefusal {
    pub(crate) const fn human_phrase(self) -> &'static str {
        match self {
            Self::ChangeInProgress => "a setting change is in progress; wait for it",
            Self::OutcomeUnknown => "a setting change's outcome is unknown; set a value explicitly",
            Self::NothingToAccept => "the setting is current; nothing to accept",
        }
    }
}

/// Why a prompt must wait, if this setting holds prompts (design note §12.6:
/// any Changing or Unresolved holds them).
pub(crate) fn prompt_hold(state: &SessionSetting) -> Option<&'static str> {
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
            Self::OutcomeUnknown { .. } => "outcome_unknown",
            Self::Observed { .. } => "observed",
            Self::Accept => "accept",
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
    edge("changing", "applied", "current"),
    edge("changing", "failed", "unresolved_refused"),
    edge("changing", "outcome_unknown", "unresolved_unknown"),
    edge("current", "observed", "current"),
    edge("changing", "observed", "changing"),
    edge("unresolved_refused", "observed", "unresolved_refused"),
    edge("unresolved_unknown", "observed", "unresolved_unknown"),
    edge("unresolved_refused", "accept", "current"),
];

pub(crate) struct SessionSettingMachine;

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
                V::Applied { .. } | V::Failed { .. } | V::OutcomeUnknown { .. },
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
                    cause: *cause,
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
