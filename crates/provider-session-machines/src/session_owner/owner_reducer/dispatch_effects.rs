use super::{OwnerRejection, OwnerResult, OwnerTransition};
use crate::SessionGeneration;
use crate::queued_entry_machine::SentOutcome;
use crate::session_cell_machine::SessionCellEvent;
use crate::session_owner::{
    AuthorizationStatus, AuthorizationTarget, AuthorizedWork, CancelDrainReport,
    DispatchAuthorization, DispatchResult, InputWait, OwnerDeadline, OwnerEffect, OwnerInput,
    PendingAuthorization, RevokeReason, SessionOwnerEffect, SessionPublication, SessionTurn,
    TurnId, TurnMembership, WireOutcome,
};
use session_event_model::InputId;

impl AuthorizedWork {
    fn route(&self) -> (SessionGeneration, &InputId) {
        match self {
            Self::Prompt(input) | Self::Steer(input) | Self::Cancel(input) => {
                (input.generation, &input.input)
            }
            Self::Setting {
                input_id, setting, ..
            } => (setting.change.generation, input_id),
        }
    }

    fn target(&self) -> AuthorizationTarget {
        match self {
            Self::Prompt(_) => AuthorizationTarget::NewTurn,
            Self::Steer(input) | Self::Cancel(input) => {
                AuthorizationTarget::ExistingTurn { turn: input.turn }
            }
            Self::Setting { .. } => AuthorizationTarget::Session,
        }
    }
}

enum Unsent {
    Revoked,
    Transport,
    StaleTarget,
}

impl OwnerTransition {
    pub(super) fn issue_cancel(
        &mut self,
        generation: SessionGeneration,
        turn: TurnId,
    ) -> OwnerResult {
        if self.next.ports.authorizations.values().any(|pending| {
            matches!(&pending.work, AuthorizedWork::Cancel(input)
            if input.generation == generation && input.turn == turn)
        }) {
            if self
                .closing()
                .is_some_and(|(run, _)| run.generation == generation)
            {
                return Ok(());
            }
            return Err(super::super::SessionOwnerRefusal::EffectInProgress.into());
        }
        let foreground = self
            .next
            .prompts
            .values()
            .find(|pending| {
                pending.input.generation == generation
                    && pending.input.turn == turn
                    && pending.wait == InputWait::Wire
            })
            .map(|pending| pending.input.clone())
            .or_else(|| {
                self.next
                    .ports
                    .provider_started_turn
                    .clone()
                    .filter(|input| input.generation == generation && input.turn == turn)
            })
            .ok_or(OwnerRejection::IgnoredLate)?;
        self.issue_authorization(AuthorizedWork::Cancel(foreground))
    }

    pub(super) fn issue_authorization(&mut self, work: AuthorizedWork) -> OwnerResult {
        let (run, input_id) = work.route();
        if !matches!(&work, AuthorizedWork::Cancel(input)
            if self.next.turn() == (SessionTurn::Cancelling { turn: input.turn })
                && self.closing().is_some_and(|(closing, _)| closing.generation == run))
        {
            self.require_live(run)?;
        }
        let Some(sequence) = self.next.ports.next_authorization.checked_add(1) else {
            return Err(OwnerRejection::IgnoredLate);
        };
        self.next.ports.next_authorization = sequence;
        let authorization = DispatchAuthorization {
            incarnation: self.next.ports.address.incarnation,
            run,
            input_id: input_id.clone(),
            authorization_seq: sequence,
            target: work.target(),
        };
        if let AuthorizedWork::Prompt(input) = &work {
            self.effects.push(OwnerEffect::CallbackMembership(
                TurnMembership::Provisional {
                    run,
                    turn: input.turn,
                },
            ));
        }
        let cancel = matches!(work, AuthorizedWork::Cancel(_));
        self.next.ports.authorizations.insert(
            authorization.clone(),
            PendingAuthorization {
                work: work.clone(),
                status: if cancel {
                    AuthorizationStatus::DrainingCancel
                } else {
                    AuthorizationStatus::Offered
                },
            },
        );
        let deadline = if cancel {
            self.effects
                .push(OwnerEffect::DrainCancel(authorization.clone()));
            OwnerDeadline::CancelDrain(authorization)
        } else {
            self.effects.push(OwnerEffect::Authorize {
                authorization: authorization.clone(),
                work,
            });
            OwnerDeadline::Enqueue(authorization)
        };
        self.arm(deadline, super::publication_effects::LOCAL_BUDGET);
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
            .iter()
            .find(|(key, pending)| {
                key.run == run
                    && &key.input_id == input_id
                    && pending.status == AuthorizationStatus::Offered
            })
            .map(|(key, _)| key.clone());
        candidate.is_some_and(|key| self.revoke(&key, reason).is_ok())
    }

    pub(super) fn revoke_run(&mut self, run: SessionGeneration) {
        let matching: Vec<_> = self
            .next
            .ports
            .authorizations
            .iter()
            .filter(|(key, pending)| {
                key.run == run
                    && matches!(
                        pending.status,
                        AuthorizationStatus::DrainingCancel | AuthorizationStatus::Offered
                    )
            })
            .map(|(key, _)| key.clone())
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
        if !matches!(
            pending.status,
            AuthorizationStatus::DrainingCancel | AuthorizationStatus::Offered
        ) {
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
            DispatchResult::CancelDrainFinished(authorization) => {
                self.finish_cancel_drain(authorization, false)
            }
            DispatchResult::CancelTaken {
                authorization,
                drain,
            } => self.authorization_taken(&authorization, Some(drain)),
            DispatchResult::Taken(key) | DispatchResult::AlreadyTaken(key) => {
                self.authorization_taken(&key, None)
            }
            DispatchResult::Revoked(key) => self.authorization_unsent(&key, Unsent::Revoked),
            DispatchResult::DefinitelyUnsent(key) => {
                self.authorization_unsent(&key, Unsent::Transport)
            }
            DispatchResult::StaleTarget(key) => {
                self.authorization_unsent(&key, Unsent::StaleTarget)
            }
            DispatchResult::PromptFinished {
                authorization,
                outcome,
            } => {
                let AuthorizedWork::Prompt(input) = self.take_wire_result(&authorization)? else {
                    return Err(OwnerRejection::IgnoredLate);
                };
                self.finish_prompt(&input, outcome)
            }
            DispatchResult::PromptFailed {
                authorization,
                failure,
            } => {
                let AuthorizedWork::Prompt(input) = self.take_wire_result(&authorization)? else {
                    return Err(OwnerRejection::IgnoredLate);
                };
                self.completing_prompt = Some((input.input.clone(), Err(failure)));
                let outcome = if failure.definitely_no_effect() {
                    WireOutcome::Refused
                } else {
                    WireOutcome::Unknown
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
            DispatchResult::SteerInvalidReply(authorization) => {
                let AuthorizedWork::Steer(input) = self.take_wire_result(&authorization)? else {
                    return Err(OwnerRejection::IgnoredLate);
                };
                if !self.steer_is_pending(&input, InputWait::Wire) {
                    return Err(OwnerRejection::IgnoredLate);
                }
                self.next.steer = None;
                self.record_terminal(crate::session_owner::SessionOwnerReply::SteerInvalidReply {
                    input,
                });
                Ok(())
            }
            DispatchResult::SteerStartedNewTurn {
                authorization,
                turn,
            } => {
                let AuthorizedWork::Steer(input) = self.take_wire_result(&authorization)? else {
                    return Err(OwnerRejection::IgnoredLate);
                };
                self.steer_started_new_turn(input, turn)
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
                self.next.ports.authorizations.remove(&authorization);
                self.setting_on_wire(setting, observation)?;
                self.continue_close()
            }
            DispatchResult::SettingKept {
                authorization,
                reason,
            } => {
                let Some(pending) = self.next.ports.authorizations.get(&authorization) else {
                    return Err(OwnerRejection::IgnoredLate);
                };
                let AuthorizedWork::Setting { setting, .. } = pending.work else {
                    return Err(OwnerRejection::IgnoredLate);
                };
                if pending.status != AuthorizationStatus::Offered
                    || setting.kind != crate::ProviderSettingKind::Mode
                {
                    return Err(OwnerRejection::IgnoredLate);
                }
                self.next.ports.authorizations.remove(&authorization);
                self.setting_kept(setting, reason)?;
                self.continue_close()
            }
        }
    }

    pub(super) fn finish_cancel_drain(
        &mut self,
        authorization: DispatchAuthorization,
        expired: bool,
    ) -> OwnerResult {
        let pending = self
            .next
            .ports
            .authorizations
            .get_mut(&authorization)
            .filter(|pending| pending.status == AuthorizationStatus::DrainingCancel)
            .ok_or(OwnerRejection::IgnoredLate)?;
        pending.status = AuthorizationStatus::Offered;
        self.effects.push(OwnerEffect::TakeCancel {
            authorization,
            expired,
        });
        Ok(())
    }

    fn authorization_taken(
        &mut self,
        key: &DispatchAuthorization,
        drain: Option<CancelDrainReport>,
    ) -> OwnerResult {
        let pending = self
            .next
            .ports
            .authorizations
            .get_mut(key)
            .ok_or(OwnerRejection::IgnoredLate)?;
        if matches!(pending.work, AuthorizedWork::Cancel(_)) != drain.is_some() {
            return Err(OwnerRejection::IgnoredLate);
        }
        let revoke_reason = match pending.status {
            AuthorizationStatus::Offered => None,
            AuthorizationStatus::Revoking(reason) => Some(reason),
            AuthorizationStatus::DrainingCancel | AuthorizationStatus::Taken => {
                return Err(OwnerRejection::IgnoredLate);
            }
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
                self.effects.push(OwnerEffect::PromptTaken(input.clone()));
                if self.is_live(input.generation) {
                    self.release(SessionOwnerEffect::Publish(
                        SessionPublication::TurnStarted {
                            generation: input.generation,
                            turn: input.turn,
                            input: input.input.clone(),
                        },
                    ));
                    if revoke_reason == Some(RevokeReason::Cancelled) {
                        self.cancel_turn(input.generation, input.turn)?;
                    } else {
                        self.steer_contending_auto_after_take(&input)?;
                    }
                } else if self
                    .closing()
                    .is_some_and(|(run, _)| run.generation == input.generation)
                {
                    self.issue_cancel(input.generation, input.turn)?;
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
            AuthorizedWork::Cancel(input) => {
                let drain = drain.ok_or(OwnerRejection::IgnoredLate)?;
                let operation = input.operation;
                if self.is_live(input.generation)
                    && self.next.turn() == (SessionTurn::Running { turn: input.turn })
                {
                    self.cell_move(SessionCellEvent::CancelTurn)?;
                    self.wait_for_cancel_confirmation(key.clone(), input);
                }
                self.next.ports.authorizations.remove(key);
                self.effects.push(OwnerEffect::CancelSent {
                    authorization: key.clone(),
                    operation,
                    drain,
                });
                self.continue_close()?;
            }
        }
        Ok(())
    }

    fn authorization_unsent(&mut self, key: &DispatchAuthorization, unsent: Unsent) -> OwnerResult {
        let pending = self
            .next
            .ports
            .authorizations
            .remove(key)
            .ok_or(OwnerRejection::IgnoredLate)?;
        let was_taken = pending.status == AuthorizationStatus::Taken;
        let reason = match (pending.status, unsent) {
            (AuthorizationStatus::Revoking(reason), Unsent::Revoked) => reason,
            (
                AuthorizationStatus::Offered | AuthorizationStatus::Revoking(_),
                Unsent::StaleTarget,
            ) if matches!(key.target, AuthorizationTarget::ExistingTurn { .. }) => {
                RevokeReason::TurnEnded
            }
            (
                AuthorizationStatus::Taken
                | AuthorizationStatus::DrainingCancel
                | AuthorizationStatus::Offered
                | AuthorizationStatus::Revoking(_),
                Unsent::Transport,
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
                let membership = if !was_taken {
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
            AuthorizedWork::Steer(input) => self.unmark_steer_unsent(&input, reason),
            AuthorizedWork::Setting { setting, .. } => self.setting_unsent(setting)?,
            AuthorizedWork::Cancel(input) => {
                self.effects.push(OwnerEffect::CancelNotSent {
                    authorization: key.clone(),
                    operation: input.operation,
                    reason,
                });
                if reason == RevokeReason::Closed
                    && self
                        .closing()
                        .is_some_and(|(run, _)| run.generation == input.generation)
                    && self.next.turn() == (SessionTurn::Cancelling { turn: input.turn })
                {
                    self.issue_cancel(input.generation, input.turn)?;
                }
            }
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
                (
                    AuthorizationStatus::DrainingCancel
                    | AuthorizationStatus::Offered
                    | AuthorizationStatus::Revoking(_),
                    _,
                ) => {
                    let unsent = if matches!(pending.status, AuthorizationStatus::Revoking(_)) {
                        Unsent::Revoked
                    } else {
                        Unsent::Transport
                    };
                    self.authorization_unsent(&key, unsent)?;
                }
                (AuthorizationStatus::Taken, AuthorizedWork::Steer(_)) => {
                    self.steer_expired(&key)?
                }
                (
                    AuthorizationStatus::Taken,
                    AuthorizedWork::Prompt(_)
                    | AuthorizedWork::Setting { .. }
                    | AuthorizedWork::Cancel(_),
                ) => {
                    // The run's retirement below settles these started effects.
                }
            }
        }
        Ok(())
    }
}
