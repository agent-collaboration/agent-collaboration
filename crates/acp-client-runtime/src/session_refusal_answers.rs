//! The one-line answers (M8) for a busy turn and a held setting: the state
//! and the valid next steps, derived from the turn, queue-line and setting
//! machines' declared transitions with caller actions, a capability
//! predicate (steering) and the state's observations; never a hand-written
//! list (design note §12.8, §13.5).

use crate::queue_line_machine::QueueLineMachine;
use crate::session_lifecycle_answers::state_answer;
use crate::session_lifecycle_machine::SessionLifecycle;
use crate::session_setting_machine::{SessionSettingMachine, UnresolvedCause};
use crate::session_turn_machine::{SessionTurn, SessionTurnMachine};
use state_machine_core::StateMachine;

/// A step a caller can take on a busy session or a held setting, in the
/// order an answer names them: waiting first, then the gentlest change.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum RefusalStep {
    Wait,
    Steer,
    Queue,
    Cancel,
    Set,
    Accept,
}

impl RefusalStep {
    const fn phrase(self) -> &'static str {
        match self {
            Self::Steer => "steer it",
            Self::Queue => "queue the message",
            Self::Cancel => "cancel the turn",
            Self::Wait => "wait for it",
            Self::Accept => "accept the current value",
            Self::Set => "set a value explicitly",
        }
    }
}

fn joined(steps: &[RefusalStep]) -> String {
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

/// The caller action a declared transition's event stands for.
fn caller_action(event_tag: &str) -> Option<RefusalStep> {
    match event_tag {
        "steer_requested" => Some(RefusalStep::Steer),
        "cancel_requested" => Some(RefusalStep::Cancel),
        "enqueue" => Some(RefusalStep::Queue),
        "accept" => Some(RefusalStep::Accept),
        "set_requested" => Some(RefusalStep::Set),
        _ => None,
    }
}

fn actions_from(
    transitions: &[state_machine_core::Transition],
    from: &str,
    eligible: impl Fn(RefusalStep) -> bool,
    steps: &mut Vec<RefusalStep>,
) {
    for transition in transitions {
        if transition.from == from
            && let Some(step) = caller_action(transition.event)
            && eligible(step)
            && !steps.contains(&step)
        {
            steps.push(step);
        }
    }
}

/// The valid next steps while a turn runs (`cancelling`: its cancel is in
/// flight): steer only when the agent advertises steering; a message can
/// always join the run's open line.
pub(crate) fn busy_steps(cancelling: bool, agent_steers: bool) -> Vec<RefusalStep> {
    let mut steps = Vec::new();
    if cancelling {
        steps.push(RefusalStep::Wait);
    }
    let from = if cancelling { "cancelling" } else { "running" };
    actions_from(
        SessionTurnMachine::transitions(),
        from,
        |step| step != RefusalStep::Steer || agent_steers,
        &mut steps,
    );
    actions_from(
        QueueLineMachine::transitions(),
        "open",
        |_| true,
        &mut steps,
    );
    steps.sort_unstable();
    steps
}

/// A prompt refused because a turn is running.
pub fn turn_busy_answer(agent_steers: bool) -> String {
    format!(
        "a turn is running; {}",
        joined(&busy_steps(false, agent_steers))
    )
}

/// A busy session's answer: a session that isn't Live answers its
/// lifecycle state; a Live one answers its turn, a cancelling turn taking
/// no second cancel.
pub(crate) fn session_busy_answer(
    lifecycle: &SessionLifecycle,
    turn: SessionTurn,
    agent_steers: bool,
) -> String {
    match (lifecycle, turn) {
        (SessionLifecycle::Live { .. }, SessionTurn::Cancelling { .. }) => format!(
            "a turn is being cancelled; {}",
            joined(&busy_steps(true, agent_steers))
        ),
        (SessionLifecycle::Live { .. }, SessionTurn::Running { .. } | SessionTurn::Idle) => {
            turn_busy_answer(agent_steers)
        }
        (
            SessionLifecycle::NotLoaded { .. }
            | SessionLifecycle::Loading { .. }
            | SessionLifecycle::Closing { .. },
            _,
        ) => state_answer(lifecycle),
    }
}

/// The valid next steps for a held setting.
pub(crate) fn hold_steps(cause: UnresolvedCause) -> Vec<RefusalStep> {
    let from = match cause {
        UnresolvedCause::DefiniteFailure => "unresolved_refused",
        UnresolvedCause::OutcomeUnknown => "unresolved_unknown",
    };
    let mut steps = Vec::new();
    actions_from(
        SessionSettingMachine::transitions(),
        from,
        |_| true,
        &mut steps,
    );
    steps.sort_unstable();
    steps
}

/// Work refused because a setting holds the session.
pub fn settings_hold_answer(outcome_unknown: bool) -> String {
    let (state, cause) = if outcome_unknown {
        (
            "a setting change's outcome is unknown",
            UnresolvedCause::OutcomeUnknown,
        )
    } else {
        (
            "a setting change was refused",
            UnresolvedCause::DefiniteFailure,
        )
    };
    format!("{state}; {}", joined(&hold_steps(cause)))
}

#[cfg(test)]
#[path = "session_refusal_answers_test.rs"]
mod tests;
