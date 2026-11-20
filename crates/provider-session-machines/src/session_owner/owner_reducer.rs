use super::{
    RemoteEffect, RunAttempt, SessionDeadline, SessionGeneration, SessionLifecycle,
    SessionOwnerCommand, SessionOwnerCommit, SessionOwnerDecision, SessionOwnerEffect,
    SessionOwnerEvent, SessionOwnerRefusal, SessionOwnerRejection as OwnerRejection,
    SessionOwnerState, SessionStoreResult, SessionWireResult, WireOutcome,
};
use crate::session_cell_machine::{SessionCellEffect, SessionCellEvent, SessionCellMachine};
use state_machine_core::{StateMachine, TransitionError};

mod input_effects;
mod lifecycle_effects;
mod setting_effects;
mod tool_effects;

impl SessionOwnerState {
    /// Reduces one mailbox event. Discarded events release no executor work.
    pub fn decide(&self, event: SessionOwnerEvent) -> SessionOwnerDecision {
        let mut transition = OwnerTransition {
            next: self.clone(),
            effects: Vec::new(),
        };
        transition.apply(event)?;
        Ok(SessionOwnerCommit {
            next: transition.next,
            effects: transition.effects,
        })
    }
}

struct OwnerTransition {
    next: SessionOwnerState,
    effects: Vec<SessionOwnerEffect>,
}

type OwnerResult<T = ()> = Result<T, OwnerRejection>;

impl From<SessionOwnerRefusal> for OwnerRejection {
    fn from(reason: SessionOwnerRefusal) -> Self {
        Self::Refused(reason)
    }
}

impl OwnerTransition {
    fn apply(&mut self, event: SessionOwnerEvent) -> OwnerResult {
        match event {
            SessionOwnerEvent::Command(command) => self.apply_command(command),
            SessionOwnerEvent::Wire(result) => self.apply_wire(result),
            SessionOwnerEvent::Store(result) => self.apply_store(result),
            SessionOwnerEvent::Publication { run, outcome } => {
                self.activation_published(run, outcome)
            }
            SessionOwnerEvent::DeadlineElapsed(deadline) => self.apply_deadline(deadline),
            SessionOwnerEvent::RoutedTool(event) => self.apply_tool(event),
        }
    }

    fn apply_command(&mut self, command: SessionOwnerCommand) -> OwnerResult {
        match command {
            SessionOwnerCommand::Restore { attempt, via } => self.start_restore(attempt, via),
            SessionOwnerCommand::Close {
                generation,
                attempt,
            } => self.start_close(RunAttempt {
                generation,
                attempt,
            }),
            SessionOwnerCommand::Prompt { input, admission } => self.admit_prompt(input, admission),
            SessionOwnerCommand::Steer { input, supported } => self.admit_steer(input, supported),
            SessionOwnerCommand::CancelTurn { generation, turn } => {
                self.cancel_turn(generation, turn)
            }
            SessionOwnerCommand::CancelQueued { generation, input } => {
                self.cancel_queued(generation, &input)
            }
            SessionOwnerCommand::Set { setting, value } => self.start_setting(setting, value),
            SessionOwnerCommand::AcceptSettings { generation } => {
                self.require_live(generation)?;
                self.cell_move(SessionCellEvent::AcceptSettings)?;
                self.drive_prompt_line()
            }
        }
    }

    fn apply_wire(&mut self, result: SessionWireResult) -> OwnerResult {
        match result {
            SessionWireResult::Restore { run, outcome } => self.restore_on_wire(run, outcome),
            SessionWireResult::RestoreMode { run, outcome } => {
                self.restore_mode_on_wire(run, outcome)
            }
            SessionWireResult::Close { run, outcome } => self.close_on_wire(run, outcome),
            SessionWireResult::Turn { input, outcome } => self.finish_prompt(&input, outcome),
            SessionWireResult::Steer { input, outcome } => self.finish_steer(&input, outcome),
            SessionWireResult::Setting {
                setting,
                observation,
            } => self.setting_on_wire(setting, observation),
        }
    }

    fn apply_store(&mut self, result: SessionStoreResult) -> OwnerResult {
        match result {
            SessionStoreResult::RestoreSetup { run, outcome } => {
                self.restore_setup_read(run, outcome)
            }
            SessionStoreResult::Dispatch { input, outcome } => {
                self.dispatch_marked(&input, outcome)
            }
            SessionStoreResult::UndoDispatch { input, outcome } => {
                self.dispatch_unmarked(&input, outcome)
            }
            SessionStoreResult::SteerDispatch { input, outcome } => {
                self.steer_marked(&input, outcome)
            }
            SessionStoreResult::ConfirmedSetting { setting, outcome } => {
                self.setting_recorded(setting, outcome)
            }
            SessionStoreResult::DispatchReadback { .. } => Err(OwnerRejection::IgnoredLate),
        }
    }

    fn apply_deadline(&mut self, deadline: SessionDeadline) -> OwnerResult {
        match deadline {
            SessionDeadline::Restore { run, wait } => self.restore_deadline(run, wait),
            SessionDeadline::Close(run) => self.close_deadline(run),
            SessionDeadline::Prompt { input, wait } => self.prompt_deadline(&input, wait),
            SessionDeadline::Steer { input, wait } => self.steer_deadline(&input, wait),
            SessionDeadline::Setting { setting, wait } => self.setting_deadline(setting, wait),
        }
    }

    fn cell_move(&mut self, event: SessionCellEvent) -> OwnerResult<Vec<SessionCellEffect>> {
        let (next, effects) = SessionCellMachine::transition(&self.next.cell, event).map_err(
            |error| match error {
                TransitionError::Stale => OwnerRejection::IgnoredLate,
                refusal => OwnerRejection::Refused(SessionOwnerRefusal::Cell(refusal)),
            },
        )?;
        self.next.cell = next;
        Ok(effects)
    }

    fn require_live(&self, expected: SessionGeneration) -> OwnerResult {
        self.is_live(expected)
            .then_some(())
            .ok_or(SessionOwnerRefusal::GenerationChanged.into())
    }

    fn is_live(&self, expected: SessionGeneration) -> bool {
        matches!(self.next.lifecycle(), SessionLifecycle::Live { generation } if generation == expected)
    }

    fn release(&mut self, effect: SessionOwnerEffect) {
        self.effects.push(effect);
    }
}

fn remote_effect(outcome: &WireOutcome<()>) -> RemoteEffect {
    match outcome {
        WireOutcome::Confirmed(()) => RemoteEffect::Applied,
        WireOutcome::Refused => RemoteEffect::None,
        WireOutcome::Unknown => RemoteEffect::Unknown,
    }
}
