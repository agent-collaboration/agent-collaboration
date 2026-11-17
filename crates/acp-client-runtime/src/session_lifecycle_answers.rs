//! A refused lifecycle request's answer (M8): the session's current state and
//! the valid next steps, derived from the machine's declared transitions plus
//! the state's observations, never a hand-written list (design note §12.8,
//! §13.5).

use crate::session_lifecycle_machine::{
    SessionLifecycle, SessionLifecycleMachine, SessionLifecycleRefusal,
};
use state_machine_core::{StateMachine, StateTag};

/// Something a caller can do next.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum CallerStep {
    /// A request that moves the session (a declared transition).
    Load,
    Resume,
    Close,
    Prompt,
    /// An observation: it changes no state, so it is not a transition.
    Wait,
}

impl CallerStep {
    const fn phrase(self) -> &'static str {
        match self {
            Self::Load => "load it",
            Self::Resume => "resume it",
            Self::Close => "close it",
            Self::Prompt => "send it a prompt",
            Self::Wait => "wait for it",
        }
    }
}

/// The caller request a declared transition's event stands for; internal
/// events (results, exits, publications) have none.
fn caller_request(event_tag: &str) -> Option<CallerStep> {
    match event_tag {
        "load_requested" => Some(CallerStep::Load),
        "resume_requested" => Some(CallerStep::Resume),
        "close_requested" => Some(CallerStep::Close),
        "prompt_requested" => Some(CallerStep::Prompt),
        _ => None,
    }
}

/// Observations a caller can make in a state.
fn observations(state: &SessionLifecycle) -> &'static [CallerStep] {
    match state {
        SessionLifecycle::Loading { .. } | SessionLifecycle::Closing { .. } => &[CallerStep::Wait],
        SessionLifecycle::NotLoaded { .. } | SessionLifecycle::Live { .. } => &[],
    }
}

/// The valid next steps from `state`: observations first, then the caller
/// requests among the declared transitions leaving this state's tag.
pub(crate) fn next_steps(state: &SessionLifecycle) -> Vec<CallerStep> {
    let mut steps: Vec<CallerStep> = observations(state).to_vec();
    let from = state.state_tag();
    for transition in SessionLifecycleMachine::transitions() {
        if transition.from == from
            && let Some(step) = caller_request(transition.event)
            && !steps.contains(&step)
        {
            steps.push(step);
        }
    }
    steps
}

fn state_phrase(state: &SessionLifecycle) -> &'static str {
    match state {
        SessionLifecycle::NotLoaded { .. } => "session is not loaded",
        SessionLifecycle::Loading { .. } => "session is loading",
        SessionLifecycle::Live { .. } => "session is already loaded",
        SessionLifecycle::Closing { .. } => "session is closing",
    }
}

fn joined(steps: &[CallerStep]) -> String {
    match steps {
        [] => "nothing can be done right now".to_owned(),
        [only] => only.phrase().to_owned(),
        [init @ .., last] => format!(
            "{} or {}",
            init.iter()
                .map(|step| step.phrase())
                .collect::<Vec<_>>()
                .join(", "),
            last.phrase()
        ),
    }
}

/// The one-line answer for a refusal: the reason where the state alone
/// doesn't say it, the state, then the valid next steps.
pub(crate) fn refusal_answer(state: &SessionLifecycle, refusal: SessionLifecycleRefusal) -> String {
    let next = joined(&next_steps(state));
    match refusal {
        SessionLifecycleRefusal::DuplicateIdentity => {
            format!(
                "the agent returned a session identity already in use; {}; {next}",
                state_phrase(state)
            )
        }
        SessionLifecycleRefusal::NothingToClose => {
            format!("{}; nothing to close; {next}", state_phrase(state))
        }
        SessionLifecycleRefusal::CloseInProgress => format!("close in progress; {next}"),
        SessionLifecycleRefusal::SessionLoading
        | SessionLifecycleRefusal::AlreadyLoaded
        | SessionLifecycleRefusal::SessionClosing
        | SessionLifecycleRefusal::NotLoaded => format!("{}; {next}", state_phrase(state)),
    }
}

#[cfg(test)]
#[path = "session_lifecycle_answers_test.rs"]
mod tests;
