use super::{OwnerRejection, OwnerResult, OwnerTransition, remote_effect};
use crate::queued_entry_machine::{QueuedEntryEffect, SentOutcome};
use crate::session_cell_machine::{QueuedOperation, SessionCellEffect, SessionCellEvent};
use crate::session_owner::{AuthorizedWork, OwnerEffect, RevokeReason, TurnMembership};
use crate::session_owner::{
    EffectOutcome, InputWait, OwnerInput, PendingPrompt, PromptAdmission, RemoteEffect,
    SessionDeadline, SessionOwnerEffect, SessionOwnerRefusal, SessionOwnerReply,
    SessionPublication, SessionStoreRequest, WireOutcome,
};
use crate::session_turn_machine::{SessionTurn, TurnId};
use crate::{SessionGeneration, session_turn_machine::SessionTurnEffect};
use session_event_model::InputId;

impl OwnerTransition {
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
        if let (PromptAdmission::Auto { steering_supported }, SessionTurn::Running { turn }) =
            (admission, self.next.turn())
        {
            // The owner selects the turn from its current session state.
            return self.admit_steer(OwnerInput { turn, ..input }, steering_supported);
        }
        if admission == PromptAdmission::Direct
            && self.is_live(input.generation)
            && self.prompt_in_progress()
        {
            return Err(SessionOwnerRefusal::EffectInProgress.into());
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
        self.next.prompts.insert(
            sequence,
            PendingPrompt {
                input,
                wait: InputWait::Admission,
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
        let Some((&sequence, pending)) = self.next.prompts.first_key_value() else {
            return Ok(());
        };
        if !self.is_live(pending.input.generation) {
            return Ok(());
        }
        self.cell_move(SessionCellEvent::Claim { sequence })?;
        self.cell_move(SessionCellEvent::Commit { sequence })?;
        let effects = self.cell_move(SessionCellEvent::StartSendClaimed { sequence })?;
        self.apply_entry_effects(effects)
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
        if let Some(reason) = self.next.ports.unsent_prompts.remove(input) {
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
        let sequence = self.prompt_sequence(input, InputWait::UndoStore)?;
        let event = if outcome == EffectOutcome::Applied {
            SessionCellEvent::UnmarkCommitted { sequence }
        } else {
            SessionCellEvent::UnmarkFailed { sequence }
        };
        let effects = self.cell_move(event)?;
        self.apply_entry_effects(effects)?;
        self.drive_prompt_line()
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
        outcome: WireOutcome<()>,
    ) -> OwnerResult {
        self.prompt_sequence(input, InputWait::Wire)?;
        let outcome = match outcome {
            WireOutcome::Confirmed(()) => SentOutcome::Applied,
            WireOutcome::Refused => SentOutcome::Refused,
            WireOutcome::Unknown => SentOutcome::Unknown,
        };
        self.finish_turn(input, outcome, self.is_live(input.generation))?;
        self.continue_close()?;
        self.drive_prompt_line()
    }

    pub(super) fn end_turn_locally(&mut self, turn: TurnId) -> OwnerResult {
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
        self.finish_turn(&input, SentOutcome::Unknown, false)
    }

    fn finish_turn(&mut self, input: &OwnerInput, outcome: SentOutcome, idle: bool) -> OwnerResult {
        let effects = self.cell_move(SessionCellEvent::TurnEnded {
            turn: input.turn,
            outcome,
        })?;
        self.retire_turn_tools(input.generation, input.turn);
        self.next.ports.last_live_turn = Some(input.turn);
        self.effects
            .push(OwnerEffect::CallbackMembership(TurnMembership::End {
                run: input.generation,
                turn: input.turn,
            }));
        self.release(SessionOwnerEffect::Publish(SessionPublication::TurnEnded {
            generation: input.generation,
            turn: input.turn,
            idle,
        }));
        self.apply_entry_effects(effects)
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
        self.require_live(input.generation)?;
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
        self.release(SessionOwnerEffect::Store(
            SessionStoreRequest::MarkSteerDispatch(input.clone()),
        ));
        self.next.ports.admitted_inputs.insert(input.input.clone());
        self.wait_for_steer(input, InputWait::DispatchStore);
        Ok(())
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
        self.next.steer = None;
        self.reply_input(input.clone(), remote_effect(&outcome));
        Ok(())
    }

    fn steer_is_pending(&self, input: &OwnerInput, wait: InputWait) -> bool {
        self.next
            .steer
            .as_ref()
            .is_some_and(|pending| &pending.input == input && pending.wait == wait)
    }

    fn wait_for_steer(&mut self, input: OwnerInput, wait: InputWait) {
        self.next.steer = Some(PendingPrompt {
            input: input.clone(),
            wait,
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
        self.reply_input(input, remote);
    }

    fn reply_input(&mut self, input: OwnerInput, remote: RemoteEffect) {
        self.release(SessionOwnerEffect::Reply(
            SessionOwnerReply::InputFinished { input, remote },
        ));
    }
}
