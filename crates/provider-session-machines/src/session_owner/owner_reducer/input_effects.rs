use super::{OwnerRejection, OwnerResult, OwnerTransition, remote_effect};
use crate::queued_entry_machine::{QueuedEntryEffect, SentOutcome};
use crate::session_cell_machine::{QueuedOperation, SessionCellEffect, SessionCellEvent};
use crate::session_lifecycle_machine::SessionLifecycle;
use crate::session_owner::{
    AuthorizedWork, DeliveryPlacementKind, OwnerEffect, RevokeReason, TurnMembership,
};
use crate::session_owner::{
    EffectOutcome, InputWait, LastLiveTurn, OwnerInput, PendingPrompt, PromptAdmission,
    PromptCompletion, RemoteEffect, SessionDeadline, SessionOwnerEffect, SessionOwnerRefusal,
    SessionOwnerReply, SessionPublication, SessionStoreRequest, WireOutcome,
};
use crate::session_turn_machine::{SessionTurn, TurnId};
use crate::{SessionGeneration, session_turn_machine::SessionTurnEffect};
use session_event_model::{InputId, TurnLostReason, TurnOutcome};

const MAX_WAITING_PROMPTS: usize = 128;

impl OwnerTransition {
    pub(super) fn admit_selected_prompt(
        &mut self,
        input: OwnerInput,
        admission: PromptAdmission,
        selected: Vec<session_event_model::SelectedSkill>,
    ) -> OwnerResult {
        if !selected.is_empty()
            && self.is_live(input.generation)
            && (!selected
                .iter()
                .all(|skill| self.next.ports.projection.skills.contains_selection(skill))
                || matches!(
                    admission,
                    PromptAdmission::Auto { .. }
                        | PromptAdmission::DeliveryAuto {
                            steering_supported: true
                        }
                ) && self.next.turn() != SessionTurn::Idle)
        {
            return Err(SessionOwnerRefusal::SkillSelectionChanged.into());
        }
        self.next
            .ports
            .selected_skills
            .insert(input.input.clone(), selected);
        self.admit_prompt(input, admission)
    }

    pub(super) fn admit_prompt(
        &mut self,
        input: OwnerInput,
        admission: PromptAdmission,
    ) -> OwnerResult {
        self.admit_prompt_on_run(input, admission, false)
    }

    fn admit_prompt_on_run(
        &mut self,
        input: OwnerInput,
        admission: PromptAdmission,
        replacement: bool,
    ) -> OwnerResult {
        if self.next.cell.run_generation() != Some(input.generation) {
            return Err(SessionOwnerRefusal::GenerationChanged.into());
        }
        if self.next.cell.settings_held() {
            return Err(SessionOwnerRefusal::SettingsUnresolved.into());
        }
        let earlier_work_waits = self.next.ports.delivery_evidence.contains_key(&input.input)
            && matches!(admission, PromptAdmission::DeliveryAuto { .. })
            && (!self.next.prompts.is_empty() || self.next.setting.is_some())
            && self.next.turn() == SessionTurn::Idle;
        let queued = admission == PromptAdmission::Queue
            || earlier_work_waits
            || matches!(self.next.lifecycle(), SessionLifecycle::Loading { .. })
            || matches!(
                (admission, self.next.turn()),
                (
                    PromptAdmission::DeliveryAuto {
                        steering_supported: false
                    },
                    SessionTurn::Running { .. }
                )
            );
        if let SessionTurn::Running { turn } = self.next.turn() {
            match admission {
                PromptAdmission::Auto { steering_supported } => {
                    return self.admit_steer(OwnerInput { turn, ..input }, steering_supported);
                }
                PromptAdmission::DeliveryAuto {
                    steering_supported: true,
                } => {
                    return self.admit_steer(OwnerInput { turn, ..input }, true);
                }
                _ => {}
            }
        }
        if admission == PromptAdmission::Direct
            && self.is_live(input.generation)
            && self.prompt_in_progress()
        {
            return Err(SessionOwnerRefusal::EffectInProgress.into());
        }
        if queued
            && self
                .next
                .prompts
                .values()
                .filter(|pending| pending.wait != InputWait::Wire)
                .count()
                >= MAX_WAITING_PROMPTS
        {
            return Err(SessionOwnerRefusal::QueueFull.into());
        }
        if !replacement {
            self.require_new_input(&input.input)?;
        }
        if let Some(restore) = self.next.restore {
            self.depend_on_restore(input.operation, restore.run())?;
        }
        let operation = QueuedOperation(input.input.as_str().to_owned());
        let effects = self.cell_move(SessionCellEvent::Enqueue { operation })?;
        let Some(sequence) = effects.into_iter().find_map(|effect| match effect {
            SessionCellEffect::Queued { sequence, .. } => Some(sequence),
            _ => None,
        }) else {
            return Err(OwnerRejection::IgnoredLate);
        };
        self.next.ports.admitted_inputs.insert(input.input.clone());
        if queued && !self.await_delivery_evidence(&input, DeliveryPlacementKind::Queue) {
            self.effects.push(OwnerEffect::InputQueued(input.clone()));
        }
        self.next.prompts.insert(
            sequence,
            PendingPrompt {
                input,
                wait: InputWait::Admission,
                choose_steer_if_a_turn_starts: matches!(
                    admission,
                    PromptAdmission::Auto {
                        steering_supported: true
                    }
                ) && !queued,
            },
        );
        self.drive_prompt_line()
    }

    fn require_new_input(&self, input: &InputId) -> OwnerResult {
        let known = self
            .next
            .prompts
            .values()
            .chain(self.next.steer.iter())
            .any(|pending| &pending.input.input == input);
        if known || self.next.ports.admitted_inputs.contains(input) {
            Err(SessionOwnerRefusal::InputAlreadyAdmitted.into())
        } else {
            Ok(())
        }
    }

    pub(super) fn drive_prompt_line(&mut self) -> OwnerResult {
        if !self.next.ports.connection_alive
            || self.prompt_in_progress()
            || self.next.cell.settings_held()
            || self.next.setting.is_some()
        {
            return Ok(());
        }
        while let Some((&sequence, pending)) = self.next.prompts.first_key_value() {
            if !self.is_live(pending.input.generation) {
                return Ok(());
            }
            let input = pending.input.clone();
            if !self.selected_skills_current(&input.input) {
                let operation = QueuedOperation(input.input.as_str().to_owned());
                self.next
                    .ports
                    .unsent_prompts
                    .insert(input, RevokeReason::SkillSelectionChanged);
                let effects = self.cell_move(SessionCellEvent::CancelEntry { operation })?;
                self.apply_entry_effects(effects)?;
            } else {
                if self.await_delivery_evidence(&input, DeliveryPlacementKind::Start) {
                    return Ok(());
                }
                self.cell_move(SessionCellEvent::Claim { sequence })?;
                self.cell_move(SessionCellEvent::Commit { sequence })?;
                let effects = self.cell_move(SessionCellEvent::StartSendClaimed { sequence })?;
                return self.apply_entry_effects(effects);
            }
        }
        Ok(())
    }

    pub(super) fn steer_contending_auto_after_take(
        &mut self,
        foreground: &OwnerInput,
    ) -> OwnerResult {
        if self.next.steer.is_some() {
            return Ok(());
        }
        let Some((&sequence, waiting)) = self
            .next
            .prompts
            .iter()
            .find(|(_, pending)| pending.wait == InputWait::Admission)
        else {
            return Ok(());
        };
        if !waiting.choose_steer_if_a_turn_starts
            || waiting.input.generation != foreground.generation
            || self
                .next
                .ports
                .selected_skills
                .get(&waiting.input.input)
                .is_some_and(|items| !items.is_empty())
        {
            return Ok(());
        }
        // Auto was waiting for a placement. Its line entry has never reached
        // a mark, so withdraw it locally and keep the same operation and input.
        let input = OwnerInput {
            turn: foreground.turn,
            ..waiting.input.clone()
        };
        let operation = QueuedOperation(input.input.as_str().to_owned());
        let effects = self.cell_move(SessionCellEvent::CancelEntry { operation })?;
        if !matches!(effects.as_slice(), [SessionCellEffect::Entry { sequence: removed, effect: QueuedEntryEffect::SettleNotSent(_), .. }] if *removed == sequence)
        {
            return Err(OwnerRejection::IgnoredLate);
        }
        self.next.prompts.remove(&sequence);
        self.next.ports.admitted_inputs.remove(&input.input);
        self.admit_existing_steer(input, true)
    }

    fn selected_skills_current(&self, input: &InputId) -> bool {
        self.next
            .ports
            .selected_skills
            .get(input)
            .is_none_or(|selected| {
                selected
                    .iter()
                    .all(|skill| self.next.ports.projection.skills.contains_selection(skill))
            })
    }

    pub(super) fn admit_warm(&mut self, input: OwnerInput) -> OwnerResult {
        self.require_live(input.generation)?;
        if self.prompt_in_progress() || !self.next.prompts.is_empty() || self.next.setting.is_some()
        {
            return Err(SessionOwnerRefusal::EffectInProgress.into());
        }
        self.admit_prompt(input, PromptAdmission::Direct)
    }

    pub(super) fn prompt_in_progress(&self) -> bool {
        self.next.turn() != SessionTurn::Idle
            || self
                .next
                .prompts
                .values()
                .any(|pending| pending.wait != InputWait::Admission)
    }

    pub(super) fn apply_entry_effects(&mut self, effects: Vec<SessionCellEffect>) -> OwnerResult {
        for effect in effects {
            if let SessionCellEffect::Entry {
                sequence, effect, ..
            } = effect
            {
                self.release_entry_effect(sequence, effect)?;
            }
        }
        Ok(())
    }

    fn release_entry_effect(&mut self, sequence: u64, effect: QueuedEntryEffect) -> OwnerResult {
        let Some(pending) = self.next.prompts.get(&sequence) else {
            return Err(OwnerRejection::IgnoredLate);
        };
        let input = pending.input.clone();
        match effect {
            QueuedEntryEffect::StartSend => Ok(()),
            QueuedEntryEffect::WriteMark => {
                self.release(SessionOwnerEffect::Store(
                    SessionStoreRequest::MarkDispatch(input),
                ));
                self.arm_prompt_deadline(sequence, InputWait::DispatchStore)
            }
            QueuedEntryEffect::SendRequest => {
                self.require_live(input.generation)?;
                self.next
                    .prompts
                    .get_mut(&sequence)
                    .ok_or(OwnerRejection::IgnoredLate)?
                    .wait = InputWait::Enqueue;
                self.issue_authorization(AuthorizedWork::Prompt(input))
            }
            QueuedEntryEffect::Unmark => {
                self.release(SessionOwnerEffect::Store(
                    SessionStoreRequest::UndoDispatch(input),
                ));
                self.arm_prompt_deadline(sequence, InputWait::UndoStore)
            }
            QueuedEntryEffect::SettleNotSent(_)
            | QueuedEntryEffect::OverlayNotSentOverDispatch(_) => {
                self.finish_entry(sequence, input, RemoteEffect::None);
                Ok(())
            }
            QueuedEntryEffect::ReadBackRow(_) => {
                // There was no authorization. Offer known-unsent terminal
                // settlement to the same lane, behind any accepted mark.
                self.finish_entry(sequence, input, RemoteEffect::None);
                Ok(())
            }
            QueuedEntryEffect::SettleSent(outcome) => {
                let remote = match outcome {
                    SentOutcome::Applied => RemoteEffect::Applied,
                    SentOutcome::Refused => RemoteEffect::None,
                    SentOutcome::Unknown => RemoteEffect::Unknown,
                };
                self.finish_entry(sequence, input, remote);
                Ok(())
            }
        }
    }

    fn arm_prompt_deadline(&mut self, sequence: u64, wait: InputWait) -> OwnerResult {
        let pending = self
            .next
            .prompts
            .get_mut(&sequence)
            .ok_or(OwnerRejection::IgnoredLate)?;
        pending.wait = wait;
        let input = pending.input.clone();
        self.release(SessionOwnerEffect::ArmDeadline(SessionDeadline::Prompt {
            input,
            wait,
        }));
        Ok(())
    }

    fn prompt_sequence(&self, input: &OwnerInput, wait: InputWait) -> OwnerResult<u64> {
        self.next
            .prompts
            .iter()
            .find_map(|(sequence, pending)| {
                (&pending.input == input && pending.wait == wait).then_some(*sequence)
            })
            .ok_or(OwnerRejection::IgnoredLate)
    }

    pub(super) fn dispatch_marked(
        &mut self,
        input: &OwnerInput,
        outcome: EffectOutcome,
    ) -> OwnerResult {
        let sequence = self.prompt_sequence(input, InputWait::DispatchStore)?;
        if self.is_live(input.generation) && !self.selected_skills_current(&input.input) {
            let operation = QueuedOperation(input.input.as_str().to_owned());
            let withdrawn = self.cell_move(SessionCellEvent::CancelEntry { operation })?;
            self.apply_entry_effects(withdrawn)?;
            self.next
                .ports
                .unsent_prompts
                .insert(input.clone(), RevokeReason::SkillSelectionChanged);
        }
        let event = match outcome {
            EffectOutcome::Applied => SessionCellEvent::MarkCommitted {
                sequence,
                // This is the local owner's permission to hand off. Transport
                // failures come back as a stamped wire result.
                transport_open: self.is_live(input.generation),
                turn: input.turn,
            },
            EffectOutcome::Failed => SessionCellEvent::MarkFailed { sequence },
            EffectOutcome::Unknown => SessionCellEvent::MarkUncertain { sequence },
        };
        let effects = self.cell_move(event)?;
        self.apply_entry_effects(effects)?;
        self.drive_prompt_line()
    }

    pub(super) fn dispatch_unmarked(
        &mut self,
        input: &OwnerInput,
        outcome: EffectOutcome,
    ) -> OwnerResult {
        let pending_prompt = self.prompt_sequence(input, InputWait::UndoStore);
        if pending_prompt.is_err()
            && let Some(reason) = self.next.ports.unsent_prompts.remove(input)
        {
            self.next.ports.selected_skills.remove(&input.input);
            self.next.ports.delivery_evidence.remove(&input.input);
            if self.steer_is_pending(input, InputWait::UndoStore) {
                self.next.steer = None;
            }
            self.release(SessionOwnerEffect::Reply(SessionOwnerReply::InputUnsent {
                input: input.clone(),
                reason,
            }));
            return self.drive_prompt_line();
        }
        if self.steer_is_pending(input, InputWait::UndoStore) {
            self.next.steer = None;
            if let Some(replacement) = self.next.ports.steer_replacement.take()
                && outcome == EffectOutcome::Applied
                && self.is_live(replacement.generation)
            {
                return self.admit_prompt_on_run(replacement, PromptAdmission::Queue, true);
            }
            self.reply_input(input.clone(), RemoteEffect::None);
            return Ok(());
        }
        let sequence = pending_prompt?;
        let event = if outcome == EffectOutcome::Applied {
            SessionCellEvent::UnmarkCommitted { sequence }
        } else {
            SessionCellEvent::UnmarkFailed { sequence }
        };
        let effects = self.cell_move(event)?;
        self.apply_entry_effects(effects)?;
        self.drive_prompt_line()
    }

    pub(super) fn dispatch_blocked_by_unknown(
        &mut self,
        input: &OwnerInput,
        operation: uuid::Uuid,
    ) -> OwnerResult {
        let reason = RevokeReason::EarlierOutcomeUnknown { operation };
        self.next.ports.unsent_prompts.insert(input.clone(), reason);
        if self.steer_is_pending(input, InputWait::DispatchStore) {
            return self.steer_marked(input, EffectOutcome::Failed);
        }
        let sequence = self.prompt_sequence(input, InputWait::DispatchStore)?;
        let effects = self.cell_move(SessionCellEvent::MarkFailed { sequence })?;
        self.apply_entry_effects(effects)?;
        self.retire_waiting_inputs(reason)
    }

    pub(super) fn cancel_operation(&mut self, operation: uuid::Uuid) -> OwnerResult {
        if let Some(pending) = self
            .next
            .prompts
            .values()
            .find(|pending| pending.input.operation == operation)
        {
            let input = pending.input.clone();
            return if pending.wait == InputWait::Wire {
                self.cancel_turn(input.generation, input.turn)
            } else {
                self.cancel_queued(input.generation, &input.input)
            };
        }
        if let Some(input) = self
            .next
            .ports
            .provider_started_turn
            .as_ref()
            .filter(|input| input.operation == operation)
        {
            return self.cancel_turn(input.generation, input.turn);
        }
        Err(SessionOwnerRefusal::CancelTargetInactive.into())
    }

    pub(super) fn cancel_queued(
        &mut self,
        generation: SessionGeneration,
        input: &InputId,
    ) -> OwnerResult {
        if self.next.cell.run_generation() != Some(generation)
            || !self.next.prompts.values().any(|pending| {
                pending.input.generation == generation && &pending.input.input == input
            })
        {
            return Err(SessionOwnerRefusal::GenerationChanged.into());
        }
        if self.revoke_input(generation, input, RevokeReason::Cancelled) {
            return Ok(());
        }
        if let Some(pending) =
            self.next.prompts.values().find(|pending| {
                pending.input.generation == generation && &pending.input.input == input
            })
        {
            self.next
                .ports
                .unsent_prompts
                .insert(pending.input.clone(), RevokeReason::QueueCancelled);
        }
        let operation = QueuedOperation(input.as_str().to_owned());
        let effects = self.cell_move(SessionCellEvent::CancelEntry { operation })?;
        self.apply_entry_effects(effects)?;
        self.drive_prompt_line()
    }

    pub(super) fn cancel_turn(
        &mut self,
        generation: SessionGeneration,
        turn: TurnId,
    ) -> OwnerResult {
        self.require_live(generation)?;
        if self.next.turn() != (SessionTurn::Running { turn }) {
            return Err(SessionOwnerRefusal::GenerationChanged.into());
        }
        let provisional = self
            .next
            .prompts
            .values()
            .find(|pending| pending.input.turn == turn && pending.wait == InputWait::Enqueue)
            .map(|pending| pending.input.input.clone());
        if let Some(input_id) = provisional
            && self.revoke_input(generation, &input_id, RevokeReason::Cancelled)
        {
            return Ok(());
        }
        self.issue_cancel(generation, turn)
    }

    pub(super) fn finish_prompt(
        &mut self,
        input: &OwnerInput,
        outcome: WireOutcome<PromptCompletion>,
    ) -> OwnerResult {
        if let Some(previous) = self.next.ports.previous_wire_prompts.get(input).cloned() {
            return self.finish_previous_prompt(input, previous, outcome);
        }
        self.prompt_sequence(input, InputWait::Wire)?;
        if self.cancel_unconfirmed_for(input) {
            return self.reconcile_cancelled_prompt(input, outcome);
        }
        self.finish_observed_prompt(input, outcome)
    }

    pub(super) fn finish_observed_prompt(
        &mut self,
        input: &OwnerInput,
        outcome: WireOutcome<PromptCompletion>,
    ) -> OwnerResult {
        let (outcome, observed) = match outcome {
            WireOutcome::Confirmed(completion) => {
                let observed = TurnOutcome::Ended {
                    stop_reason: completion.stop_reason.clone(),
                    local_cause: completion.local_cause.clone(),
                };
                self.completing_prompt = Some((input.input.clone(), Ok(completion)));
                (SentOutcome::Applied, observed)
            }
            WireOutcome::Refused => (
                SentOutcome::Refused,
                TurnOutcome::Lost {
                    reason: TurnLostReason::ProviderTurnFailed,
                },
            ),
            WireOutcome::Unknown => (
                SentOutcome::Unknown,
                TurnOutcome::Lost {
                    reason: if matches!(self.completing_prompt, Some((_, Err(_)))) {
                        TurnLostReason::ProviderTurnFailed
                    } else {
                        TurnLostReason::EndNotObservable
                    },
                },
            ),
        };
        self.finish_turn(
            input,
            outcome,
            Some(observed),
            self.is_live(input.generation),
        )?;
        self.continue_close()?;
        if outcome == SentOutcome::Unknown {
            self.retire_waiting_inputs(RevokeReason::EarlierOutcomeUnknown {
                operation: input.operation,
            })
        } else {
            self.drive_prompt_line()
        }
    }

    pub(super) fn retire_waiting_inputs(&mut self, reason: RevokeReason) -> OwnerResult {
        let queued: Vec<_> = self
            .next
            .prompts
            .values()
            .filter(|pending| pending.wait == InputWait::Admission)
            .map(|pending| pending.input.clone())
            .collect();
        for input in queued {
            let operation = QueuedOperation(input.input.as_str().to_owned());
            self.next.ports.unsent_prompts.insert(input, reason);
            let effects = self.cell_move(SessionCellEvent::CancelEntry { operation })?;
            self.apply_entry_effects(effects)?;
        }
        Ok(())
    }

    pub(super) fn tag_waiting_inputs(&mut self, run: SessionGeneration, reason: RevokeReason) {
        for pending in self.next.prompts.values() {
            if pending.input.generation == run && pending.wait == InputWait::Admission {
                self.next
                    .ports
                    .unsent_prompts
                    .insert(pending.input.clone(), reason);
            }
        }
    }

    pub(super) fn end_turn_locally(&mut self, turn: TurnId) -> OwnerResult {
        self.end_unobservable_turn(turn, TurnLostReason::ProviderRetired)
    }

    pub(super) fn end_unobservable_turn(
        &mut self,
        turn: TurnId,
        reason: TurnLostReason,
    ) -> OwnerResult {
        if let Some(input) = self
            .next
            .ports
            .provider_started_turn
            .clone()
            .filter(|input| input.turn == turn)
        {
            self.next.ports.provider_started_turn = None;
            return self.finish_turn(
                &input,
                SentOutcome::Unknown,
                Some(TurnOutcome::Lost { reason }),
                false,
            );
        }
        let input = self
            .next
            .prompts
            .values()
            .find(|pending| {
                pending.input.turn == turn
                    && matches!(pending.wait, InputWait::Wire | InputWait::Enqueue)
            })
            .map(|pending| pending.input.clone())
            .ok_or(OwnerRejection::IgnoredLate)?;
        self.abandon_taken_prompt(&input);
        let observed = self
            .prompt_sequence(&input, InputWait::Wire)
            .is_ok()
            .then_some(TurnOutcome::Lost { reason });
        self.finish_turn(&input, SentOutcome::Unknown, observed, false)
    }

    pub(super) fn finish_turn(
        &mut self,
        input: &OwnerInput,
        outcome: SentOutcome,
        observed: Option<TurnOutcome>,
        idle: bool,
    ) -> OwnerResult {
        let effects = self.cell_move(SessionCellEvent::TurnEnded {
            turn: input.turn,
            outcome,
        })?;
        self.retire_turn_tools(input.generation, input.turn);
        if let Some(observed) = observed {
            self.next.ports.last_live_turn = Some(LastLiveTurn {
                turn: input.turn,
                input: input.input.clone(),
                outcome: observed.clone(),
                observed_at_ms: self.observed_at_ms,
            });
            self.effects
                .push(OwnerEffect::CallbackMembership(TurnMembership::End {
                    run: input.generation,
                    turn: input.turn,
                }));
            self.release(SessionOwnerEffect::Publish(SessionPublication::TurnEnded {
                generation: input.generation,
                turn: input.turn,
                outcome: observed,
                idle,
            }));
        }
        self.apply_entry_effects(effects)?;
        self.clear_cancel_confirmation(input);
        Ok(())
    }

    pub(super) fn prompt_deadline(&mut self, input: &OwnerInput, wait: InputWait) -> OwnerResult {
        self.prompt_sequence(input, wait)?;
        match wait {
            InputWait::Admission => self.cancel_queued(input.generation, &input.input),
            InputWait::DispatchStore => self.dispatch_marked(input, EffectOutcome::Unknown),
            InputWait::UndoStore => self.dispatch_unmarked(input, EffectOutcome::Unknown),
            InputWait::Enqueue | InputWait::Wire => Err(OwnerRejection::IgnoredLate),
        }
    }

    pub(super) fn finish_retired_store_work(
        &mut self,
        generation: SessionGeneration,
    ) -> OwnerResult {
        let retiring: Vec<_> = self
            .next
            .prompts
            .values()
            .filter(|pending| pending.input.generation == generation)
            .map(|pending| (pending.input.clone(), pending.wait))
            .collect();
        for (input, wait) in retiring {
            if !matches!(wait, InputWait::Enqueue | InputWait::Wire) {
                self.prompt_deadline(&input, wait)?;
            }
        }
        if let Some(pending) = self.next.steer.clone().filter(|pending| {
            pending.input.generation == generation && pending.wait == InputWait::Admission
        }) {
            self.reject_delivery_placement(
                pending.input,
                DeliveryPlacementKind::Steer,
                RevokeReason::RunReplaced,
            )?;
        }
        Ok(())
    }

    pub(super) fn unmark_prompt(
        &mut self,
        input: &OwnerInput,
        reason: RevokeReason,
    ) -> OwnerResult {
        let sequence = self
            .next
            .prompts
            .iter()
            .find_map(|(sequence, pending)| (&pending.input == input).then_some(*sequence))
            .ok_or(OwnerRejection::IgnoredLate)?;
        self.next.prompts.remove(&sequence);
        self.next.ports.unsent_prompts.insert(input.clone(), reason);
        self.release(SessionOwnerEffect::Store(
            SessionStoreRequest::UndoDispatch(input.clone()),
        ));
        self.arm(
            super::super::OwnerDeadline::UndoStore {
                input: input.clone(),
            },
            super::publication_effects::LOCAL_BUDGET,
        );
        Ok(())
    }

    pub(super) fn unmark_steer(&mut self, input: &OwnerInput) {
        self.release(SessionOwnerEffect::Store(
            SessionStoreRequest::UndoDispatch(input.clone()),
        ));
        self.wait_for_steer(input.clone(), InputWait::UndoStore);
    }

    pub(super) fn unmark_steer_unsent(&mut self, input: &OwnerInput, reason: RevokeReason) {
        self.next.ports.unsent_prompts.insert(input.clone(), reason);
        self.unmark_steer(input);
    }

    pub(super) fn waiting_for_prompt_enqueue(&self, turn: TurnId) -> bool {
        self.next
            .prompts
            .values()
            .any(|pending| pending.wait == InputWait::Enqueue && pending.input.turn == turn)
    }

    pub(super) fn admit_steer(&mut self, input: OwnerInput, supported: bool) -> OwnerResult {
        if self.next.turn() == SessionTurn::Idle {
            self.require_live(input.generation)?;
            self.effects
                .push(OwnerEffect::IdleSteerUsesPrompt(input.clone()));
            return self.admit_prompt(input, PromptAdmission::Direct);
        }
        self.admit_existing_steer(input, supported)
    }

    pub(super) fn admit_existing_steer(
        &mut self,
        input: OwnerInput,
        supported: bool,
    ) -> OwnerResult {
        self.require_live(input.generation)?;
        if self.next.turn() != (SessionTurn::Running { turn: input.turn }) {
            return Err(SessionOwnerRefusal::GenerationChanged.into());
        }
        self.require_new_input(&input.input)?;
        if self.next.cell.settings_held() {
            return Err(SessionOwnerRefusal::SettingsUnresolved.into());
        }
        if self.next.steer.is_some() {
            return Err(SessionOwnerRefusal::EffectInProgress.into());
        }
        let effects = self.cell_move(SessionCellEvent::SteerRequested {
            agent_steers: supported,
        })?;
        if !effects.contains(&SessionCellEffect::Turn(SessionTurnEffect::InjectSteer {
            turn: input.turn,
        })) {
            return Err(SessionOwnerRefusal::GenerationChanged.into());
        }
        self.next.ports.admitted_inputs.insert(input.input.clone());
        if self.await_delivery_evidence(&input, DeliveryPlacementKind::Steer) {
            self.next.steer = Some(PendingPrompt {
                input,
                wait: InputWait::Admission,
                choose_steer_if_a_turn_starts: false,
            });
        } else {
            self.start_steer_mark(input);
        }
        Ok(())
    }

    pub(super) fn start_steer_mark(&mut self, input: OwnerInput) {
        self.release(SessionOwnerEffect::Store(
            SessionStoreRequest::MarkSteerDispatch(input.clone()),
        ));
        self.wait_for_steer(input, InputWait::DispatchStore);
    }

    pub(super) fn steer_marked(
        &mut self,
        input: &OwnerInput,
        outcome: EffectOutcome,
    ) -> OwnerResult {
        if !self.steer_is_pending(input, InputWait::DispatchStore) {
            return Err(OwnerRejection::IgnoredLate);
        }
        if outcome != EffectOutcome::Applied {
            // The terminal offer follows any accepted mark in the lane.
            self.next.steer = None;
            self.reply_input(input.clone(), RemoteEffect::None);
        } else if self.is_live(input.generation)
            && self.next.turn() == (SessionTurn::Running { turn: input.turn })
        {
            self.next.steer = Some(PendingPrompt {
                input: input.clone(),
                wait: InputWait::Enqueue,
                choose_steer_if_a_turn_starts: false,
            });
            self.issue_authorization(AuthorizedWork::Steer(input.clone()))?;
        } else {
            self.release(SessionOwnerEffect::Store(
                SessionStoreRequest::UndoDispatch(input.clone()),
            ));
            self.wait_for_steer(input.clone(), InputWait::UndoStore);
        }
        Ok(())
    }

    pub(super) fn finish_steer(
        &mut self,
        input: &OwnerInput,
        outcome: WireOutcome<()>,
    ) -> OwnerResult {
        if !self.steer_is_pending(input, InputWait::Wire) {
            return Err(OwnerRejection::IgnoredLate);
        }
        if outcome == WireOutcome::Confirmed(())
            && self.is_live(input.generation)
            && matches!(self.next.turn(), SessionTurn::Running { turn } | SessionTurn::Cancelling { turn } if turn == input.turn)
        {
            self.next.ports.recent_steer = Some(input.clone());
            self.offer_commit(vec![super::super::PublicationFact::Stream {
                run: input.generation,
                event: session_event_model::SessionEvent::InputAccepted {
                    input_id: input.input.clone(),
                    turn_id: input.turn.0.to_string(),
                },
            }]);
            self.effects
                .push(OwnerEffect::SteerSubmitted(input.clone()));
        }
        self.next.steer = None;
        if outcome == WireOutcome::Refused {
            self.next
                .ports
                .unsent_prompts
                .insert(input.clone(), RevokeReason::ProviderSteerRefused);
        }
        self.reply_input(input.clone(), remote_effect(&outcome));
        Ok(())
    }

    pub(super) fn steer_is_pending(&self, input: &OwnerInput, wait: InputWait) -> bool {
        self.next
            .steer
            .as_ref()
            .is_some_and(|pending| &pending.input == input && pending.wait == wait)
    }

    fn wait_for_steer(&mut self, input: OwnerInput, wait: InputWait) {
        self.next.steer = Some(PendingPrompt {
            input: input.clone(),
            wait,
            choose_steer_if_a_turn_starts: false,
        });
        self.release(SessionOwnerEffect::ArmDeadline(SessionDeadline::Steer {
            input,
            wait,
        }));
    }

    pub(super) fn steer_deadline(&mut self, input: &OwnerInput, wait: InputWait) -> OwnerResult {
        if !self.steer_is_pending(input, wait) {
            return Err(OwnerRejection::IgnoredLate);
        }
        match wait {
            InputWait::Admission | InputWait::Enqueue => Err(OwnerRejection::IgnoredLate),
            InputWait::DispatchStore => self.steer_marked(input, EffectOutcome::Unknown),
            InputWait::UndoStore => self.dispatch_unmarked(input, EffectOutcome::Unknown),
            InputWait::Wire => self.finish_steer(input, WireOutcome::Unknown),
        }
    }

    fn finish_entry(&mut self, sequence: u64, input: OwnerInput, remote: RemoteEffect) {
        self.next.prompts.remove(&sequence);
        if !self.cancel_unconfirmed_for(&input)
            && !self.next.ports.previous_wire_prompts.contains_key(&input)
        {
            self.reply_input(input, remote);
        }
    }

    pub(super) fn reply_input(&mut self, input: OwnerInput, remote: RemoteEffect) {
        self.next.ports.selected_skills.remove(&input.input);
        self.next.ports.delivery_evidence.remove(&input.input);
        if remote == RemoteEffect::None
            && let Some(reason) = self.next.ports.unsent_prompts.remove(&input)
        {
            self.release(SessionOwnerEffect::Reply(SessionOwnerReply::InputUnsent {
                input,
                reason,
            }));
            return;
        }
        let reply = match self.completing_prompt.take_if(|(id, _)| id == &input.input) {
            Some((_, Ok(completion))) => SessionOwnerReply::PromptCompleted { input, completion },
            Some((_, Err(failure))) => SessionOwnerReply::PromptFailed {
                input,
                remote,
                failure,
            },
            None => SessionOwnerReply::InputFinished { input, remote },
        };
        self.release(SessionOwnerEffect::Reply(reply));
    }
}
