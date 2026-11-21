//! Drive the queue head and handle the exact wait when its dispatch is busy.

use super::{OwnerRejection, OwnerResult, OwnerTransition};
use crate::session_cell_machine::{QueuedOperation, SessionCellEvent};
use crate::session_owner::{DeliveryPlacementKind, InputWait, OwnerInput, RevokeReason};

impl OwnerTransition {
    pub(super) fn drive_prompt_line(&mut self) -> OwnerResult {
        if !self.next.ports.connection_alive
            || self.prompt_in_progress()
            || self.next.cell.settings_held()
            || self.next.setting.is_some()
        {
            return Ok(());
        }
        while let Some((&sequence, pending)) = self.next.prompts.first_key_value() {
            if pending.parked_mark.is_some() || !self.is_live(pending.input.generation) {
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

    pub(super) fn park_queued_dispatch(
        &mut self,
        input: &OwnerInput,
        attempt: uuid::Uuid,
    ) -> OwnerResult {
        let sequence = self.prompt_sequence(input, InputWait::DispatchStore)?;
        if !self
            .next
            .prompts
            .get(&sequence)
            .is_some_and(|pending| pending.queued)
        {
            return Err(OwnerRejection::IgnoredLate);
        }
        let effects = self.cell_move(SessionCellEvent::MarkBusy { sequence })?;
        if effects.is_empty() {
            let pending = self
                .next
                .prompts
                .get_mut(&sequence)
                .ok_or(OwnerRejection::IgnoredLate)?;
            pending.wait = InputWait::Admission;
            pending.parked_mark = Some(attempt);
        }
        self.apply_entry_effects(effects)
    }

    pub(super) fn retry_queued_dispatch(
        &mut self,
        input: &OwnerInput,
        attempt: uuid::Uuid,
    ) -> OwnerResult {
        let mut head = self
            .next
            .prompts
            .first_entry()
            .ok_or(OwnerRejection::IgnoredLate)?;
        let pending = head.get_mut();
        if &pending.input != input || pending.parked_mark != Some(attempt) {
            return Err(OwnerRejection::IgnoredLate);
        }
        pending.parked_mark = None;
        self.drive_prompt_line()
    }
}
