use super::{OwnerRejection, OwnerResult, OwnerTransition, publication_effects::LOCAL_BUDGET};
use crate::{
    session_cell_machine::{QueuedOperation, SessionCellEvent},
    session_lifecycle_machine::{LoadAttempt, RestoreVia},
    session_owner::{
        DeliveryEvidenceState, DeliveryInput, DeliveryPlacement, DeliveryPlacementKind,
        EffectOutcome, InputWait, OwnerDeadline, OwnerEffect, OwnerInput, PromptAdmission,
        RemoteEffect, RevokeReason, SessionTurn,
    },
};

impl OwnerTransition {
    pub(super) fn queue_cold_delivery(
        &mut self,
        input: OwnerInput,
        attempt: LoadAttempt,
        selected: Vec<session_event_model::SelectedSkill>,
        record_evidence: bool,
    ) -> OwnerResult {
        self.start_restore_after_queue_evidence(
            attempt,
            RestoreVia::Load,
            record_evidence.then_some(input.operation),
        )?;
        if record_evidence {
            self.place_delivery(
                input,
                DeliveryInput::Prompt {
                    admission: PromptAdmission::Queue,
                    selected,
                },
            )
        } else {
            self.admit_selected_prompt(input, PromptAdmission::Queue, selected)
        }
    }

    pub(super) fn place_delivery(
        &mut self,
        input: OwnerInput,
        delivery: DeliveryInput,
    ) -> OwnerResult {
        self.next
            .ports
            .delivery_evidence
            .insert(input.input.clone(), DeliveryEvidenceState::Waiting);
        match delivery {
            DeliveryInput::Prompt {
                admission,
                selected,
            } => self.admit_selected_prompt(input, admission, selected),
            DeliveryInput::Steer { supported } => self.admit_steer(input, supported),
        }
    }

    pub(super) fn await_delivery_evidence(
        &mut self,
        input: &OwnerInput,
        kind: DeliveryPlacementKind,
    ) -> bool {
        use DeliveryEvidenceState::{Offered, Recorded};
        let Some(state) = self.next.ports.delivery_evidence.get_mut(&input.input) else {
            return false;
        };
        match state {
            Offered(_) => return true,
            Recorded(recorded) if *recorded == kind => return false,
            _ => {}
        }
        let placement = DeliveryPlacement {
            owner: self.next.ports.address,
            input: input.clone(),
            kind,
        };
        *state = Offered(placement.clone());
        self.effects
            .push(OwnerEffect::RecordDeliveryEvidence(placement.clone()));
        self.arm(OwnerDeadline::DeliveryEvidence(placement), LOCAL_BUDGET);
        true
    }

    pub(super) fn delivery_evidence_recorded(
        &mut self,
        placement: DeliveryPlacement,
        outcome: EffectOutcome,
    ) -> OwnerResult {
        if self
            .next
            .ports
            .delivery_evidence
            .get(&placement.input.input)
            != Some(&DeliveryEvidenceState::Offered(placement.clone()))
        {
            return Err(OwnerRejection::IgnoredLate);
        }
        let DeliveryPlacement { input, kind, .. } = placement;
        if outcome != EffectOutcome::Applied {
            self.reject_delivery_placement(
                input.clone(),
                kind,
                RevokeReason::DeliveryEvidenceUnavailable,
            )?;
            return self.release_cold_restore(&input, false);
        }

        self.next
            .ports
            .delivery_evidence
            .insert(input.input.clone(), DeliveryEvidenceState::Recorded(kind));
        match kind {
            DeliveryPlacementKind::Queue => {
                self.effects.push(OwnerEffect::InputQueued(input.clone()));
                self.release_cold_restore(&input, true)?;
                self.drive_prompt_line()
            }
            DeliveryPlacementKind::Start => self.drive_prompt_line(),
            DeliveryPlacementKind::Steer => {
                if !self.is_live(input.generation)
                    || self.next.turn() != (SessionTurn::Running { turn: input.turn })
                {
                    return self.reject_delivery_placement(input, kind, RevokeReason::TurnEnded);
                }
                self.start_steer_mark(input);
                Ok(())
            }
        }
    }

    pub(super) fn reject_delivery_placement(
        &mut self,
        input: OwnerInput,
        kind: DeliveryPlacementKind,
        reason: RevokeReason,
    ) -> OwnerResult {
        self.next.ports.unsent_prompts.insert(input.clone(), reason);
        if kind == DeliveryPlacementKind::Steer {
            if !self.next.steer.as_ref().is_some_and(|pending| {
                pending.input == input && pending.wait == InputWait::Admission
            }) {
                return Err(OwnerRejection::IgnoredLate);
            }
            self.next.steer = None;
            self.reply_input(input, RemoteEffect::None);
            self.continue_close()?;
        } else {
            let operation = QueuedOperation(input.input.as_str().to_owned());
            let effects = self.cell_move(SessionCellEvent::CancelEntry { operation })?;
            self.apply_entry_effects(effects)?;
        }
        self.drive_prompt_line()
    }
}
