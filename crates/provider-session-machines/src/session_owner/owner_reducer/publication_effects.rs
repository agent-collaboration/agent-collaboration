use super::{OwnerRejection, OwnerResult, OwnerTransition};
use crate::session_lifecycle_machine::RestoreVia;
use crate::session_owner::{
    AuthorizationStatus, AuthorizedWork, BatchId, HistoryAvailability, InputWait, ObservedSetting,
    OwnerDeadline, OwnerEffect, PublicationBatch, PublicationFact, PublicationGap,
    PublicationResult, PublicationStatus, QueueEntrySnapshot, RestoreWait, RevokeReason,
    SessionDeadline, SessionOwnerState, SessionPublication, SessionSnapshot, SettingWait,
};
use crate::session_turn_machine::SessionTurn;
use std::time::Duration;

pub(super) const LOCAL_BUDGET: Duration = Duration::from_secs(10);
pub(super) const WIRE_BUDGET: Duration = Duration::from_secs(30);

impl SessionOwnerState {
    /// A projection of committed owner facts for readers outside the mailbox.
    #[must_use]
    pub fn snapshot(&self) -> SessionSnapshot {
        let turn = match self.turn() {
            SessionTurn::Running { turn } | SessionTurn::Cancelling { turn }
                if self.ports.authorizations.values().any(|pending| matches!(&pending.work,
                    AuthorizedWork::Prompt(input) if input.turn == turn && pending.status != AuthorizationStatus::Taken)) => SessionTurn::Idle,
            turn => turn,
        };
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
            settings,
            publication: self.ports.publication,
            history: self.ports.history,
            last_restore: self.ports.last_restore.clone(),
            last_live_turn: self.ports.last_live_turn,
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
                        .map(|item| PublicationFact::Replay { run: *run, item }),
                );
            }
            if let Some(setup) = self.next.ports.setup.take() {
                facts.push(PublicationFact::Capabilities {
                    run: run.generation,
                    report: setup.capabilities,
                });
                facts.push(PublicationFact::Settings {
                    run: run.generation,
                    settings: setup.settings,
                    observed: self.next.ports.observed.clone(),
                });
            }
        }
        facts.push(PublicationFact::Live(fact));
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
