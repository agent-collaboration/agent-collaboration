use super::{
    CloseRequest, OwnerEffect, OwnerEvent, OwnerMailboxEvent, OwnerOutbox, RemoteEffect,
    RunAttempt, SessionGeneration, SessionLifecycle, SessionOwnerCommand, SessionOwnerCommit,
    SessionOwnerDecision, SessionOwnerEffect, SessionOwnerRefusal,
    SessionOwnerRejection as OwnerRejection, SessionOwnerState, SessionStoreResult,
    SessionWireResult, WireOutcome,
};
use crate::session_cell_machine::{SessionCellEffect, SessionCellEvent, SessionCellMachine};
use state_machine_core::{StateMachine, TransitionError};

mod dispatch_effects;
mod input_effects;
mod lifecycle_effects;
mod publication_effects;
mod registry_effects;
mod setting_effects;
mod store_effects;
mod tool_effects;

impl SessionOwnerState {
    /// Reduces one mailbox event. Discarded events release no executor work.
    pub fn decide(&self, event: OwnerMailboxEvent) -> SessionOwnerDecision {
        let attaching = event.event == OwnerEvent::ConnectionAttached
            && !self.ports.connection_alive
            && event.owner.incarnation == self.ports.address.incarnation
            && event.owner.connection_epoch != self.ports.address.connection_epoch;
        if event.owner != self.ports.address && !attaching {
            return Err(OwnerRejection::IgnoredLate);
        }
        let mut transition = OwnerTransition {
            next: self.clone(),
            effects: Vec::new(),
        };
        if transition.next.ports.latest_batch.is_none() {
            transition.offer_rebase()?;
        }
        if attaching {
            transition.next.ports.address = event.owner;
            transition.next.ports.connection_alive = true;
        } else {
            transition.apply(event.event)?;
        }
        Ok(SessionOwnerCommit {
            next: transition.next,
            effects: OwnerOutbox {
                owner: event.owner,
                effects: transition.effects,
            },
        })
    }
}

struct OwnerTransition {
    next: SessionOwnerState,
    effects: Vec<OwnerEffect>,
}

type OwnerResult<T = ()> = Result<T, OwnerRejection>;

impl From<SessionOwnerRefusal> for OwnerRejection {
    fn from(reason: SessionOwnerRefusal) -> Self {
        Self::Refused(reason)
    }
}

impl OwnerTransition {
    fn apply(&mut self, event: OwnerEvent) -> OwnerResult {
        match event {
            OwnerEvent::Command(command) => self.apply_command(command),
            OwnerEvent::Wire(result) => self.apply_wire(result),
            OwnerEvent::Store(result) => self.apply_store(result),
            OwnerEvent::Publication(result) => self.publication_result(result),
            OwnerEvent::Deadline(deadline) => self.port_deadline(deadline),
            OwnerEvent::RoutedTool(event) => self.routed_tool(event),
            OwnerEvent::Dispatch(result) => self.dispatch_result(result),
            OwnerEvent::Create(progress) => self.create_progress(progress),
            OwnerEvent::ConnectionLost => self.connection_lost(),
            OwnerEvent::ConnectionAttached => Err(OwnerRejection::IgnoredLate),
            OwnerEvent::Prepare(progress) => self.restore_preparation(progress),
        }
    }

    fn apply_command(&mut self, command: SessionOwnerCommand) -> OwnerResult {
        match command {
            SessionOwnerCommand::Create { operation } => self.start_create(operation),
            SessionOwnerCommand::Restore { attempt, via } => self.start_restore(attempt, via),
            SessionOwnerCommand::Close {
                generation,
                attempt,
            } => self.start_close(CloseRequest {
                run: RunAttempt {
                    generation,
                    attempt,
                },
                provider_supports_close: true,
            }),
            SessionOwnerCommand::CloseWithCapability(request) => self.start_close(request),
            SessionOwnerCommand::DependOnRestore { operation, run } => {
                self.depend_on_restore(operation, run)
            }
            SessionOwnerCommand::WarmPrompt(input) => self.admit_warm(input),
            SessionOwnerCommand::Prompt { input, admission } => self.admit_prompt(input, admission),
            SessionOwnerCommand::Steer { input, supported } => self.admit_steer(input, supported),
            SessionOwnerCommand::CancelTurn { generation, turn } => {
                self.cancel_turn(generation, turn)
            }
            SessionOwnerCommand::CancelQueued { generation, input } => {
                self.cancel_queued(generation, &input)
            }
            SessionOwnerCommand::Set {
                input,
                setting,
                value,
            } => self.start_setting(input, setting, value),
            SessionOwnerCommand::CancelSetting { input, setting } => {
                self.cancel_setting(&input, setting)
            }
            SessionOwnerCommand::AcceptSettings { generation } => {
                self.require_live(generation)?;
                let effects = self.cell_move(SessionCellEvent::AcceptSettings)?;
                if effects.contains(&SessionCellEffect::SettingAccepted {
                    kind: crate::ProviderSettingKind::Fast,
                }) {
                    self.cell_move(SessionCellEvent::KeepFastOff(false))?;
                }
                self.drive_prompt_line()
            }
        }
    }

    fn apply_wire(&mut self, result: SessionWireResult) -> OwnerResult {
        match result {
            SessionWireResult::Restore {
                run,
                outcome,
                fast_on,
            } => self.restore_on_wire(run, outcome, fast_on),
            SessionWireResult::RestoreFailed { run, failure } => {
                self.restore_failed_on_wire(run, failure)
            }
            SessionWireResult::RestoreMode {
                run,
                setting,
                outcome,
            } => self.restore_mode_on_wire(run, setting, outcome),
            SessionWireResult::Close { run, outcome } => self.close_on_wire(run, outcome),
            SessionWireResult::CloseNotSent(run) => self.close_not_sent(run),
        }
    }

    fn apply_store(&mut self, result: SessionStoreResult) -> OwnerResult {
        match result {
            SessionStoreResult::Terminal { write, outcome } => {
                self.terminal_recorded(write, outcome)
            }
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
        match effect {
            SessionOwnerEffect::ArmDeadline(deadline) => self.arm_business(deadline),
            SessionOwnerEffect::Wire(request) => self.effects.push(OwnerEffect::Wire(request)),
            SessionOwnerEffect::Store(request) => self.effects.push(OwnerEffect::Store(request)),
            SessionOwnerEffect::Publish(fact) => self.publish_fact(fact),
            SessionOwnerEffect::Reply(reply) => self.record_terminal(reply),
            SessionOwnerEffect::RetireRoute(run) => {
                self.effects.push(OwnerEffect::RetireRoute(run))
            }
            SessionOwnerEffect::PrepareHost(run) => {
                self.effects.push(OwnerEffect::PrepareHost(run))
            }
            SessionOwnerEffect::ForwardTool {
                generation,
                tool_id,
            } => {
                let turn = match self.next.turn() {
                    super::SessionTurn::Running { turn }
                    | super::SessionTurn::Cancelling { turn } => Some(turn),
                    super::SessionTurn::Idle => None,
                };
                self.effects.push(OwnerEffect::ForwardTool {
                    run: generation,
                    turn,
                    tool_id,
                });
            }
            SessionOwnerEffect::CheckRetainedTool {
                generation,
                tool_id,
            } => {
                self.effects.push(OwnerEffect::CheckRetainedTool {
                    run: generation,
                    tool_id,
                });
            }
        }
    }
}

fn remote_effect(outcome: &WireOutcome<()>) -> RemoteEffect {
    match outcome {
        WireOutcome::Confirmed(()) => RemoteEffect::Applied,
        WireOutcome::Refused => RemoteEffect::None,
        WireOutcome::Unknown => RemoteEffect::Unknown,
    }
}
