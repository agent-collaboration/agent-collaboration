use super::{OwnerRejection, OwnerResult, OwnerTransition};
use crate::SessionGeneration;
use crate::queued_entry_machine::SentOutcome;
use crate::session_cell_machine::SessionCellEvent;
use crate::session_owner::{
    AuthorizationStatus, AuthorizedWork, DispatchAuthorization, DispatchResult, InputWait,
    OwnerDeadline, OwnerEffect, OwnerInput, PendingAuthorization, RevokeReason, SessionOwnerEffect,
    SessionPublication, SessionWireRequest, TurnMembership, WireOutcome,
};
use session_event_model::InputId;

impl AuthorizedWork {
    fn route(&self) -> (SessionGeneration, &InputId) {
        match self {
            Self::Prompt(input) | Self::Steer(input) => (input.generation, &input.input),
            Self::Setting {
                input_id, setting, ..
            } => (setting.change.generation, input_id),
        }
    }
}

impl OwnerTransition {
    pub(super) fn issue_authorization(&mut self, work: AuthorizedWork) -> OwnerResult {
        let (run, input_id) = work.route();
        self.require_live(run)?;
        let Some(sequence) = self.next.ports.next_authorization.checked_add(1) else {
            return Err(OwnerRejection::IgnoredLate);
        };
        self.next.ports.next_authorization = sequence;
        let authorization = DispatchAuthorization {
            incarnation: self.next.ports.address.incarnation,
            run,
            input_id: input_id.clone(),
            authorization_seq: sequence,
        };
        if let AuthorizedWork::Prompt(input) = &work {
            self.effects.push(OwnerEffect::CallbackMembership(
                TurnMembership::Provisional {
                    run,
                    turn: input.turn,
                },
            ));
        }
        self.next.ports.authorizations.insert(
            authorization.clone(),
            PendingAuthorization {
                work: work.clone(),
                status: AuthorizationStatus::Offered,
            },
        );
        self.effects.push(OwnerEffect::Authorize {
            authorization: authorization.clone(),
            work,
        });
        self.arm(
            OwnerDeadline::Enqueue(authorization),
            super::publication_effects::LOCAL_BUDGET,
        );
        Ok(())
    }

    pub(super) fn revoke_input(
        &mut self,
        run: SessionGeneration,
        input_id: &InputId,
        reason: RevokeReason,
    ) -> bool {
        let candidate = self
            .next
            .ports
            .authorizations
            .keys()
            .find(|key| key.run == run && &key.input_id == input_id)
            .cloned();
        candidate.is_some_and(|key| self.revoke(&key, reason).is_ok())
    }

    pub(super) fn revoke_run(&mut self, run: SessionGeneration) {
        let matching: Vec<_> = self
            .next
            .ports
            .authorizations
            .keys()
            .filter(|key| key.run == run)
            .cloned()
            .collect();
        for key in matching {
            let _result = self.revoke(&key, RevokeReason::Closed);
        }
    }

    pub(super) fn revoke(
        &mut self,
        authorization: &DispatchAuthorization,
        reason: RevokeReason,
    ) -> OwnerResult {
        let pending = self
            .next
            .ports
            .authorizations
            .get_mut(authorization)
            .ok_or(OwnerRejection::IgnoredLate)?;
        if pending.status != AuthorizationStatus::Offered {
            return Err(OwnerRejection::IgnoredLate);
        }
        pending.status = AuthorizationStatus::Revoking(reason);
        self.effects.push(OwnerEffect::Revoke {
            authorization: authorization.clone(),
            reason,
        });
        Ok(())
    }

    pub(super) fn dispatch_result(&mut self, result: DispatchResult) -> OwnerResult {
        match result {
            DispatchResult::Taken(key) | DispatchResult::AlreadyTaken(key) => {
                self.authorization_taken(&key)
            }
            DispatchResult::Revoked(key) => self.authorization_unsent(&key, true),
            DispatchResult::DefinitelyUnsent(key) => self.authorization_unsent(&key, false),
            DispatchResult::PromptFinished {
                authorization,
                outcome,
            } => {
                let AuthorizedWork::Prompt(input) = self.take_wire_result(&authorization)? else {
                    return Err(OwnerRejection::IgnoredLate);
                };
                self.finish_prompt(&input, outcome)
            }
            DispatchResult::SteerFinished {
                authorization,
                outcome,
            } => {
                let AuthorizedWork::Steer(input) = self.take_wire_result(&authorization)? else {
                    return Err(OwnerRejection::IgnoredLate);
                };
                self.finish_steer(&input, outcome)
            }
            DispatchResult::PromptRequired {
                authorization,
                replacement_turn,
            } => {
                let AuthorizedWork::Steer(input) = self.take_wire_result(&authorization)? else {
                    return Err(OwnerRejection::IgnoredLate);
                };
                self.unmark_steer(&input);
                if self.is_live(input.generation) {
                    self.next.ports.steer_replacement = Some(OwnerInput {
                        turn: replacement_turn,
                        ..input
                    });
                }
                Ok(())
            }
            DispatchResult::SettingFinished {
                authorization,
                observation,
            } => {
                let pending = self.pending_wire(&authorization)?;
                let AuthorizedWork::Setting { setting, .. } = pending.work else {
                    return Err(OwnerRejection::IgnoredLate);
                };
                self.setting_on_wire(setting, observation)?;
                self.next.ports.authorizations.remove(&authorization);
                self.continue_close()
            }
        }
    }

    fn authorization_taken(&mut self, key: &DispatchAuthorization) -> OwnerResult {
        let pending = self
            .next
            .ports
            .authorizations
            .get_mut(key)
            .ok_or(OwnerRejection::IgnoredLate)?;
        let revoke_reason = match pending.status {
            AuthorizationStatus::Offered => None,
            AuthorizationStatus::Revoking(reason) => Some(reason),
            AuthorizationStatus::Taken => return Err(OwnerRejection::IgnoredLate),
        };
        pending.status = AuthorizationStatus::Taken;
        match pending.work.clone() {
            AuthorizedWork::Prompt(input) => {
                let prompt = self
                    .next
                    .prompts
                    .values_mut()
                    .find(|pending| pending.input == input)
                    .ok_or(OwnerRejection::IgnoredLate)?;
                prompt.wait = InputWait::Wire;
                if self.is_live(input.generation) {
                    self.release(SessionOwnerEffect::Publish(
                        SessionPublication::TurnStarted {
                            generation: input.generation,
                            turn: input.turn,
                        },
                    ));
                    if revoke_reason == Some(RevokeReason::Cancelled) {
                        self.cancel_turn(input.generation, input.turn)?;
                    }
                } else if self
                    .closing()
                    .is_some_and(|(run, _)| run.generation == input.generation)
                {
                    self.release(SessionOwnerEffect::Wire(SessionWireRequest::Cancel {
                        generation: input.generation,
                        turn: input.turn,
                    }));
                }
            }
            AuthorizedWork::Steer(_) => {
                if let Some(steer) = self.next.steer.as_mut() {
                    steer.wait = InputWait::Wire;
                    self.arm(
                        OwnerDeadline::SteerResult(key.clone()),
                        super::publication_effects::LOCAL_BUDGET,
                    );
                } else {
                    return Err(OwnerRejection::IgnoredLate);
                }
            }
            AuthorizedWork::Setting { setting, .. } => self.setting_taken(setting),
        }
        Ok(())
    }

    fn authorization_unsent(&mut self, key: &DispatchAuthorization, revoked: bool) -> OwnerResult {
        let pending = self
            .next
            .ports
            .authorizations
            .remove(key)
            .ok_or(OwnerRejection::IgnoredLate)?;
        let reason = match (pending.status, revoked) {
            (AuthorizationStatus::Revoking(reason), true) => reason,
            (
                AuthorizationStatus::Taken
                | AuthorizationStatus::Offered
                | AuthorizationStatus::Revoking(_),
                false,
            ) => {
                if self.next.ports.connection_alive {
                    RevokeReason::EnqueueExpired
                } else {
                    RevokeReason::ConnectionLost
                }
            }
            _ => return Err(OwnerRejection::IgnoredLate),
        };
        match pending.work {
            AuthorizedWork::Prompt(input) => {
                let membership = if revoked {
                    TurnMembership::RemoveOnRevoke {
                        run: input.generation,
                        turn: input.turn,
                    }
                } else {
                    TurnMembership::End {
                        run: input.generation,
                        turn: input.turn,
                    }
                };
                self.effects
                    .push(OwnerEffect::CallbackMembership(membership));
                self.cell_move(SessionCellEvent::TurnEnded {
                    turn: input.turn,
                    outcome: SentOutcome::Refused,
                })?;
                self.unmark_prompt(&input, reason)?;
                self.continue_close()?;
            }
            AuthorizedWork::Steer(input) => self.unmark_steer(&input),
            AuthorizedWork::Setting { setting, .. } => self.setting_unsent(setting)?,
        }
        self.continue_close()
    }

    fn pending_wire(
        &self,
        authorization: &DispatchAuthorization,
    ) -> OwnerResult<PendingAuthorization> {
        self.next
            .ports
            .authorizations
            .get(authorization)
            .filter(|pending| pending.status == AuthorizationStatus::Taken)
            .cloned()
            .ok_or(OwnerRejection::IgnoredLate)
    }

    fn take_wire_result(
        &mut self,
        authorization: &DispatchAuthorization,
    ) -> OwnerResult<AuthorizedWork> {
        let pending = self.pending_wire(authorization)?;
        self.next.ports.authorizations.remove(authorization);
        Ok(pending.work)
    }

    pub(super) fn steer_expired(&mut self, key: &DispatchAuthorization) -> OwnerResult {
        let AuthorizedWork::Steer(input) = self.take_wire_result(key)? else {
            return Err(OwnerRejection::IgnoredLate);
        };
        self.finish_steer(&input, WireOutcome::Unknown)
    }

    pub(super) fn abandon_taken_prompt(&mut self, input: &OwnerInput) {
        self.next.ports.authorizations.retain(|_, pending| {
            pending.work != AuthorizedWork::Prompt(input.clone())
                || pending.status != AuthorizationStatus::Taken
        });
    }

    pub(super) fn lose_transport_authorizations(&mut self) -> OwnerResult {
        // Transport reports successful takes before reporting loss for the epoch.
        // Correlations may be lost, but the ordered enqueue decisions are known.
        let pending: Vec<_> = self
            .next
            .ports
            .authorizations
            .iter()
            .map(|(key, state)| (key.clone(), state.clone()))
            .collect();
        for (key, pending) in pending {
            match (&pending.status, &pending.work) {
                (AuthorizationStatus::Offered | AuthorizationStatus::Revoking(_), _) => {
                    self.authorization_unsent(
                        &key,
                        matches!(pending.status, AuthorizationStatus::Revoking(_)),
                    )?;
                }
                (AuthorizationStatus::Taken, AuthorizedWork::Steer(_)) => {
                    self.steer_expired(&key)?
                }
                (
                    AuthorizationStatus::Taken,
                    AuthorizedWork::Prompt(_) | AuthorizedWork::Setting { .. },
                ) => {
                    // The run's retirement below settles these started effects.
                }
            }
        }
        Ok(())
    }
}
