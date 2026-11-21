use super::{
    CloseRequest, OwnerEffect, OwnerEvent, OwnerMailboxEvent, OwnerOutbox, RemoteEffect,
    RunAttempt, SessionGeneration, SessionLifecycle, SessionOwnerCommand, SessionOwnerCommit,
    SessionOwnerDecision, SessionOwnerEffect, SessionOwnerRefusal,
    SessionOwnerRejection as OwnerRejection, SessionOwnerState, SessionStoreResult,
    SessionWireResult, WireOutcome,
};
use crate::session_cell_machine::{SessionCellEffect, SessionCellEvent, SessionCellMachine};
use state_machine_core::{StateMachine, TransitionError};

mod cancel_confirmation;
pub(super) use cancel_confirmation::PendingCancelConfirmation;
mod delivery_evidence_effects;
mod dispatch_effects;
mod input_effects;
mod lifecycle_effects;
mod provider_started_turn;
pub(super) use provider_started_turn::PreviousPromptSettlement;
mod publication_effects;
mod publication_projection;
pub(super) use publication_projection::OwnerProjection;
mod registry_effects;
mod setting_effects;
mod store_effects;
mod tool_effects;

impl SessionOwnerState {
    /// Reduces one mailbox event. Discarded events release no executor work.
    #[cfg(test)]
    pub fn decide(&self, event: OwnerMailboxEvent) -> SessionOwnerDecision {
        self.decide_at(event, 0)
    }

    pub(super) fn decide_at(
        &self,
        event: OwnerMailboxEvent,
        observed_at_ms: i64,
    ) -> SessionOwnerDecision {
        let attaching = event.event == OwnerEvent::ConnectionAttached
            && !self.ports.connection_alive
            && event.owner.incarnation == self.ports.address.incarnation
            && event.owner.connection_epoch != self.ports.address.connection_epoch;
        // Store, publication and timers belong to the incarnation, even
        // when the ACP connection epoch changes while a result is pending.
        let incarnation_local = event.owner.incarnation == self.ports.address.incarnation
            && matches!(
                &event.event,
                OwnerEvent::Store(_) | OwnerEvent::Publication(_) | OwnerEvent::Deadline(_)
            );
        if event.owner != self.ports.address && !attaching && !incarnation_local {
            return Err(OwnerRejection::IgnoredLate);
        }
        let mut transition = OwnerTransition {
            next: self.clone(),
            effects: Vec::new(),
            observed_at_ms,
            completing_prompt: None,
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
        let owner = transition.next.ports.address;
        Ok(SessionOwnerCommit {
            next: transition.next,
            effects: OwnerOutbox {
                owner,
                effects: transition.effects,
            },
        })
    }
}

struct OwnerTransition {
    next: SessionOwnerState,
    effects: Vec<OwnerEffect>,
    observed_at_ms: i64,
    completing_prompt: Option<(
        super::InputId,
        Result<super::PromptCompletion, super::ProviderWireFailure>,
    )>,
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
            OwnerEvent::DeliveryEvidence { placement, outcome } => {
                self.delivery_evidence_recorded(placement, outcome)
            }
            OwnerEvent::Wire(result) => self.apply_wire(result),
            OwnerEvent::Store(result) => self.apply_store(result),
            OwnerEvent::Publication(result) => self.publication_result(result),
            OwnerEvent::Deadline(deadline) => self.port_deadline(deadline),
            OwnerEvent::RoutedTool(event) => self.routed_tool(event),
            OwnerEvent::Dispatch(result) => self.dispatch_result(result),
            OwnerEvent::Create(progress) => self.create_progress(progress),
            OwnerEvent::ConnectionLost => self.connection_lost(),
            OwnerEvent::RunLost(run) => self.run_lost(run),
            OwnerEvent::ConnectionAttached => Err(OwnerRejection::IgnoredLate),
            OwnerEvent::Prepare(progress) => self.restore_preparation(progress),
            OwnerEvent::Stream {
                run,
                expected_turn,
                event,
            } => self.observe_stream(run, expected_turn, event),
            OwnerEvent::CatalogueObserved { run, update } => {
                self.observe_live_catalogue(run, update)
            }
            OwnerEvent::SubmittedInput { input, item } => {
                self.observe_submitted_input(&input, item)
            }
            OwnerEvent::Replay { run, events } => self.observe_replay(run, events),
            OwnerEvent::SkillView { run, view } => self.observe_skill_view(run, view),
            OwnerEvent::ObservedTurnEnded { run, turn, outcome } => {
                self.observed_turn_ended(run, turn, outcome)
            }
            OwnerEvent::OutputLimit { run, turn } => self.cancel_turn(run, turn),
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
            SessionOwnerCommand::PromptWithSkills {
                input,
                admission,
                selected,
            } => self.admit_selected_prompt(input, admission, selected),
            SessionOwnerCommand::Steer { input, supported } => self.admit_steer(input, supported),
            SessionOwnerCommand::SteerExisting { input, supported } => {
                self.admit_existing_steer(input, supported)
            }
            SessionOwnerCommand::PlaceDelivery { input, delivery } => {
                self.place_delivery(input, delivery)
            }
            SessionOwnerCommand::QueueColdDelivery {
                input,
                attempt,
                selected,
                record_evidence,
            } => self.queue_cold_delivery(input, attempt, selected, record_evidence),
            SessionOwnerCommand::CancelTurn { generation, turn } => {
                self.cancel_turn(generation, turn)
            }
            SessionOwnerCommand::CancelQueued { generation, input } => {
                self.cancel_queued(generation, &input)
            }
            SessionOwnerCommand::CancelOperation { target } => self.cancel_operation(target),
            SessionOwnerCommand::Set {
                input,
                setting,
                value,
                expected_model,
            } => self.admit_setting(super::QueuedSetting {
                input,
                setting,
                value,
                expected_model,
                context: None,
            }),
            SessionOwnerCommand::SetInContext {
                input,
                setting,
                value,
                expected_model,
                reuse_confirmed,
            } => self.admit_setting(super::QueuedSetting {
                input,
                setting,
                value,
                expected_model,
                context: Some(super::TurnSettingContext { reuse_confirmed }),
            }),
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
            SessionWireResult::Restore { run, outcome } => self.restore_on_wire(run, outcome),
            SessionWireResult::RestoreFailed { run, failure } => {
                self.restore_failed_on_wire(run, failure)
            }
            SessionWireResult::RestoreMode {
                run,
                setting,
                outcome,
                catalogue,
            } => self.restore_mode_on_wire(run, setting, outcome, catalogue),
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
            SessionStoreResult::DispatchBlockedByUnknown { input, operation } => {
                self.dispatch_blocked_by_unknown(&input, operation)
            }
            SessionStoreResult::LateCancelledPromptReconciled { input, outcome } => {
                self.late_cancelled_prompt_reconciled(&input, outcome)
            }
            SessionStoreResult::ConfirmedSetting { setting, outcome } => {
                self.setting_recorded(setting, outcome)
            }
        }
    }

    fn cell_move(&mut self, event: SessionCellEvent) -> OwnerResult<Vec<SessionCellEffect>> {
        let (next, effects) = SessionCellMachine::transition(&self.next.cell, event).map_err(
            |error| match error {
                TransitionError::Stale => OwnerRejection::IgnoredLate,
                TransitionError::Domain(crate::session_cell_machine::SessionCellRefusal::Turn(
                    crate::session_turn_machine::SessionTurnRefusal::CannotSteer,
                )) => SessionOwnerRefusal::CannotSteer.into(),
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
            SessionOwnerEffect::Reply(
                reply @ (super::SessionOwnerReply::SettingFinished { .. }
                | super::SessionOwnerReply::SettingNotSent { .. }
                | super::SessionOwnerReply::ModelFastFinished { .. }),
            ) => self.effects.push(OwnerEffect::Reply(reply)),
            SessionOwnerEffect::Reply(reply) => self.record_terminal(reply),
            SessionOwnerEffect::RetireRoute(run) => {
                self.effects.push(OwnerEffect::RetireRoute(run))
            }
            SessionOwnerEffect::PrepareHost { run, catalogue } => self
                .effects
                .push(OwnerEffect::PrepareHost { run, catalogue }),
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
        }
    }
}

fn remote_effect<T>(outcome: &WireOutcome<T>) -> RemoteEffect {
    match outcome {
        WireOutcome::Confirmed(_) => RemoteEffect::Applied,
        WireOutcome::Refused => RemoteEffect::None,
        WireOutcome::Unknown => RemoteEffect::Unknown,
    }
}
