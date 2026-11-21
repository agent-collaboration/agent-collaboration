//! Record a new turn reported by the provider in a correlated steer result.

use super::{OwnerRejection, OwnerResult, OwnerTransition, remote_effect};
use crate::SessionGeneration;
use crate::queued_entry_machine::SentOutcome;
use crate::session_cell_machine::SessionCellEvent;
use crate::session_owner::{
    InputWait, OwnerEffect, OwnerInput, PromptCompletion, RemoteEffect, SessionOwnerEffect,
    SessionOwnerReply, SessionPublication, SessionStoreRequest, TurnMembership, WireOutcome,
};
use crate::session_turn_machine::{SessionTurn, TurnId};
use session_event_model::{TurnLostReason, TurnOutcome};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::session_owner) enum PreviousPromptSettlement {
    AwaitingWire,
    CancelUnconfirmed,
    Reconciling(PromptCompletion),
}

impl OwnerTransition {
    pub(super) fn steer_started_new_turn(
        &mut self,
        input: OwnerInput,
        turn: TurnId,
    ) -> OwnerResult {
        if !self.steer_is_pending(&input, InputWait::Wire) {
            return Err(OwnerRejection::IgnoredLate);
        }
        self.next.steer = None;
        self.record_terminal(SessionOwnerReply::SteerStarted {
            input: input.clone(),
            turn,
        });
        let run = input.generation;
        if !self.is_live(run) {
            return Ok(());
        }
        match self.next.turn() {
            SessionTurn::Running { turn } | SessionTurn::Cancelling { turn }
                if turn == input.turn =>
            {
                self.detach_previous_turn(run, turn)?;
            }
            SessionTurn::Idle
                if self
                    .next
                    .ports
                    .last_live_turn
                    .as_ref()
                    .is_some_and(|last| last.turn == input.turn) => {}
            _ => return Ok(()),
        }
        if self
            .cell_move(SessionCellEvent::PromptStarted { turn })
            .is_err()
        {
            // The receipt concerns a real provider effect, even if a newer
            // admission owns the Router's live projection by the time it arrives.
            return Ok(());
        }
        let input = OwnerInput { turn, ..input };
        self.next.ports.provider_started_turn = Some(input.clone());
        self.effects.push(OwnerEffect::CallbackMembership(
            TurnMembership::ObservedEligible { run, turn },
        ));
        self.release(SessionOwnerEffect::Publish(
            SessionPublication::TurnStarted {
                generation: run,
                turn,
                input: input.input.clone(),
            },
        ));
        self.effects.push(OwnerEffect::ProviderStartedTurn(input));
        Ok(())
    }

    fn detach_previous_turn(&mut self, run: SessionGeneration, turn: TurnId) -> OwnerResult {
        if let Some(previous) = self.next.ports.provider_started_turn.take() {
            return self.finish_turn(
                &previous,
                SentOutcome::Applied,
                Some(TurnOutcome::Ended {
                    stop_reason: session_event_model::StopReason::Unknown(
                        "provider_started_next_turn".to_owned(),
                    ),
                    local_cause: None,
                }),
                true,
            );
        }
        let previous = self
            .next
            .prompts
            .values()
            .find(|pending| pending.input.turn == turn && pending.wait == InputWait::Wire)
            .map(|pending| pending.input.clone())
            .ok_or(OwnerRejection::IgnoredLate)?;
        let settlement = self.detach_cancel_confirmation(&previous);
        self.next
            .ports
            .previous_wire_prompts
            .insert(previous, settlement);
        let effects = self.cell_move(SessionCellEvent::TurnEnded {
            turn,
            outcome: SentOutcome::Applied,
        })?;
        self.apply_entry_effects(effects)?;
        self.retire_turn_tools(run, turn);
        self.effects
            .push(OwnerEffect::CallbackMembership(TurnMembership::End {
                run,
                turn,
            }));
        self.resolve_previous_turn_interactions(run);
        Ok(())
    }

    pub(super) fn finish_previous_prompt(
        &mut self,
        input: &OwnerInput,
        previous: PreviousPromptSettlement,
        outcome: WireOutcome<PromptCompletion>,
    ) -> OwnerResult {
        match previous {
            PreviousPromptSettlement::Reconciling(_) => return Err(OwnerRejection::IgnoredLate),
            PreviousPromptSettlement::CancelUnconfirmed => {
                if let WireOutcome::Confirmed(completion) = outcome {
                    self.next.ports.previous_wire_prompts.insert(
                        input.clone(),
                        PreviousPromptSettlement::Reconciling(completion),
                    );
                    self.release(SessionOwnerEffect::Store(
                        SessionStoreRequest::ReconcileLateCancelledPrompt(input.clone()),
                    ));
                    return Ok(());
                }
                self.next.ports.previous_wire_prompts.remove(input);
                return Ok(());
            }
            PreviousPromptSettlement::AwaitingWire => {}
        }
        self.next.ports.previous_wire_prompts.remove(input);
        let remote = remote_effect(&outcome);
        match outcome {
            WireOutcome::Confirmed(completion) => {
                self.publish_previous_completion(input, &completion);
                self.completing_prompt = Some((input.input.clone(), Ok(completion)));
            }
            WireOutcome::Refused | WireOutcome::Unknown if self.is_live(input.generation) => {
                self.release(SessionOwnerEffect::Publish(
                    SessionPublication::PreviousTurnEnded {
                        generation: input.generation,
                        turn: input.turn,
                        outcome: TurnOutcome::Lost {
                            reason: TurnLostReason::ProviderTurnFailed,
                        },
                    },
                ));
            }
            _ => {}
        }
        self.reply_input(input.clone(), remote);
        if remote == RemoteEffect::Unknown {
            self.retire_waiting_inputs(
                crate::session_owner::RevokeReason::EarlierOutcomeUnknown {
                    operation: input.operation,
                },
            )?;
        }
        Ok(())
    }

    pub(super) fn publish_previous_completion(
        &mut self,
        input: &OwnerInput,
        completion: &PromptCompletion,
    ) {
        if self.is_live(input.generation) {
            self.release(SessionOwnerEffect::Publish(
                SessionPublication::PreviousTurnEnded {
                    generation: input.generation,
                    turn: input.turn,
                    outcome: TurnOutcome::Ended {
                        stop_reason: completion.stop_reason.clone(),
                        local_cause: completion.local_cause.clone(),
                    },
                },
            ));
        }
    }

    pub(super) fn retire_previous_prompts(&mut self, run: SessionGeneration) {
        let previous: Vec<_> = self
            .next
            .ports
            .previous_wire_prompts
            .keys()
            .filter(|input| input.generation == run)
            .cloned()
            .collect();
        for input in previous {
            self.abandon_taken_prompt(&input);
            if self.next.ports.previous_wire_prompts.remove(&input)
                == Some(PreviousPromptSettlement::AwaitingWire)
            {
                self.reply_input(input, RemoteEffect::Unknown);
            }
        }
    }

    pub(super) fn observed_turn_ended(
        &mut self,
        run: SessionGeneration,
        turn: TurnId,
        outcome: TurnOutcome,
    ) -> OwnerResult {
        let input = self
            .next
            .ports
            .provider_started_turn
            .take()
            .filter(|input| input.generation == run && input.turn == turn)
            .ok_or(OwnerRejection::IgnoredLate)?;
        self.finish_turn(
            &input,
            SentOutcome::Applied,
            Some(outcome),
            self.is_live(run),
        )?;
        self.continue_close()?;
        self.drive_prompt_line()
    }
}
