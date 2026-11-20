use super::{OwnerRejection, OwnerResult, OwnerTransition};
use crate::session_cell_machine::{SessionCellEffect, SessionCellEvent};
use crate::session_lifecycle_machine::{
    CloseAttempt, ClosePhase, LoadAttempt, RestoreVia, SessionLifecycle,
};
use crate::session_owner::OwnerSetting;
use crate::session_owner::{
    AuthorizationStatus, CloseRequest, ObservationCause, ObservedSetting, OwnerEffect,
    PreparationStage, PreparedSetup, RestoreDisposition, RestoreFailure, RestoreModeOutcome,
    RestorePhase, RestorePreparation, RestoreResult, SettingEvidence, TurnMembership,
};
use crate::session_owner::{
    CloseOutcome, EffectOutcome, PendingRestore, RemoteEffect, RestoreFailureStage, RestoreWait,
    RestoredMode, RunAttempt, SessionDeadline, SessionOwnerEffect, SessionOwnerRefusal,
    SessionOwnerReply, SessionPublication, SessionStoreRequest, SessionWireRequest, WireOutcome,
};
use crate::session_setting_machine::{SessionSettingEvent, SettingApplication, SettingValueId};
use crate::session_turn_machine::{SessionTurn, SessionTurnEffect};
use crate::{ProviderSettingKind, SessionGeneration};

impl PendingRestore {
    pub(super) const fn run(self) -> RunAttempt<LoadAttempt> {
        match self {
            Self::Setup { run, .. }
            | Self::Wire { run, .. }
            | Self::Mode { run, .. }
            | Self::FastOff { run, .. }
            | Self::Preparing { run, .. } => run,
        }
    }

    pub(super) const fn wait(self) -> RestoreWait {
        match self {
            Self::Setup { .. } => RestoreWait::Setup,
            Self::Wire { .. } => RestoreWait::Wire,
            Self::Mode { .. } => RestoreWait::Mode,
            Self::FastOff { .. } => RestoreWait::FastOff,
            Self::Preparing { .. } => RestoreWait::Preparation,
        }
    }

    pub(super) const fn remote(self) -> RemoteEffect {
        match self {
            Self::Setup { .. } => RemoteEffect::None,
            Self::Wire { .. } => RemoteEffect::Unknown,
            Self::Mode { .. } | Self::FastOff { .. } | Self::Preparing { .. } => {
                RemoteEffect::Applied
            }
        }
    }
}

impl OwnerTransition {
    pub(super) fn start_restore(&mut self, attempt: LoadAttempt, via: RestoreVia) -> OwnerResult {
        self.start_restore_after_queue_evidence(attempt, via, None)
    }

    pub(super) fn start_restore_after_queue_evidence(
        &mut self,
        attempt: LoadAttempt,
        via: RestoreVia,
        after_queue_evidence: Option<uuid::Uuid>,
    ) -> OwnerResult {
        if !self.next.ports.connection_alive || self.next.ports.create.is_some() {
            return Err(SessionOwnerRefusal::EffectInProgress.into());
        }
        self.cell_move(SessionCellEvent::LoadRequested { attempt, via })?;
        self.next.ports.observed.clear();
        self.next.ports.mode_reason = None;
        self.next.ports.admitted_inputs.clear();
        self.next.ports.replay.clear();
        self.next.ports.retained_items.clear();
        self.next.ports.route_ready = false;
        self.next.ports.fast_reapply = None;
        let Some(generation) = self.next.cell.run_generation() else {
            return Err(OwnerRejection::IgnoredLate);
        };
        let run = RunAttempt {
            generation,
            attempt,
        };
        if after_queue_evidence.is_none() {
            self.release(SessionOwnerEffect::Store(
                SessionStoreRequest::ReadRestoreSetup(run),
            ));
        }
        self.wait_for_restore(PendingRestore::Setup {
            run,
            via,
            after_queue_evidence,
        });
        Ok(())
    }

    pub(super) fn release_cold_restore(
        &mut self,
        input: &super::super::OwnerInput,
        recorded: bool,
    ) -> OwnerResult {
        let Some(PendingRestore::Setup {
            run,
            via,
            after_queue_evidence: Some(operation),
        }) = self.next.restore
        else {
            return Ok(());
        };
        if operation != input.operation || run.generation != input.generation {
            return Ok(());
        }
        if !recorded {
            return self.fail_loading(run, RestoreFailureStage::Setup, RemoteEffect::None);
        }
        self.next.restore = Some(PendingRestore::Setup {
            run,
            via,
            after_queue_evidence: None,
        });
        self.release(SessionOwnerEffect::Store(
            SessionStoreRequest::ReadRestoreSetup(run),
        ));
        Ok(())
    }

    pub(super) fn restore_setup_read(
        &mut self,
        run: RunAttempt<LoadAttempt>,
        outcome: EffectOutcome,
    ) -> OwnerResult {
        let Some(PendingRestore::Setup {
            run: pending,
            via,
            after_queue_evidence: None,
        }) = self.next.restore
        else {
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
        fast_on: Option<SettingApplication>,
    ) -> OwnerResult {
        let Some(PendingRestore::Wire { run: pending, via }) = self.next.restore else {
            return Err(OwnerRejection::IgnoredLate);
        };
        if pending != run {
            return Err(OwnerRejection::IgnoredLate);
        }
        self.next.ports.fast_reapply = fast_on.map(|application| OwnerSetting {
            change: RunAttempt {
                generation: run.generation,
                attempt: application,
            },
            kind: ProviderSettingKind::Fast,
        });
        match outcome {
            WireOutcome::Confirmed(RestoredMode::Observed) => {
                self.after_restore_mode(run, via, true)
            }
            WireOutcome::Confirmed(RestoredMode::NotOffered { current }) => {
                self.record_restore_mode(current, Some(ObservationCause::NotOffered));
                self.after_restore_mode(run, via, false)
            }
            WireOutcome::Confirmed(RestoredMode::Reapply { application, value }) => {
                if self.setting_fenced(ProviderSettingKind::Mode) {
                    self.record_restore_mode(None, Some(ObservationCause::NoAnswer));
                    return self.after_restore_mode(run, via, false);
                }
                let setting = OwnerSetting {
                    change: RunAttempt {
                        generation: run.generation,
                        attempt: application,
                    },
                    kind: ProviderSettingKind::Mode,
                };
                self.next
                    .ports
                    .restore_setting_fences
                    .insert(setting.kind, (run, setting));
                self.release(SessionOwnerEffect::Wire(SessionWireRequest::RestoreMode {
                    run,
                    setting,
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
        setting: OwnerSetting,
        outcome: RestoreModeOutcome,
    ) -> OwnerResult {
        self.release_restore_fence(run, setting, ProviderSettingKind::Mode)?;
        match self.next.restore {
            Some(PendingRestore::Mode { run: pending, via }) if pending == run => {
                let (current, cause) = match outcome {
                    RestoreModeOutcome::Confirmed(value) => (Some(value), None),
                    RestoreModeOutcome::Refused { current } => {
                        (current, Some(ObservationCause::Refused))
                    }
                    RestoreModeOutcome::ReportedOther(value) => {
                        (Some(value), Some(ObservationCause::ReportedOther))
                    }
                    RestoreModeOutcome::FellBack(value) => {
                        (Some(value), Some(ObservationCause::FellBack))
                    }
                    RestoreModeOutcome::Unconfirmed => (None, Some(ObservationCause::Unconfirmed)),
                };
                let confirmed = current.is_some();
                self.record_restore_mode(current, cause);
                self.after_restore_mode(run, via, confirmed)
            }
            _ => Ok(()),
        }
    }

    fn after_restore_mode(
        &mut self,
        run: RunAttempt<LoadAttempt>,
        via: RestoreVia,
        mode_confirmed: bool,
    ) -> OwnerResult {
        if !mode_confirmed {
            self.next
                .ports
                .observed
                .entry(crate::ProviderSettingKind::Mode)
                .or_insert(SettingEvidence {
                    value: ObservedSetting::Unknown,
                    cause: Some(ObservationCause::Unconfirmed),
                });
        }
        if self.next.cell.keep_fast_off
            && let Some(setting) = self.next.ports.fast_reapply.take()
        {
            if self.setting_fenced(ProviderSettingKind::Fast) {
                return self.finish_fast_off(
                    run,
                    setting,
                    via,
                    mode_confirmed,
                    SettingEvidence {
                        value: ObservedSetting::Unknown,
                        cause: Some(ObservationCause::Unconfirmed),
                    },
                );
            }
            self.next
                .ports
                .restore_setting_fences
                .insert(setting.kind, (run, setting));
            self.release(SessionOwnerEffect::Wire(
                SessionWireRequest::RestoreFastOff { run, setting },
            ));
            self.wait_for_restore(PendingRestore::FastOff {
                run,
                setting,
                via,
                mode_confirmed,
            });
        } else {
            self.begin_host_prepare(run, via, mode_confirmed);
        }
        Ok(())
    }

    fn record_restore_mode(
        &mut self,
        current: Option<SettingValueId>,
        cause: Option<ObservationCause>,
    ) {
        if cause == Some(ObservationCause::FellBack) {
            self.next.ports.mode_reason = Some(super::super::PermissionModeReason::AgentFellBack);
        }
        self.next.ports.observed.insert(
            ProviderSettingKind::Mode,
            SettingEvidence {
                value: current.map_or(ObservedSetting::Unknown, ObservedSetting::Confirmed),
                cause,
            },
        );
    }

    fn release_restore_fence(
        &mut self,
        run: RunAttempt<LoadAttempt>,
        setting: OwnerSetting,
        kind: ProviderSettingKind,
    ) -> OwnerResult {
        if self.next.ports.restore_setting_fences.get(&kind) != Some(&(run, setting)) {
            return Err(OwnerRejection::IgnoredLate);
        }
        self.next.ports.restore_setting_fences.remove(&kind);
        Ok(())
    }

    fn finish_fast_off(
        &mut self,
        run: RunAttempt<LoadAttempt>,
        setting: OwnerSetting,
        via: RestoreVia,
        mode_confirmed: bool,
        observation: SettingEvidence,
    ) -> OwnerResult {
        if matches!(observation.value, ObservedSetting::Unknown)
            && matches!(
                observation.cause,
                Some(ObservationCause::NoAnswer | ObservationCause::Unconfirmed)
            )
        {
            let application = setting.change.attempt;
            self.cell_move(SessionCellEvent::Setting {
                kind: ProviderSettingKind::Fast,
                event: SessionSettingEvent::SetRequested {
                    to: SettingValueId("off".to_owned()),
                    application,
                },
            })?;
            self.cell_move(SessionCellEvent::Setting {
                kind: ProviderSettingKind::Fast,
                event: SessionSettingEvent::OutcomeUnknown { application },
            })?;
            self.next
                .ports
                .latest_setting
                .insert(ProviderSettingKind::Fast, setting);
        }
        self.next
            .ports
            .observed
            .insert(ProviderSettingKind::Fast, observation);
        self.begin_host_prepare(run, via, mode_confirmed);
        Ok(())
    }

    fn begin_host_prepare(
        &mut self,
        run: RunAttempt<LoadAttempt>,
        via: RestoreVia,
        mode_confirmed: bool,
    ) {
        self.effects.push(OwnerEffect::PrepareRoute(run));
        self.release(SessionOwnerEffect::PrepareHost(run));
        self.wait_for_restore(PendingRestore::Preparing {
            run,
            via,
            mode_confirmed,
        });
    }

    pub(super) fn restore_preparation(&mut self, progress: RestorePreparation) -> OwnerResult {
        let run = match &progress {
            RestorePreparation::RouteReady(run)
            | RestorePreparation::HostReady { run, .. }
            | RestorePreparation::FastOff { run, .. }
            | RestorePreparation::Failed { run, .. } => *run,
        };
        if let RestorePreparation::FastOff {
            setting,
            observation,
            ..
        } = progress
        {
            self.release_restore_fence(run, setting, ProviderSettingKind::Fast)?;
            if let Some(PendingRestore::FastOff {
                run: pending,
                via,
                mode_confirmed,
                ..
            }) = self.next.restore
                && pending == run
            {
                return self.finish_fast_off(run, setting, via, mode_confirmed, observation);
            }
            if self.is_live(run.generation)
                && let ObservedSetting::Confirmed(value) = observation.value
            {
                self.setting_on_wire(setting, super::super::SettingObservation::Applied(value))?;
            }
            return Ok(());
        }
        let pending = self
            .next
            .restore
            .filter(|pending| pending.run() == run)
            .ok_or(OwnerRejection::IgnoredLate)?;
        match (pending, progress) {
            (
                PendingRestore::Preparing {
                    via,
                    mode_confirmed,
                    ..
                },
                RestorePreparation::HostReady {
                    capabilities,
                    settings,
                    observed,
                    ..
                },
            ) => {
                for (kind, evidence) in observed {
                    self.next.ports.observed.entry(kind).or_insert(evidence);
                }
                for (kind, value) in [
                    (ProviderSettingKind::Mode, settings.mode.as_ref()),
                    (ProviderSettingKind::Model, settings.model.as_ref()),
                    (ProviderSettingKind::Effort, settings.effort.as_ref()),
                ] {
                    if let Some(value) = value {
                        self.next
                            .ports
                            .observed
                            .entry(kind)
                            .or_insert_with(|| SettingEvidence {
                                value: ObservedSetting::Confirmed(SettingValueId(value.clone())),
                                cause: None,
                            });
                    }
                }
                self.next.ports.setup = Some(PreparedSetup {
                    capabilities,
                    settings,
                });
                if self.next.ports.route_ready {
                    self.commit_restore(run, via, mode_confirmed)
                } else {
                    Ok(())
                }
            }
            (
                PendingRestore::Preparing {
                    via,
                    mode_confirmed,
                    ..
                },
                RestorePreparation::RouteReady(_),
            ) => {
                self.next.ports.route_ready = true;
                if self.next.ports.setup.is_some() {
                    self.commit_restore(run, via, mode_confirmed)
                } else {
                    Ok(())
                }
            }
            (
                PendingRestore::Preparing { .. }
                | PendingRestore::Mode { .. }
                | PendingRestore::FastOff { .. },
                RestorePreparation::Failed { stage, .. },
            ) => self.fail_restore(
                run,
                RestoreFailure::LocalPrerequisite { stage },
                RemoteEffect::Applied,
            ),
            _ => Err(OwnerRejection::IgnoredLate),
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
        self.next.restore = None;
        self.release(SessionOwnerEffect::Publish(SessionPublication::Activated {
            run,
            via,
        }));
        let _mode_was_confirmed = mode_confirmed;
        self.settle_restore(RestoreResult {
            attempt: run,
            phase: RestorePhase::Attempted,
            disposition: RestoreDisposition::Restored {
                earlier_unanswered: self.next.cell.last_unanswered_restore.is_some(),
            },
            observed: self.next.ports.observed.clone(),
        });
        if self.next.cell.settings_held() {
            return self
                .retire_waiting_inputs(crate::session_owner::RevokeReason::SettingsUnresolved);
        }
        self.drive_prompt_line()
    }

    fn wait_for_restore(&mut self, pending: PendingRestore) {
        self.next.restore = Some(pending);
        self.release(SessionOwnerEffect::ArmDeadline(SessionDeadline::Restore {
            run: pending.run(),
            wait: pending.wait(),
        }));
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
            PendingRestore::Mode { via, .. } => {
                self.record_restore_mode(None, Some(ObservationCause::NoAnswer));
                self.after_restore_mode(run, via, false)
            }
            PendingRestore::FastOff {
                setting,
                via,
                mode_confirmed,
                ..
            } => self.finish_fast_off(
                run,
                setting,
                via,
                mode_confirmed,
                SettingEvidence {
                    value: ObservedSetting::Unknown,
                    cause: Some(ObservationCause::NoAnswer),
                },
            ),
            PendingRestore::Preparing { .. } => self.fail_restore(
                run,
                RestoreFailure::LocalPrerequisite {
                    stage: PreparationStage::HostSetup,
                },
                RemoteEffect::Applied,
            ),
        }
    }

    pub(super) fn restore_failed_on_wire(
        &mut self,
        run: RunAttempt<LoadAttempt>,
        failure: RestoreFailure,
    ) -> OwnerResult {
        if !matches!(self.next.restore, Some(PendingRestore::Wire { run: pending, .. }) if pending == run)
        {
            return Err(OwnerRejection::IgnoredLate);
        }
        let remote = if failure
            .coded_evidence()
            .is_some_and(|coded| coded.definitely_no_effect())
        {
            RemoteEffect::None
        } else {
            RemoteEffect::Unknown
        };
        self.fail_restore(run, failure, remote)
    }

    pub(super) fn fail_restore(
        &mut self,
        run: RunAttempt<LoadAttempt>,
        failure: RestoreFailure,
        remote: RemoteEffect,
    ) -> OwnerResult {
        let stage = match self.next.restore {
            Some(PendingRestore::Setup { .. }) => RestoreFailureStage::Setup,
            Some(PendingRestore::Wire { .. }) => RestoreFailureStage::Wire,
            _ => RestoreFailureStage::Preparation,
        };
        self.retire_loading(run, stage, remote, failure)
    }

    fn fail_loading(
        &mut self,
        run: RunAttempt<LoadAttempt>,
        stage: RestoreFailureStage,
        remote: RemoteEffect,
    ) -> OwnerResult {
        let failure = match stage {
            RestoreFailureStage::Setup => RestoreFailure::LocalPrerequisite {
                stage: PreparationStage::SavedSetup,
            },
            RestoreFailureStage::Wire if remote == RemoteEffect::Unknown => {
                RestoreFailure::NoAnswer
            }
            RestoreFailureStage::Wire => RestoreFailure::Lost { reported: None },
            RestoreFailureStage::Preparation => RestoreFailure::LocalPrerequisite {
                stage: PreparationStage::HostSetup,
            },
            RestoreFailureStage::Closed => RestoreFailure::Closed,
        };
        self.retire_loading(run, stage, remote, failure)
    }

    fn retire_loading(
        &mut self,
        run: RunAttempt<LoadAttempt>,
        stage: RestoreFailureStage,
        remote: RemoteEffect,
        failure: RestoreFailure,
    ) -> OwnerResult {
        let event = if failure == RestoreFailure::NoAnswer {
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
        self.reply_restore_failed(run, stage, remote, failure);
        self.tag_waiting_inputs(
            run.generation,
            super::super::RevokeReason::RestoreDidNotActivate,
        );
        self.apply_entry_effects(effects)?;
        self.retire_generation(run.generation)?;
        Ok(())
    }

    fn reply_restore_failed(
        &mut self,
        run: RunAttempt<LoadAttempt>,
        stage: RestoreFailureStage,
        remote: RemoteEffect,
        failure: RestoreFailure,
    ) {
        self.settle_restore(RestoreResult {
            attempt: run,
            phase: if stage == RestoreFailureStage::Setup {
                RestorePhase::Preflight
            } else {
                RestorePhase::Attempted
            },
            disposition: RestoreDisposition::Retired { failure, remote },
            observed: self.next.ports.observed.clone(),
        });
        self.next.ports.replay.clear();
        self.next.ports.setup = None;
    }

    pub(super) fn start_close(&mut self, request: CloseRequest) -> OwnerResult {
        let run = request.run;
        if self.next.cell.run_generation() != Some(run.generation) {
            return Err(SessionOwnerRefusal::GenerationChanged.into());
        }
        let effects = self.cell_move(SessionCellEvent::CloseRequested {
            attempt: run.attempt,
        })?;
        if let Some(restore) = self.next.restore.take() {
            self.reply_restore_failed(
                restore.run(),
                RestoreFailureStage::Closed,
                restore.remote(),
                RestoreFailure::Closed,
            );
        }
        self.next.ports.close_support = Some((run, request.provider_supports_close));
        self.tag_waiting_inputs(run.generation, super::super::RevokeReason::QueueClosed);
        self.revoke_run(run.generation);
        self.release(SessionOwnerEffect::ArmDeadline(SessionDeadline::Close(run)));
        for effect in &effects {
            if let SessionCellEffect::Turn(SessionTurnEffect::WriteCancel { turn }) = effect
                && !self.waiting_for_prompt_enqueue(*turn)
            {
                self.issue_cancel(run.generation, *turn)?;
            }
        }
        self.apply_entry_effects(effects)?;
        self.continue_close()
    }

    pub(super) fn continue_close(&mut self) -> OwnerResult {
        if !self.next.ports.connection_alive {
            return Ok(());
        }
        let Some((run, ClosePhase::Freeing)) = self.closing() else {
            return Ok(());
        };
        if self.next.ports.authorizations.iter().any(|(key, pending)| {
            key.run == run.generation && pending.status != AuthorizationStatus::Taken
        }) {
            return Ok(());
        }
        let Some((_, supported)) = self.next.ports.close_support.take() else {
            return Ok(());
        };
        if supported {
            self.release(SessionOwnerEffect::Wire(SessionWireRequest::Close(run)));
            Ok(())
        } else {
            self.complete_close(run, CloseOutcome::RouterOnly)
        }
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
                let outcome = if self.is_live(run.generation) {
                    self.effects.push(OwnerEffect::ReopenRoute(run));
                    CloseOutcome::RefusedLive
                } else {
                    CloseOutcome::RefusedLoading
                };
                self.reply_close_finished(run, outcome);
                self.drive_after_setting()
            }
        }
    }

    pub(super) fn close_deadline(&mut self, run: RunAttempt<CloseAttempt>) -> OwnerResult {
        let Some((pending, phase)) = self.closing() else {
            return Err(OwnerRejection::IgnoredLate);
        };
        if pending != run {
            return Err(OwnerRejection::IgnoredLate);
        }
        let waiting_for_enqueue = self.next.ports.close_support.is_some();
        self.abandon_running_turn()?;
        let outcome = match phase {
            ClosePhase::EndingTurn => CloseOutcome::DrainExpired,
            ClosePhase::Freeing if waiting_for_enqueue => CloseOutcome::NotEnqueued,
            ClosePhase::Freeing => CloseOutcome::OutcomeUnknown,
        };
        self.complete_close(run, outcome)
    }

    pub(super) fn close_not_sent(&mut self, run: RunAttempt<CloseAttempt>) -> OwnerResult {
        if self.closing() != Some((run, ClosePhase::Freeing))
            || self.next.ports.close_support.is_some()
        {
            return Err(OwnerRejection::IgnoredLate);
        }
        self.complete_close(run, CloseOutcome::NotEnqueued)
    }

    pub(super) fn complete_close(
        &mut self,
        run: RunAttempt<CloseAttempt>,
        outcome: CloseOutcome,
    ) -> OwnerResult {
        self.next.ports.close_support = None;
        let effects = self.cell_move(SessionCellEvent::CloseDone {
            attempt: run.attempt,
        })?;
        self.apply_entry_effects(effects)?;
        self.retire_generation(run.generation)?;
        let fact = if matches!(outcome, CloseOutcome::Confirmed | CloseOutcome::RouterOnly) {
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

    pub(super) fn abandon_running_turn(&mut self) -> OwnerResult {
        match self.next.turn() {
            SessionTurn::Idle => Ok(()),
            SessionTurn::Running { turn } | SessionTurn::Cancelling { turn } => {
                self.end_turn_locally(turn)
            }
        }
    }

    pub(super) fn retire_generation(&mut self, generation: SessionGeneration) -> OwnerResult {
        self.retire_previous_prompts(generation);
        self.finish_retired_store_work(generation)?;
        self.retire_queued_settings(generation);
        self.discontinue_setting()?;
        self.retire_run_tools(generation);
        self.effects
            .push(OwnerEffect::CallbackMembership(TurnMembership::Retire(
                generation,
            )));
        self.release(SessionOwnerEffect::RetireRoute(generation));
        Ok(())
    }
}
