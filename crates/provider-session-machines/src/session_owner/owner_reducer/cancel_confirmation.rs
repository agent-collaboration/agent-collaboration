//! Keep a cancelled prompt's terminal answer separate from the physical turn.

use super::{OwnerRejection, OwnerResult, OwnerTransition, PreviousPromptSettlement};
use crate::session_owner::{
    DispatchAuthorization, EffectOutcome, InputWait, OwnerDeadline, OwnerInput, PromptCompletion,
    SessionOwnerEffect, SessionOwnerReply, SessionStoreRequest, SessionTurn, WireOutcome,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::session_owner) struct PendingCancelConfirmation {
    authorization: DispatchAuthorization,
    pub input: OwnerInput,
    confirmation: CancelConfirmation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CancelConfirmation {
    Waiting,
    Unconfirmed,
    Reconciling(PromptCompletion),
}

impl PendingCancelConfirmation {
    pub(super) fn is_unconfirmed(&self) -> bool {
        self.confirmation != CancelConfirmation::Waiting
    }
}

impl super::SessionOwnerState {
    pub(super) fn cancel_confirmation_waiting(&self, key: &DispatchAuthorization) -> bool {
        self.ports
            .cancel_confirmation
            .as_ref()
            .is_some_and(|pending| &pending.authorization == key && !pending.is_unconfirmed())
    }
}

impl OwnerTransition {
    pub(super) fn wait_for_cancel_confirmation(
        &mut self,
        key: DispatchAuthorization,
        input: OwnerInput,
    ) {
        if self
            .next
            .prompts
            .values()
            .any(|prompt| prompt.input == input && prompt.wait == InputWait::Wire)
            || self.next.ports.provider_started_turn.as_ref() == Some(&input)
        {
            self.next.ports.cancel_confirmation = Some(PendingCancelConfirmation {
                authorization: key.clone(),
                input,
                confirmation: CancelConfirmation::Waiting,
            });
            self.arm(
                OwnerDeadline::CancelConfirmation(key),
                super::publication_effects::WIRE_BUDGET,
            );
        }
    }

    pub(super) fn expire_cancel_confirmation(
        &mut self,
        key: &DispatchAuthorization,
    ) -> OwnerResult {
        if !self.next.cancel_confirmation_waiting(key) {
            return Err(OwnerRejection::IgnoredLate);
        }
        let turn = self.next.turn();
        let pending = self
            .next
            .ports
            .cancel_confirmation
            .as_mut()
            .ok_or(OwnerRejection::IgnoredLate)?;
        if turn
            != (SessionTurn::Cancelling {
                turn: pending.input.turn,
            })
        {
            return Err(OwnerRejection::IgnoredLate);
        }
        pending.confirmation = CancelConfirmation::Unconfirmed;
        let input = pending.input.clone();
        // A provider-started steer already returned an applied receipt. Its
        // turn can become unconfirmed without re-settling that operation.
        if self.next.ports.provider_started_turn.as_ref() != Some(&input) {
            self.record_terminal(SessionOwnerReply::PromptCancelUnconfirmed { input });
        }
        Ok(())
    }

    pub(super) fn cancel_unconfirmed_for(&self, input: &OwnerInput) -> bool {
        self.next
            .ports
            .cancel_confirmation
            .as_ref()
            .is_some_and(|pending| &pending.input == input && pending.is_unconfirmed())
    }

    pub(super) fn clear_cancel_confirmation(&mut self, input: &OwnerInput) {
        self.next
            .ports
            .cancel_confirmation
            .take_if(|pending| &pending.input == input);
    }

    pub(super) fn detach_cancel_confirmation(
        &mut self,
        input: &OwnerInput,
    ) -> PreviousPromptSettlement {
        match self
            .next
            .ports
            .cancel_confirmation
            .take_if(|pending| &pending.input == input)
            .map(|pending| pending.confirmation)
        {
            None | Some(CancelConfirmation::Waiting) => PreviousPromptSettlement::AwaitingWire,
            Some(CancelConfirmation::Unconfirmed) => PreviousPromptSettlement::CancelUnconfirmed,
            Some(CancelConfirmation::Reconciling(completion)) => {
                PreviousPromptSettlement::Reconciling(completion)
            }
        }
    }

    pub(super) fn reconcile_cancelled_prompt(
        &mut self,
        input: &OwnerInput,
        outcome: WireOutcome<PromptCompletion>,
    ) -> OwnerResult {
        let pending = self
            .next
            .ports
            .cancel_confirmation
            .as_mut()
            .filter(|pending| {
                &pending.input == input && pending.confirmation == CancelConfirmation::Unconfirmed
            })
            .ok_or(OwnerRejection::IgnoredLate)?;
        let WireOutcome::Confirmed(completion) = outcome else {
            // This exact request has answered, but it gave no proof the turn stopped.
            return Ok(());
        };
        pending.confirmation = CancelConfirmation::Reconciling(completion);
        self.release(SessionOwnerEffect::Store(
            SessionStoreRequest::ReconcileLateCancelledPrompt(input.clone()),
        ));
        Ok(())
    }

    pub(super) fn late_cancelled_prompt_reconciled(
        &mut self,
        input: &OwnerInput,
        outcome: EffectOutcome,
    ) -> OwnerResult {
        if let Some(PreviousPromptSettlement::Reconciling(completion)) =
            self.next.ports.previous_wire_prompts.get(input).cloned()
        {
            self.next.ports.previous_wire_prompts.remove(input);
            if outcome == EffectOutcome::Applied {
                self.publish_previous_completion(input, &completion);
            }
            return Ok(());
        }
        let pending = self
            .next
            .ports
            .cancel_confirmation
            .as_mut()
            .filter(|pending| &pending.input == input)
            .ok_or(OwnerRejection::IgnoredLate)?;
        let CancelConfirmation::Reconciling(completion) = &pending.confirmation else {
            return Err(OwnerRejection::IgnoredLate);
        };
        if outcome == EffectOutcome::Applied {
            let completion = completion.clone();
            self.finish_observed_prompt(input, WireOutcome::Confirmed(completion))
        } else {
            pending.confirmation = CancelConfirmation::Unconfirmed;
            Ok(())
        }
    }
}
