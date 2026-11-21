use super::{OwnerRejection, OwnerResult, OwnerTransition};
use crate::session_lifecycle_machine::RestoreVia;
use crate::session_owner::{
    AuthorizationStatus, AuthorizedWork, BatchId, HistoryAvailability, InputWait, ObservedSetting,
    OwnerDeadline, OwnerEffect, OwnerInput, PublicationBatch, PublicationFact, PublicationGap,
    PublicationResult, PublicationStatus, QueueEntrySnapshot, RestoreWait, RevokeReason,
    SessionDeadline, SessionOwnerState, SessionPublication, SessionSnapshot, SettingWait,
};
use crate::session_turn_machine::SessionTurn;
use std::time::Duration;

pub(super) const LOCAL_BUDGET: Duration = Duration::from_secs(10);
pub(super) const WIRE_BUDGET: Duration = Duration::from_secs(30);

impl SessionOwnerState {
    /// Task timers can be dropped as soon as their stamped work settles.
    #[must_use]
    pub fn has_pending_deadline(&self, deadline: &OwnerDeadline) -> bool {
        match deadline {
            OwnerDeadline::DeliveryEvidence(placement) => {
                self.ports.delivery_evidence.get(&placement.input.input)
                    == Some(&super::super::DeliveryEvidenceState::Offered(
                        placement.clone(),
                    ))
            }
            OwnerDeadline::ReadSetup(run) => self.waiting_for_restore(*run, RestoreWait::Setup),
            OwnerDeadline::RestoreWire(run) => self.waiting_for_restore(*run, RestoreWait::Wire),
            OwnerDeadline::RestoreMode(run) => self.waiting_for_restore(*run, RestoreWait::Mode),
            OwnerDeadline::RestoreFastOff(run) => {
                self.waiting_for_restore(*run, RestoreWait::FastOff)
            }
            OwnerDeadline::HostPrepare(run) => {
                self.waiting_for_restore(*run, RestoreWait::Preparation)
            }
            OwnerDeadline::DispatchMark { input } => {
                self.waiting_for_input(input, InputWait::DispatchStore)
            }
            OwnerDeadline::UndoStore { input } => {
                self.ports.unsent_prompts.contains_key(input)
                    || self.waiting_for_input(input, InputWait::UndoStore)
            }
            OwnerDeadline::Close(run) => matches!(
                self.lifecycle(),
                super::super::SessionLifecycle::Closing { generation, attempt, .. }
                    if generation == run.generation && attempt == run.attempt
            ),
            OwnerDeadline::SettingWire(setting) => {
                self.setting == Some((*setting, SettingWait::Wire))
            }
            OwnerDeadline::SettingStore(setting) => {
                self.setting == Some((*setting, SettingWait::Store))
            }
            OwnerDeadline::TerminalStore(write) => self.ports.pending_terminal.contains_key(write),
            OwnerDeadline::Enqueue(key) => self
                .ports
                .authorizations
                .get(key)
                .is_some_and(|pending| pending.status == AuthorizationStatus::Offered),
            OwnerDeadline::CancelDrain(key) => self
                .ports
                .authorizations
                .get(key)
                .is_some_and(|pending| pending.status == AuthorizationStatus::DrainingCancel),
            OwnerDeadline::CancelConfirmation(key) => self.cancel_confirmation_waiting(key),
            OwnerDeadline::SteerResult(key) => {
                self.ports.authorizations.get(key).is_some_and(|pending| {
                    pending.status == AuthorizationStatus::Taken
                        && matches!(pending.work, AuthorizedWork::Steer(_))
                })
            }
            OwnerDeadline::Publication(id) => self.ports.awaiting_apply.contains(id),
            OwnerDeadline::Rebase(id) => self.ports.rebase == Some(*id),
            OwnerDeadline::Create(operation) => self.ports.create == Some(*operation),
        }
    }

    fn waiting_for_input(&self, input: &OwnerInput, wait: InputWait) -> bool {
        self.prompts
            .values()
            .chain(self.steer.iter())
            .any(|pending| &pending.input == input && pending.wait == wait)
    }

    fn waiting_for_restore(
        &self,
        run: super::super::RunAttempt<crate::session_lifecycle_machine::LoadAttempt>,
        wait: RestoreWait,
    ) -> bool {
        self.restore
            .is_some_and(|pending| pending.run() == run && pending.wait() == wait)
    }

    /// A projection of committed owner facts for readers outside the mailbox.
    #[must_use]
    pub fn snapshot(&self) -> SessionSnapshot {
        let turn = self.published_turn();
        let mut settings = self.ports.observed.clone();
        if !matches!(
            self.lifecycle(),
            super::super::SessionLifecycle::Live { .. }
        ) {
            for evidence in settings.values_mut() {
                if let ObservedSetting::Confirmed(value) = &evidence.value {
                    evidence.value = ObservedSetting::LastConfirmed(value.clone());
                }
            }
        }
        SessionSnapshot {
            owner: self.ports.address,
            lifecycle: self.lifecycle(),
            turn,
            foreground: self
                .prompts
                .values()
                .find(|pending| pending.wait == InputWait::Wire)
                .map(|pending| pending.input.clone())
                .or_else(|| self.ports.provider_started_turn.clone()),
            recent_steer: self.ports.recent_steer.clone(),
            settings,
            mode_reason: self.ports.mode_reason,
            settings_block_input: self.cell.settings_held(),
            publication: self.ports.publication,
            history: self.ports.history,
            last_restore: self.ports.last_restore.clone(),
            last_live_turn: self.ports.last_live_turn.clone(),
            cancel_unconfirmed: self
                .ports
                .cancel_confirmation
                .as_ref()
                .filter(|pending| pending.is_unconfirmed())
                .map(|pending| pending.input.clone()),
            provider_started_turn: self.ports.provider_started_turn.clone(),
            projection: self.project_snapshot(turn),
            queue: self
                .prompts
                .iter()
                .map(|(&sequence, pending)| QueueEntrySnapshot {
                    sequence,
                    input: pending.input.clone(),
                    started: pending.wait == InputWait::Wire,
                })
                .collect(),
        }
    }

    pub(super) fn published_turn(&self) -> SessionTurn {
        match self.turn() {
            SessionTurn::Running { turn } | SessionTurn::Cancelling { turn }
                if self.ports.authorizations.values().any(|pending| matches!(&pending.work,
                    AuthorizedWork::Prompt(input) if input.turn == turn && pending.status != AuthorizationStatus::Taken)) => SessionTurn::Idle,
            turn => turn,
        }
    }
}

impl OwnerTransition {
    pub(super) fn arm(&mut self, deadline: OwnerDeadline, budget: Duration) {
        self.effects.push(OwnerEffect::Arm { deadline, budget });
    }

    pub(super) fn arm_business(&mut self, deadline: SessionDeadline) {
        let (deadline, budget) = match deadline {
            SessionDeadline::Restore { run, wait } => match wait {
                RestoreWait::Setup => (OwnerDeadline::ReadSetup(run), LOCAL_BUDGET),
                RestoreWait::Wire => (OwnerDeadline::RestoreWire(run), WIRE_BUDGET),
                RestoreWait::Mode => (OwnerDeadline::RestoreMode(run), WIRE_BUDGET),
                RestoreWait::FastOff => (OwnerDeadline::RestoreFastOff(run), WIRE_BUDGET),
                RestoreWait::Preparation => (OwnerDeadline::HostPrepare(run), LOCAL_BUDGET),
            },
            SessionDeadline::Close(run) => (OwnerDeadline::Close(run), WIRE_BUDGET),
            SessionDeadline::Prompt { input, wait } | SessionDeadline::Steer { input, wait } => {
                match wait {
                    InputWait::DispatchStore => {
                        (OwnerDeadline::DispatchMark { input }, LOCAL_BUDGET)
                    }
                    InputWait::UndoStore => (OwnerDeadline::UndoStore { input }, LOCAL_BUDGET),
                    InputWait::Admission | InputWait::Enqueue | InputWait::Wire => return,
                }
            }
            SessionDeadline::Setting { setting, wait } => match wait {
                SettingWait::Enqueue => return,
                SettingWait::Wire => (OwnerDeadline::SettingWire(setting), WIRE_BUDGET),
                SettingWait::Store => (OwnerDeadline::SettingStore(setting), LOCAL_BUDGET),
            },
        };
        self.arm(deadline, budget);
    }

    pub(super) fn port_deadline(&mut self, deadline: OwnerDeadline) -> OwnerResult {
        match deadline {
            OwnerDeadline::DeliveryEvidence(placement) => {
                self.delivery_evidence_recorded(placement, super::super::EffectOutcome::Unknown)
            }
            OwnerDeadline::TerminalStore(write) => {
                self.terminal_recorded(write, super::super::EffectOutcome::Unknown)
            }
            OwnerDeadline::ReadSetup(run) => self.restore_deadline(run, RestoreWait::Setup),
            OwnerDeadline::RestoreWire(run) => self.restore_deadline(run, RestoreWait::Wire),
            OwnerDeadline::RestoreMode(run) => self.restore_deadline(run, RestoreWait::Mode),
            OwnerDeadline::RestoreFastOff(run) => self.restore_deadline(run, RestoreWait::FastOff),
            OwnerDeadline::HostPrepare(run) => self.restore_deadline(run, RestoreWait::Preparation),
            OwnerDeadline::Close(run) => self.close_deadline(run),
            OwnerDeadline::DispatchMark { input } => {
                if self
                    .next
                    .steer
                    .as_ref()
                    .is_some_and(|pending| pending.input == input)
                {
                    self.steer_deadline(&input, InputWait::DispatchStore)
                } else {
                    self.prompt_deadline(&input, InputWait::DispatchStore)
                }
            }
            OwnerDeadline::UndoStore { input } => {
                self.dispatch_unmarked(&input, super::super::EffectOutcome::Unknown)
            }
            OwnerDeadline::SettingWire(setting) => {
                self.setting_deadline(setting, SettingWait::Wire)
            }
            OwnerDeadline::SettingStore(setting) => {
                self.setting_deadline(setting, SettingWait::Store)
            }
            OwnerDeadline::Enqueue(authorization) => {
                self.revoke(&authorization, RevokeReason::EnqueueExpired)
            }
            OwnerDeadline::CancelDrain(authorization) => {
                self.finish_cancel_drain(authorization, true)
            }
            OwnerDeadline::CancelConfirmation(authorization) => {
                self.expire_cancel_confirmation(&authorization)
            }
            OwnerDeadline::SteerResult(authorization) => self.steer_expired(&authorization),
            OwnerDeadline::Publication(id) => self.publication_gap(id, PublicationGap::Expired),
            OwnerDeadline::Rebase(id) if self.next.ports.rebase == Some(id) => self.offer_rebase(),
            OwnerDeadline::Rebase(_) => Err(OwnerRejection::IgnoredLate),
            OwnerDeadline::Create(operation) => self.expire_create(operation),
        }
    }

    fn next_batch(&mut self) -> BatchId {
        self.next.ports.next_batch += 1;
        let id = BatchId {
            incarnation: self.next.ports.address.incarnation,
            sequence: self.next.ports.next_batch,
        };
        self.next.ports.latest_batch = Some(id);
        self.next.ports.awaiting_apply.insert(id);
        id
    }

    pub(super) fn publish_fact(&mut self, fact: SessionPublication) {
        let mut facts = Vec::new();
        if let SessionPublication::Activated { run, via } = &fact {
            self.next.ports.history = match via {
                RestoreVia::Load => HistoryAvailability::Available,
                RestoreVia::Resume => HistoryAvailability::Unavailable,
            };
            facts.push(match via {
                RestoreVia::Load => PublicationFact::ReplayBoundary(*run),
                RestoreVia::Resume => PublicationFact::HistoryUnavailable(*run),
            });
            let replay = std::mem::take(&mut self.next.ports.replay);
            if *via == RestoreVia::Load {
                facts.extend(
                    replay
                        .into_iter()
                        .map(|event| PublicationFact::Replay { run: *run, event }),
                );
            }
            self.append_setup(run.generation, &mut facts);
        } else if let SessionPublication::Created(run) = &fact {
            self.next.ports.history = HistoryAvailability::Available;
            self.append_setup(*run, &mut facts);
        }
        self.apply_committed_projection(&fact, &mut facts);
        facts.push(PublicationFact::Live(fact));
        self.offer_commit(facts);
    }

    fn append_setup(&mut self, run: crate::SessionGeneration, facts: &mut Vec<PublicationFact>) {
        if let Some(setup) = self.next.ports.setup.take() {
            self.project_restore_setup(&setup.capabilities, &setup.settings);
            facts.push(PublicationFact::Capabilities {
                run,
                report: setup.capabilities,
            });
            facts.push(PublicationFact::Settings {
                run,
                settings: setup.settings,
                observed: self.next.ports.observed.clone(),
                accepted: self
                    .next
                    .ports
                    .settings_catalogue
                    .as_ref()
                    .map(|catalogue| crate::session_owner::AcceptedRunSettings {
                        catalogue: std::sync::Arc::clone(catalogue),
                        router_auto_requested: self.next.ports.router_auto_requested,
                    }),
            });
        }
    }

    pub(super) fn offer_commit(&mut self, facts: Vec<PublicationFact>) {
        let id = self.next_batch();
        self.effects
            .push(OwnerEffect::Publish(PublicationBatch::Commit { id, facts }));
        self.arm(OwnerDeadline::Publication(id), LOCAL_BUDGET);
    }

    pub(super) fn offer_rebase(&mut self) -> OwnerResult {
        self.next.ports.awaiting_apply.clear();
        self.next.ports.rebase = None;
        let id = self.next_batch();
        let history = self.next.ports.history;
        let baseline = Box::new(self.next.snapshot());
        self.effects
            .push(OwnerEffect::Publish(PublicationBatch::Rebase {
                id,
                baseline,
                history,
            }));
        self.arm(OwnerDeadline::Publication(id), LOCAL_BUDGET);
        Ok(())
    }

    pub(super) fn publication_result(&mut self, result: PublicationResult) -> OwnerResult {
        match result {
            PublicationResult::Applied(id) => {
                if !self.next.ports.awaiting_apply.remove(&id) {
                    return Err(OwnerRejection::IgnoredLate);
                }
                if self.next.ports.latest_batch == Some(id) {
                    self.next.ports.publication = PublicationStatus::Current;
                    self.next.ports.rebase = None;
                    self.next.ports.awaiting_apply.clear();
                }
                Ok(())
            }
            PublicationResult::Refused(id) => self.publication_gap(id, PublicationGap::Refused),
            PublicationResult::CapacityReturned if self.next.ports.rebase.is_some() => {
                self.offer_rebase()
            }
            PublicationResult::CapacityReturned => Err(OwnerRejection::IgnoredLate),
        }
    }

    fn publication_gap(&mut self, id: BatchId, reason: PublicationGap) -> OwnerResult {
        if !self.next.ports.awaiting_apply.remove(&id) {
            return Err(OwnerRejection::IgnoredLate);
        }
        if self.next.ports.publication == PublicationStatus::Current {
            self.next.ports.publication = PublicationStatus::Degraded { since: id, reason };
        }
        // A failed batch can leave a gap in retained history.
        self.next.ports.history = HistoryAvailability::Unavailable;
        if self.next.ports.rebase.is_none() {
            self.next.ports.rebase = Some(id);
            self.arm(OwnerDeadline::Rebase(id), LOCAL_BUDGET);
        }
        Ok(())
    }
}
