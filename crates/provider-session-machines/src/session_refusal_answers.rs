//! The one-line answers (M8) for a busy turn and a held setting: the state
//! and the valid next steps, derived from the turn, queue-line and setting
//! machines' declared transitions with caller actions, a capability
//! predicate (steering) and the state's observations; never a hand-written
//! list (design note §12.8, §13.5).

use crate::queue_line_machine::QueueLineMachine;
use crate::session_lifecycle_answers::state_answer;
use crate::session_lifecycle_machine::SessionLifecycle;
use crate::session_setting_machine::{
    SessionSetting, SessionSettingMachine, SettingValueId, UnresolvedCause,
};
use crate::session_turn_machine::{SessionTurn, SessionTurnMachine};
use state_machine_core::{StateMachine, StateTag};

/// A step a caller can take on a busy session or a held setting, in the
/// order an answer names them: waiting first, then the gentlest change.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RefusalStep {
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

/// Observations a busy turn offers: they change no state (§13.5). A
/// running turn is acted on; a cancelling one, or a queued message about to
/// start its turn, can be waited for.
fn turn_observations(turn: SessionTurn) -> &'static [RefusalStep] {
    match turn {
        SessionTurn::Running { .. } => &[],
        SessionTurn::Idle | SessionTurn::Cancelling { .. } => &[RefusalStep::Wait],
    }
}

/// The valid next steps for a busy session in `turn`: the turn's caller
/// moves from that exact state (steer only when the agent advertises
/// steering), the open line's queue move, and the state's observations.
pub fn busy_steps(turn: SessionTurn, agent_steers: bool) -> Vec<RefusalStep> {
    let mut steps = turn_observations(turn).to_vec();
    actions_from(
        SessionTurnMachine::transitions(),
        turn.state_tag(),
        |step| step != RefusalStep::Steer || agent_steers,
        &mut steps,
    );
    // A Live session's line is open.
    actions_from(
        QueueLineMachine::transitions(),
        "open",
        |_| true,
        &mut steps,
    );
    steps.sort_unstable();
    steps
}

/// A Live session's busy answer: its turn's state, then the valid next
/// steps.
fn turn_answer(turn: SessionTurn, agent_steers: bool) -> String {
    let state = match turn {
        SessionTurn::Idle => "a queued message is being sent",
        SessionTurn::Running { .. } => "a turn is running",
        SessionTurn::Cancelling { .. } => "a turn is being cancelled",
    };
    format!("{state}; {}", joined(&busy_steps(turn, agent_steers)))
}

/// A busy session's answer: a session that isn't Live answers its
/// lifecycle state; a Live one answers its turn.
pub fn session_busy_answer(
    lifecycle: &SessionLifecycle,
    turn: SessionTurn,
    agent_steers: bool,
) -> String {
    match lifecycle {
        SessionLifecycle::Live { .. } => turn_answer(turn, agent_steers),
        SessionLifecycle::NotLoaded { .. }
        | SessionLifecycle::Loading { .. }
        | SessionLifecycle::Closing { .. } => state_answer(lifecycle),
    }
}

/// The valid next steps for a session setting in `setting`: its caller
/// moves from that exact state.
pub fn hold_steps(setting: &SessionSetting) -> Vec<RefusalStep> {
    let mut steps = Vec::new();
    actions_from(
        SessionSettingMachine::transitions(),
        setting.state_tag(),
        |_| true,
        &mut steps,
    );
    steps.sort_unstable();
    steps
}

/// Work refused because a restore couldn't put back the session's recorded
/// mode `recorded` and the agent runs `observed`, which allows more (Lead's
/// mode-mismatch rule under owner Decision 3).
pub fn mode_not_restored_answer(recorded: &SettingValueId, observed: &SettingValueId) -> String {
    format!(
        "the session's recorded mode `{}` couldn't be put back and the agent is in `{}`, which allows more; set the mode, or close the session",
        recorded.0, observed.0
    )
}

/// Work refused because a setting holds the session, held for `cause`.
pub fn settings_hold_answer(cause: UnresolvedCause) -> String {
    let state = match &cause {
        UnresolvedCause::DefiniteFailure => "a setting change was refused",
        UnresolvedCause::OutcomeUnknown => "a setting change's outcome is unknown",
        UnresolvedCause::ModeNotRestored { .. } => "the session's recorded mode wasn't put back",
    };
    let held = SessionSetting::Unresolved {
        cause,
        observed: SettingValueId(String::new()),
    };
    format!("{state}; {}", joined(&hold_steps(&held)))
}

#[cfg(test)]
#[path = "session_refusal_answers_test.rs"]
mod tests;
