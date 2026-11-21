//! The one-line answers (M8) for a busy turn and a held setting: the state
//! and the valid next steps, derived from the turn, queue-line and setting
//! machines' declared transitions with caller actions, a capability
//! predicate (steering) and the state's observations; never a hand-written
//! list (design note §12.8, §13.5).

use crate::queue_line_machine::QueueLineMachine;
use crate::session_lifecycle_answers::state_answer;
use crate::session_lifecycle_machine::SessionLifecycle;
use crate::session_owner::{EffectInProgress, SessionSnapshot};
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

pub fn effect_in_progress_answer(effect: EffectInProgress, snapshot: &SessionSnapshot) -> String {
    match effect {
        EffectInProgress::TurnInProgress => {
            return session_busy_answer(
                &snapshot.lifecycle,
                snapshot.turn,
                snapshot
                    .projection
                    .capabilities
                    .as_ref()
                    .is_some_and(|report| report.steer),
            );
        }
        EffectInProgress::PromptDispatch => {
            "the Router is preparing a prompt. Wait for it before sending again"
        }
        EffectInProgress::QueuedPrompts => {
            "messages are waiting in the queue. Wait for them to be sent"
        }
        EffectInProgress::SettingChange => {
            "a setting is being changed. Wait for the change before sending again"
        }
        EffectInProgress::Steer => {
            "a steer is in progress. Wait for its result before steering again"
        }
        EffectInProgress::Cancel => "a cancellation is in progress. Wait for its result",
        EffectInProgress::Create => {
            "the Router is creating the session. Wait for the create result"
        }
        EffectInProgress::ProviderDisconnected => {
            "the provider connection ended. Reconnect, then retry the session request"
        }
        EffectInProgress::CreateOnActiveOwner => {
            "the session already has an owner. Use the existing session"
        }
        EffectInProgress::SettingCancelTooLate => {
            "the setting can no longer be cancelled. Wait for its result"
        }
    }
    .to_owned()
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

/// Work refused because a setting holds the session, held for `cause`.
pub fn settings_hold_answer(cause: UnresolvedCause) -> String {
    let state = match cause {
        UnresolvedCause::DefiniteFailure => "a setting change was refused",
        UnresolvedCause::OutcomeUnknown => "a setting change's outcome is unknown",
    };
    let held = SessionSetting::Unresolved {
        cause,
        observed: SettingValueId(String::new()),
    };
    format!("{state}; {}", joined(&hold_steps(&held)))
}

/// What the caller can observe when no setting move is currently eligible.
pub enum SettingWaitObservation {
    Change,
    EarlierWriteReply,
}

/// The pending setting write and the eligible caller moves from its actual state.
pub fn setting_in_progress_answer(
    setting: Option<&SessionSetting>,
    eligible: impl Fn(RefusalStep) -> bool,
    wait: SettingWaitObservation,
) -> String {
    let mut steps = setting.map_or_else(Vec::new, hold_steps);
    steps.retain(|step| eligible(*step));
    let next = if steps.is_empty() {
        match wait {
            SettingWaitObservation::Change => RefusalStep::Wait.phrase(),
            SettingWaitObservation::EarlierWriteReply => "wait for the earlier write's reply",
        }
        .to_owned()
    } else {
        joined(&steps)
    };
    format!("a setting change is in progress; {next}")
}

#[cfg(test)]
#[path = "session_refusal_answers_test.rs"]
mod tests;
