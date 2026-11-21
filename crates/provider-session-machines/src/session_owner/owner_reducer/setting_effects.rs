use super::{OwnerRejection, OwnerResult, OwnerTransition};
use crate::session_cell_machine::SessionCellEvent;
use crate::session_lifecycle_machine::{LoadAttempt, SessionLifecycle};
use crate::session_owner::{
    AuthorizationStatus, AuthorizedWork, EffectInProgress, EffectOutcome, FastReadback,
    ModelFastOutcome, ObservationCause, ObservedSetting, OwnerEffect, OwnerSetting, QueuedSetting,
    RemoteEffect, RevokeReason, RunAttempt, SessionDeadline, SessionOwnerEffect,
    SessionOwnerRefusal, SessionOwnerReply, SessionPublication, SessionStoreRequest,
    SettingEffectInProgress, SettingEvidence, SettingObservation, SettingReadback, SettingWait,
};
use crate::session_setting_machine::{SessionSettingEvent, SettingValueId};
use crate::{ProviderSettingKind, ProviderSettingsCatalog};
use session_event_model::InputId;
use std::sync::Arc;

impl OwnerTransition {
    pub(super) fn admit_setting(&mut self, request: QueuedSetting) -> OwnerResult {
        self.require_live(request.setting.change.generation)?;
        if self.next.setting.is_some() {
            self.effects.push(OwnerEffect::SettingQueued {
                input: request.input.clone(),
                setting: request.setting,
            });
            self.next.ports.queued_settings.push_back(request);
            return Ok(());
        }
        self.start_queued_setting(request)
    }

    fn start_queued_setting(&mut self, request: QueuedSetting) -> OwnerResult {
        let QueuedSetting {
            input,
            setting,
            value,
            expected_model,
            context,
        } = request;
        if self.next.cell.settings_held() && context.is_some() {
            return Err(SessionOwnerRefusal::SettingsUnresolved.into());
        }
        if expected_model.is_some_and(|expected| {
            !self
                .next
                .ports
                .observed
                .get(&ProviderSettingKind::Model)
                .is_some_and(|evidence| evidence.value == ObservedSetting::Confirmed(expected))
        }) {
            return Err(SessionOwnerRefusal::SettingContextChanged.into());
        }
        if context.is_some_and(|context| context.reuse_confirmed)
            && !self.setting_fenced(setting.kind)
            && self.next.ports.observed.get(&setting.kind).is_some_and(|evidence| {
                matches!(&evidence.value, ObservedSetting::Confirmed(current) if current == &value)
            })
        {
            self.release(SessionOwnerEffect::Reply(SessionOwnerReply::SettingFinished {
                setting, remote: RemoteEffect::Applied, saved: false,
            }));
            return Ok(());
        }
        self.start_setting(input, setting, value)
    }

    pub(super) fn start_setting(
        &mut self,
        input_id: InputId,
        setting: OwnerSetting,
        value: SettingValueId,
    ) -> OwnerResult {
        self.require_live(setting.change.generation)?;
        if setting.kind == ProviderSettingKind::Fast && value.0 == "auto" {
            self.cell_move(SessionCellEvent::KeepFastOff(false))?;
            self.release(SessionOwnerEffect::Reply(
                SessionOwnerReply::SettingFinished {
                    setting,
                    remote: RemoteEffect::None,
                    saved: false,
                },
            ));
            return Ok(());
        }
        // Admission queues an incoming setting behind one already in flight.
        let blocked = if let Some((in_flight, _)) = self.next.setting {
            Some(SettingEffectInProgress::SettingInFlight(
                self.next.cell.settings.get(&in_flight.kind).cloned(),
            ))
        } else if self.prompt_in_progress() {
            Some(SettingEffectInProgress::PromptInProgress)
        } else if self.setting_fenced(setting.kind) {
            Some(SettingEffectInProgress::FencedSetting(
                self.next.cell.settings.get(&setting.kind).cloned(),
            ))
        } else {
            None
        };
        if let Some(blocked) = blocked {
            return Err(SessionOwnerRefusal::SettingEffectInProgress(blocked).into());
        }
        if setting.kind == ProviderSettingKind::Fast && value.0 == "off" {
            self.cell_move(SessionCellEvent::KeepFastOff(true))?;
        }
        self.apply_setting(
            setting,
            SessionSettingEvent::SetRequested {
                to: value.clone(),
                application: setting.change.attempt,
            },
        )?;
        self.next.ports.latest_setting.insert(setting.kind, setting);
        self.next.setting = Some((setting, SettingWait::Enqueue));
        self.issue_authorization(AuthorizedWork::Setting {
            input_id,
            setting,
            value,
        })
    }

    pub(super) fn setting_fenced(&self, kind: ProviderSettingKind) -> bool {
        self.next.ports.restore_setting_fences.contains_key(&kind)
            || self.next.ports.authorizations.values().any(|pending| matches!(&pending.work,
            AuthorizedWork::Setting { setting, .. } if setting.kind == kind && pending.status == AuthorizationStatus::Taken))
    }

    pub(super) fn setting_taken(&mut self, setting: OwnerSetting) {
        if let Some(evidence) = self.next.ports.observed.get_mut(&setting.kind)
            && let ObservedSetting::Confirmed(value) = &evidence.value
        {
            evidence.value = ObservedSetting::LastConfirmed(value.clone());
        }
        self.wait_for_setting(setting, SettingWait::Wire);
    }

    pub(super) fn cancel_setting(&mut self, input: &InputId, setting: OwnerSetting) -> OwnerResult {
        if let Some(position) = self
            .next
            .ports
            .queued_settings
            .iter()
            .position(|request| request.setting == setting && &request.input == input)
        {
            self.next.ports.queued_settings.remove(position);
            self.reply_queued_setting_unsent(setting);
            return Ok(());
        }
        if self.next.setting != Some((setting, SettingWait::Enqueue))
            || !self.revoke_input(setting.change.generation, input, RevokeReason::Cancelled)
        {
            // The owner uses this guard when a setting waiter withdraws.
            return Err(SessionOwnerRefusal::EffectInProgress(
                EffectInProgress::SettingCancelTooLate,
            )
            .into());
        }
        Ok(())
    }

    pub(super) fn setting_unsent(&mut self, setting: OwnerSetting) -> OwnerResult {
        if self.next.setting == Some((setting, SettingWait::Enqueue)) {
            if self.next.cell.settings.contains_key(&setting.kind) {
                self.apply_setting(
                    setting,
                    SessionSettingEvent::Refused {
                        application: setting.change.attempt,
                    },
                )?;
            }
            self.reply_setting(setting, RemoteEffect::None, false);
        }
        self.drive_after_setting()
    }

    pub(super) fn setting_kept(
        &mut self,
        setting: OwnerSetting,
        reason: Option<super::super::PermissionModeReason>,
    ) -> OwnerResult {
        if self.next.setting != Some((setting, SettingWait::Enqueue)) {
            return Err(OwnerRejection::IgnoredLate);
        }
        self.apply_setting(
            setting,
            SessionSettingEvent::Refused {
                application: setting.change.attempt,
            },
        )?;
        self.next.ports.mode_reason = reason;
        self.next.setting = None;
        self.release(SessionOwnerEffect::Reply(SessionOwnerReply::SettingKept {
            setting,
        }));
        self.drive_after_setting()
    }

    pub(super) fn observe_late_restore_setting(
        &mut self,
        run: RunAttempt<LoadAttempt>,
        setting: OwnerSetting,
        value: SettingValueId,
        catalogue: Option<Arc<ProviderSettingsCatalog>>,
    ) -> OwnerResult {
        if !self.is_live(run.generation)
            || self.next.ports.latest_setting.get(&setting.kind) != Some(&setting)
        {
            return Ok(());
        }
        self.setting_on_wire(setting, SettingObservation::Applied(value), None)?;
        if let Some(catalogue) = catalogue {
            self.publish_late_setting_readback(setting, &catalogue);
        }
        Ok(())
    }

    fn publish_late_setting_readback(
        &mut self,
        setting: OwnerSetting,
        readback: &ProviderSettingsCatalog,
    ) {
        if let Some(current) = &mut self.next.ports.settings_catalogue {
            Arc::make_mut(current).apply_late_setting_readback(setting.kind, readback);
            let catalogue = Arc::clone(current);
            self.publish_accepted_catalogue(setting.change.generation, catalogue);
        }
    }

    pub(super) fn setting_on_wire(
        &mut self,
        setting: OwnerSetting,
        observation: SettingObservation,
        mode_request: Option<bool>,
    ) -> OwnerResult {
        let current_run = matches!(self.next.lifecycle(), SessionLifecycle::Live { generation } | SessionLifecycle::Closing { generation, .. } if generation == setting.change.generation);
        if self.next.setting != Some((setting, SettingWait::Wire)) || !current_run {
            let (reported, catalogue, confirmed) = match observation {
                SettingObservation::Applied(value) => (Some(value), None, true),
                SettingObservation::Refused { current, catalogue } => (current, catalogue, false),
                SettingObservation::AppliedWithReadback(readback) => {
                    (Some(readback.value), Some(readback.catalogue), true)
                }
                SettingObservation::Unknown => (None, None, false),
            };
            if current_run
                && self.next.ports.latest_setting.get(&setting.kind) == Some(&setting)
                && let Some(value) = reported
            {
                self.apply_setting(
                    setting,
                    SessionSettingEvent::Observed {
                        value: value.clone(),
                    },
                )?;
                self.next.ports.observed.insert(
                    setting.kind,
                    SettingEvidence {
                        value: ObservedSetting::LastConfirmed(value),
                        cause: None,
                    },
                );
                if confirmed && let Some(requested) = mode_request {
                    self.next.ports.router_auto_requested = requested;
                }
                if let Some(catalogue) = catalogue {
                    self.publish_late_setting_readback(setting, &catalogue);
                }
            }
            return Ok(());
        }
        let application = setting.change.attempt;
        match observation {
            SettingObservation::Applied(value) => {
                self.next.ports.observed.insert(
                    setting.kind,
                    SettingEvidence {
                        value: ObservedSetting::Confirmed(value.clone()),
                        cause: None,
                    },
                );
                self.apply_setting(
                    setting,
                    SessionSettingEvent::Applied {
                        application,
                        value: value.clone(),
                    },
                )?;
                self.release(SessionOwnerEffect::Publish(
                    SessionPublication::SettingObserved {
                        setting,
                        value: value.clone(),
                    },
                ));
                self.release(SessionOwnerEffect::Store(
                    SessionStoreRequest::RecordConfirmedSetting { setting, value },
                ));
                self.wait_for_setting(setting, SettingWait::Store);
            }
            SettingObservation::AppliedWithReadback(readback) => {
                self.apply_confirmed_readback(setting, *readback, mode_request)?;
            }
            SettingObservation::Refused { current, catalogue } => {
                let confirmed_current =
                    current.is_some() && setting.kind != ProviderSettingKind::Fast;
                if let Some(value) = current {
                    self.next.ports.observed.insert(
                        setting.kind,
                        SettingEvidence {
                            value: ObservedSetting::Confirmed(value.clone()),
                            cause: Some(ObservationCause::Refused),
                        },
                    );
                    self.apply_setting(
                        setting,
                        SessionSettingEvent::Observed {
                            value: value.clone(),
                        },
                    )?;
                    self.release(SessionOwnerEffect::Publish(
                        SessionPublication::SettingObserved { setting, value },
                    ));
                }
                let event = if setting.kind == ProviderSettingKind::Fast || confirmed_current {
                    SessionSettingEvent::Failed {
                        application,
                        confirmed_current,
                    }
                } else {
                    SessionSettingEvent::Refused { application }
                };
                self.apply_setting(setting, event)?;
                if let Some(catalogue) = catalogue {
                    self.publish_late_setting_readback(setting, &catalogue);
                }
                self.reply_setting(setting, RemoteEffect::None, false);
            }
            SettingObservation::Unknown => {
                let evidence =
                    self.next
                        .ports
                        .observed
                        .entry(setting.kind)
                        .or_insert(SettingEvidence {
                            value: ObservedSetting::Unknown,
                            cause: Some(ObservationCause::Unconfirmed),
                        });
                if let ObservedSetting::Confirmed(value) = &evidence.value {
                    evidence.value = ObservedSetting::LastConfirmed(value.clone());
                }
                evidence.cause = Some(ObservationCause::Unconfirmed);
                self.apply_setting(setting, SessionSettingEvent::OutcomeUnknown { application })?;
                self.reply_setting(setting, RemoteEffect::Unknown, false);
            }
        }
        if self.next.cell.settings_held() {
            self.retire_waiting_inputs(RevokeReason::SettingsUnresolved)?;
        }
        self.drive_after_setting()
    }

    fn apply_confirmed_readback(
        &mut self,
        setting: OwnerSetting,
        readback: SettingReadback,
        mode_request: Option<bool>,
    ) -> OwnerResult {
        let SettingReadback {
            value,
            catalogue,
            fast,
            mode_reason,
        } = readback;
        self.apply_setting(
            setting,
            SessionSettingEvent::Applied {
                application: setting.change.attempt,
                value: value.clone(),
            },
        )?;
        self.observe_catalogue_values(&catalogue.to_session_settings())?;
        self.next.ports.observed.insert(
            setting.kind,
            SettingEvidence {
                value: ObservedSetting::Confirmed(value.clone()),
                cause: None,
            },
        );
        self.observe_companion_fast(&fast);
        if let Some(requested) = mode_request {
            self.next.ports.router_auto_requested = requested;
        }
        self.publish_accepted_catalogue(setting.change.generation, catalogue);
        if setting.kind == ProviderSettingKind::Mode {
            self.next.ports.mode_reason = mode_reason;
            self.release(SessionOwnerEffect::Store(
                SessionStoreRequest::RecordConfirmedSetting { setting, value },
            ));
            self.wait_for_setting(setting, SettingWait::Store);
        } else if setting.kind == ProviderSettingKind::Model
            && self.next.cell.keep_fast_off
            && self.is_live(setting.change.generation)
        {
            self.finish_model_with_fast(setting, fast)?;
        } else {
            self.reply_setting(setting, RemoteEffect::Applied, false);
        }
        Ok(())
    }

    fn observe_companion_fast(&mut self, fast: &FastReadback) {
        if self.setting_fenced(ProviderSettingKind::Fast)
            || self
                .next
                .cell
                .settings
                .get(&ProviderSettingKind::Fast)
                .is_some_and(crate::session_setting_machine::SessionSetting::outcome_unknown)
        {
            return;
        }
        let value = match fast {
            FastReadback::NotOffered => {
                self.next.ports.observed.remove(&ProviderSettingKind::Fast);
                return;
            }
            FastReadback::Off => "off",
            FastReadback::On { .. } => "on",
        };
        self.next.ports.observed.insert(
            ProviderSettingKind::Fast,
            SettingEvidence {
                value: ObservedSetting::Confirmed(SettingValueId(value.to_owned())),
                cause: None,
            },
        );
    }

    fn finish_model_with_fast(&mut self, model: OwnerSetting, fast: FastReadback) -> OwnerResult {
        match fast {
            FastReadback::NotOffered => self.reply_model_fast(model, ModelFastOutcome::NotOffered),
            FastReadback::Off => self.reply_model_fast(model, ModelFastOutcome::ConfirmedOff),
            FastReadback::On { application, input } => {
                if self.setting_fenced(ProviderSettingKind::Fast) {
                    self.reply_model_fast(model, ModelFastOutcome::Unconfirmed);
                } else {
                    let setting = OwnerSetting {
                        change: crate::session_owner::RunAttempt {
                            generation: model.change.generation,
                            attempt: application,
                        },
                        kind: ProviderSettingKind::Fast,
                    };
                    self.next.setting = None;
                    self.next.ports.model_fast_after = Some((model, setting));
                    self.start_setting(input, setting, SettingValueId("off".to_owned()))?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn setting_recorded(
        &mut self,
        setting: OwnerSetting,
        outcome: EffectOutcome,
    ) -> OwnerResult {
        if self.next.setting != Some((setting, SettingWait::Store)) {
            return Err(OwnerRejection::IgnoredLate);
        }
        self.reply_setting(
            setting,
            RemoteEffect::Applied,
            outcome == EffectOutcome::Applied,
        );
        self.drive_after_setting()
    }

    pub(super) fn setting_deadline(
        &mut self,
        setting: OwnerSetting,
        wait: SettingWait,
    ) -> OwnerResult {
        if self.next.setting != Some((setting, wait)) {
            return Err(OwnerRejection::IgnoredLate);
        }
        match wait {
            SettingWait::Enqueue => Err(OwnerRejection::IgnoredLate),
            SettingWait::Wire => self.setting_on_wire(setting, SettingObservation::Unknown, None),
            SettingWait::Store => self.setting_recorded(setting, EffectOutcome::Unknown),
        }
    }

    pub(super) fn discontinue_setting(&mut self) -> OwnerResult {
        if let Some((setting, wait)) = self.next.setting {
            let remote = match wait {
                SettingWait::Enqueue => return Ok(()),
                SettingWait::Wire => {
                    // A refused close may leave this same run usable. Resolve
                    // its in-flight write conservatively, even in Closing.
                    if self.next.cell.settings.contains_key(&setting.kind) {
                        self.apply_setting(
                            setting,
                            SessionSettingEvent::OutcomeUnknown {
                                application: setting.change.attempt,
                            },
                        )?;
                    }
                    RemoteEffect::Unknown
                }
                SettingWait::Store => RemoteEffect::Applied,
            };
            self.reply_setting(setting, remote, false);
        }
        Ok(())
    }

    pub(super) fn retire_queued_settings(&mut self, generation: crate::SessionGeneration) {
        let mut queued = std::mem::take(&mut self.next.ports.queued_settings);
        queued.retain(|request| {
            let retired = request.setting.change.generation == generation;
            if retired {
                self.reply_queued_setting_unsent(request.setting);
            }
            !retired
        });
        self.next.ports.queued_settings = queued;
    }

    pub(super) fn drive_after_setting(&mut self) -> OwnerResult {
        let SessionLifecycle::Live { generation } = self.next.lifecycle() else {
            return Ok(());
        };
        while self.next.setting.is_none()
            && let Some(request) = self.next.ports.queued_settings.pop_front()
        {
            if request.setting.change.generation != generation
                || self.setting_fenced(request.setting.kind)
                || self.prompt_in_progress()
            {
                self.reply_queued_setting_unsent(request.setting);
            } else {
                let setting = request.setting;
                match self.start_queued_setting(request) {
                    Ok(()) => {}
                    Err(OwnerRejection::Refused(refusal)) => {
                        self.release(SessionOwnerEffect::Reply(
                            SessionOwnerReply::SettingNotSent { setting, refusal },
                        ))
                    }
                    Err(error @ OwnerRejection::IgnoredLate) => return Err(error),
                }
            }
        }
        self.drive_prompt_line()
    }

    fn reply_queued_setting_unsent(&mut self, setting: OwnerSetting) {
        self.release(SessionOwnerEffect::Reply(
            SessionOwnerReply::SettingFinished {
                setting,
                remote: RemoteEffect::None,
                saved: false,
            },
        ));
    }

    fn apply_setting(&mut self, setting: OwnerSetting, event: SessionSettingEvent) -> OwnerResult {
        self.cell_move(SessionCellEvent::Setting {
            kind: setting.kind,
            event,
        })?;
        Ok(())
    }

    fn wait_for_setting(&mut self, setting: OwnerSetting, wait: SettingWait) {
        self.next.setting = Some((setting, wait));
        self.release(SessionOwnerEffect::ArmDeadline(SessionDeadline::Setting {
            setting,
            wait,
        }));
    }

    fn reply_setting(&mut self, setting: OwnerSetting, remote: RemoteEffect, saved: bool) {
        if let Some((model, _)) = self
            .next
            .ports
            .model_fast_after
            .take_if(|(_, fast)| *fast == setting)
        {
            let fast = match remote {
                RemoteEffect::Applied => ModelFastOutcome::ConfirmedOff,
                RemoteEffect::None => ModelFastOutcome::Refused,
                RemoteEffect::Unknown => ModelFastOutcome::Unconfirmed,
            };
            self.reply_model_fast(model, fast);
            return;
        }
        self.next.setting = None;
        self.release(SessionOwnerEffect::Reply(
            SessionOwnerReply::SettingFinished {
                setting,
                remote,
                saved,
            },
        ));
    }

    fn reply_model_fast(&mut self, setting: OwnerSetting, fast: ModelFastOutcome) {
        self.next.setting = None;
        self.release(SessionOwnerEffect::Reply(
            SessionOwnerReply::ModelFastFinished { setting, fast },
        ));
    }
}
