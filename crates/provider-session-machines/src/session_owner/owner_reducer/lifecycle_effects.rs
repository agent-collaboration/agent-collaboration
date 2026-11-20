use super::{OwnerRejection, OwnerResult, OwnerTransition};
use crate::SessionGeneration;
use crate::session_cell_machine::{SessionCellEffect, SessionCellEvent};
use crate::session_lifecycle_machine::{
    CloseAttempt, ClosePhase, LoadAttempt, RestoreVia, SessionLifecycle, SessionLifecycleEffect,
};
use crate::session_owner::{
    CloseOutcome, EffectOutcome, PendingRestore, RemoteEffect, RestoreFailureStage, RestoreWait,
    RestoredMode, RunAttempt, SessionDeadline, SessionOwnerEffect, SessionOwnerRefusal,
    SessionOwnerReply, SessionPublication, SessionStoreRequest, SessionWireRequest, WireOutcome,
};
use crate::session_turn_machine::{SessionTurn, SessionTurnEffect};

impl PendingRestore {
    pub(super) const fn run(self) -> RunAttempt<LoadAttempt> {
        match self {
            Self::Setup { run, .. }
            | Self::Wire { run, .. }
            | Self::Mode { run, .. }
            | Self::Publication { run, .. } => run,
        }
    }

    pub(super) const fn wait(self) -> RestoreWait {
        match self {
            Self::Setup { .. } => RestoreWait::Setup,
            Self::Wire { .. } => RestoreWait::Wire,
            Self::Mode { .. } => RestoreWait::Mode,
            Self::Publication { .. } => RestoreWait::Publication,
        }
    }

    const fn remote(self) -> RemoteEffect {
        match self {
            Self::Setup { .. } => RemoteEffect::None,
            Self::Wire { .. } => RemoteEffect::Unknown,
            Self::Mode { .. } | Self::Publication { .. } => RemoteEffect::Applied,
        }
    }
}

impl OwnerTransition {
    pub(super) fn start_restore(&mut self, attempt: LoadAttempt, via: RestoreVia) -> OwnerResult {
        self.cell_move(SessionCellEvent::LoadRequested { attempt, via })?;
        let Some(generation) = self.next.cell.run_generation() else {
            return Err(OwnerRejection::IgnoredLate);
        };
        let run = RunAttempt {
            generation,
            attempt,
        };
        self.release(SessionOwnerEffect::Store(
            SessionStoreRequest::ReadRestoreSetup(run),
        ));
        self.wait_for_restore(PendingRestore::Setup { run, via });
        Ok(())
    }

    pub(super) fn restore_setup_read(
        &mut self,
        run: RunAttempt<LoadAttempt>,
        outcome: EffectOutcome,
    ) -> OwnerResult {
        let Some(PendingRestore::Setup { run: pending, via }) = self.next.restore else {
            return Err(OwnerRejection::IgnoredLate);
        };
        if pending != run {
            return Err(OwnerRejection::IgnoredLate);
        }
        if outcome == EffectOutcome::Applied {
            self.release(SessionOwnerEffect::Wire(SessionWireRequest::Restore {
                run,
                via,
            }));
            self.wait_for_restore(PendingRestore::Wire { run, via });
            Ok(())
        } else {
            self.fail_loading(run, RestoreFailureStage::Setup, RemoteEffect::None)
        }
    }

    pub(super) fn restore_on_wire(
        &mut self,
        run: RunAttempt<LoadAttempt>,
        outcome: WireOutcome<RestoredMode>,
    ) -> OwnerResult {
        let Some(PendingRestore::Wire { run: pending, via }) = self.next.restore else {
            return Err(OwnerRejection::IgnoredLate);
        };
        if pending != run {
            return Err(OwnerRejection::IgnoredLate);
        }
        match outcome {
            WireOutcome::Confirmed(RestoredMode::Observed) => self.commit_restore(run, via, true),
            WireOutcome::Confirmed(RestoredMode::Reapply(value)) => {
                if self.next.cell.restore_mode_write.is_some() {
                    return self.commit_restore(run, via, false);
                }
                self.cell_move(SessionCellEvent::RestoreModeWriteClaimed {
                    attempt: run.attempt,
                })?;
                self.release(SessionOwnerEffect::Wire(SessionWireRequest::RestoreMode {
                    run,
                    value,
                }));
                self.wait_for_restore(PendingRestore::Mode { run, via });
                Ok(())
            }
            WireOutcome::Refused => {
                self.fail_loading(run, RestoreFailureStage::Wire, RemoteEffect::None)
            }
            WireOutcome::Unknown => {
                self.fail_loading(run, RestoreFailureStage::Wire, RemoteEffect::Unknown)
            }
        }
    }

    pub(super) fn restore_mode_on_wire(
        &mut self,
        run: RunAttempt<LoadAttempt>,
        outcome: WireOutcome<()>,
    ) -> OwnerResult {
        // A late correlated result can release only its original wire claim.
        // It cannot confirm the mode or activate another attempt.
        self.cell_move(SessionCellEvent::RestoreModeWriteSettled {
            attempt: run.attempt,
        })?;
        match self.next.restore {
            Some(PendingRestore::Mode { run: pending, via }) if pending == run => {
                self.commit_restore(run, via, outcome == WireOutcome::Confirmed(()))
            }
            _ => Ok(()),
        }
    }

    fn commit_restore(
        &mut self,
        run: RunAttempt<LoadAttempt>,
        via: RestoreVia,
        mode_confirmed: bool,
    ) -> OwnerResult {
        self.cell_move(SessionCellEvent::Loaded {
            attempt: run.attempt,
        })?;
        self.release(SessionOwnerEffect::Publish(SessionPublication::Activated {
            run,
            via,
        }));
        self.wait_for_restore(PendingRestore::Publication {
            run,
            mode_confirmed,
        });
        self.drive_prompt_line()
    }

    fn wait_for_restore(&mut self, pending: PendingRestore) {
        self.next.restore = Some(pending);
        self.release(SessionOwnerEffect::ArmDeadline(SessionDeadline::Restore {
            run: pending.run(),
            wait: pending.wait(),
        }));
    }

    pub(super) fn activation_published(
        &mut self,
        run: RunAttempt<LoadAttempt>,
        outcome: EffectOutcome,
    ) -> OwnerResult {
        let Some(PendingRestore::Publication {
            run: pending,
            mode_confirmed,
        }) = self.next.restore
        else {
            return Err(OwnerRejection::IgnoredLate);
        };
        if pending != run || !self.is_live(run.generation) {
            return Err(OwnerRejection::IgnoredLate);
        }
        self.next.restore = None;
        if outcome == EffectOutcome::Applied {
            self.release(SessionOwnerEffect::Reply(SessionOwnerReply::Restored {
                run,
                mode_confirmed,
            }));
            return Ok(());
        }
        self.abandon_running_turn()?;
        let effects = self.cell_move(SessionCellEvent::ActorExited {
            generation: run.generation,
        })?;
        self.apply_entry_effects(effects)?;
        self.retire_generation(run.generation)?;
        self.release(SessionOwnerEffect::Publish(SessionPublication::Unloaded(
            run.generation,
        )));
        self.reply_restore_failed(run, RestoreFailureStage::Publication, RemoteEffect::Applied);
        Ok(())
    }

    pub(super) fn restore_deadline(
        &mut self,
        run: RunAttempt<LoadAttempt>,
        wait: RestoreWait,
    ) -> OwnerResult {
        let Some(pending) = self.next.restore else {
            return Err(OwnerRejection::IgnoredLate);
        };
        if pending.run() != run || pending.wait() != wait {
            return Err(OwnerRejection::IgnoredLate);
        }
        match pending {
            PendingRestore::Setup { .. } => {
                self.fail_loading(run, RestoreFailureStage::Setup, RemoteEffect::None)
            }
            PendingRestore::Wire { .. } => {
                self.fail_loading(run, RestoreFailureStage::Wire, RemoteEffect::Unknown)
            }
            PendingRestore::Mode { via, .. } => self.commit_restore(run, via, false),
            PendingRestore::Publication { .. } => {
                self.activation_published(run, EffectOutcome::Unknown)
            }
        }
    }

    fn fail_loading(
        &mut self,
        run: RunAttempt<LoadAttempt>,
        stage: RestoreFailureStage,
        remote: RemoteEffect,
    ) -> OwnerResult {
        let event = if remote == RemoteEffect::Unknown {
            SessionCellEvent::LoadUnanswered {
                attempt: run.attempt,
            }
        } else {
            SessionCellEvent::LoadFailed {
                attempt: run.attempt,
            }
        };
        let effects = self.cell_move(event)?;
        self.next.restore = None;
        self.apply_entry_effects(effects)?;
        self.retire_generation(run.generation)?;
        self.release(SessionOwnerEffect::Publish(SessionPublication::Unloaded(
            run.generation,
        )));
        self.reply_restore_failed(run, stage, remote);
        Ok(())
    }

    fn reply_restore_failed(
        &mut self,
        run: RunAttempt<LoadAttempt>,
        stage: RestoreFailureStage,
        remote: RemoteEffect,
    ) {
        self.release(SessionOwnerEffect::Reply(
            SessionOwnerReply::RestoreFailed { run, stage, remote },
        ));
    }

    pub(super) fn start_close(&mut self, run: RunAttempt<CloseAttempt>) -> OwnerResult {
        if self.next.cell.run_generation() != Some(run.generation) {
            return Err(SessionOwnerRefusal::GenerationChanged.into());
        }
        let effects = self.cell_move(SessionCellEvent::CloseRequested {
            attempt: run.attempt,
        })?;
        if let Some(restore) = self.next.restore.take() {
            self.reply_restore_failed(restore.run(), RestoreFailureStage::Closed, restore.remote());
        }
        self.discontinue_setting()?;
        self.end_pending_steer();
        self.release(SessionOwnerEffect::ArmDeadline(SessionDeadline::Close(run)));
        for effect in &effects {
            match effect {
                SessionCellEffect::Turn(SessionTurnEffect::WriteCancel { turn }) => {
                    self.release(SessionOwnerEffect::Wire(SessionWireRequest::Cancel {
                        generation: run.generation,
                        turn: *turn,
                    }));
                }
                SessionCellEffect::Lifecycle(SessionLifecycleEffect::StartFree { .. }) => {
                    self.release(SessionOwnerEffect::Wire(SessionWireRequest::Close(run)));
                }
                _ => {}
            }
        }
        self.apply_entry_effects(effects)
    }

    pub(super) fn closing(&self) -> Option<(RunAttempt<CloseAttempt>, ClosePhase)> {
        match self.next.lifecycle() {
            SessionLifecycle::Closing {
                generation,
                attempt,
                phase,
                ..
            } => Some((
                RunAttempt {
                    generation,
                    attempt,
                },
                phase,
            )),
            _ => None,
        }
    }

    pub(super) fn close_on_wire(
        &mut self,
        run: RunAttempt<CloseAttempt>,
        outcome: WireOutcome<()>,
    ) -> OwnerResult {
        if self.closing() != Some((run, ClosePhase::Freeing)) {
            return Err(OwnerRejection::IgnoredLate);
        }
        match outcome {
            WireOutcome::Confirmed(()) => self.complete_close(run, CloseOutcome::Confirmed),
            WireOutcome::Unknown => self.complete_close(run, CloseOutcome::OutcomeUnknown),
            WireOutcome::Refused => {
                let effects = self.cell_move(SessionCellEvent::CloseFailed {
                    attempt: run.attempt,
                })?;
                self.apply_entry_effects(effects)?;
                if !self.is_live(run.generation) {
                    self.retire_generation(run.generation)?;
                    self.release(SessionOwnerEffect::Publish(SessionPublication::Unloaded(
                        run.generation,
                    )));
                }
                self.reply_close_finished(run, CloseOutcome::Refused);
                Ok(())
            }
        }
    }

    pub(super) fn close_deadline(&mut self, run: RunAttempt<CloseAttempt>) -> OwnerResult {
        if !self.closing().is_some_and(|(pending, _)| pending == run) {
            return Err(OwnerRejection::IgnoredLate);
        }
        self.abandon_running_turn()?;
        self.complete_close(run, CloseOutcome::OutcomeUnknown)
    }

    fn complete_close(
        &mut self,
        run: RunAttempt<CloseAttempt>,
        outcome: CloseOutcome,
    ) -> OwnerResult {
        let effects = self.cell_move(SessionCellEvent::CloseDone {
            attempt: run.attempt,
        })?;
        self.apply_entry_effects(effects)?;
        self.retire_generation(run.generation)?;
        let fact = if outcome == CloseOutcome::Confirmed {
            SessionPublication::Closed(run.generation)
        } else {
            SessionPublication::Unloaded(run.generation)
        };
        self.release(SessionOwnerEffect::Publish(fact));
        self.reply_close_finished(run, outcome);
        Ok(())
    }

    fn reply_close_finished(&mut self, run: RunAttempt<CloseAttempt>, outcome: CloseOutcome) {
        self.release(SessionOwnerEffect::Reply(
            SessionOwnerReply::CloseFinished { run, outcome },
        ));
    }

    fn abandon_running_turn(&mut self) -> OwnerResult {
        match self.next.turn() {
            SessionTurn::Idle => Ok(()),
            SessionTurn::Running { turn } | SessionTurn::Cancelling { turn } => {
                self.end_turn_locally(turn)
            }
        }
    }

    fn retire_generation(&mut self, generation: SessionGeneration) -> OwnerResult {
        self.finish_retired_store_work(generation)?;
        self.discontinue_setting()?;
        self.end_pending_steer();
        self.retire_run_tools(generation);
        self.release(SessionOwnerEffect::RetireRoute(generation));
        Ok(())
    }
}
