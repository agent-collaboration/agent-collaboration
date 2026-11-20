use super::{OwnerRejection, OwnerResult, OwnerTransition};
use crate::ProviderSettingKind;
use crate::session_cell_machine::SessionCellEvent;
use crate::session_lifecycle_machine::SessionLifecycle;
use crate::session_owner::{
    AuthorizationStatus, AuthorizedWork, EffectOutcome, ObservationCause, ObservedSetting,
    OwnerSetting, RemoteEffect, RevokeReason, SessionDeadline, SessionOwnerEffect,
    SessionOwnerRefusal, SessionOwnerReply, SessionPublication, SessionStoreRequest,
    SettingEvidence, SettingObservation, SettingWait,
};
use crate::session_setting_machine::{SessionSettingEvent, SettingValueId};
use session_event_model::InputId;

impl OwnerTransition {
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
        if self.next.setting.is_some()
            || self.prompt_in_progress()
            || self.setting_fenced(setting.kind)
        {
            return Err(SessionOwnerRefusal::EffectInProgress.into());
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
        if self.next.setting != Some((setting, SettingWait::Enqueue))
            || !self.revoke_input(setting.change.generation, input, RevokeReason::Cancelled)
        {
            return Err(SessionOwnerRefusal::EffectInProgress.into());
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
        self.drive_prompt_line()
    }

    pub(super) fn setting_on_wire(
        &mut self,
        setting: OwnerSetting,
        observation: SettingObservation,
    ) -> OwnerResult {
        let current_run = matches!(self.next.lifecycle(), SessionLifecycle::Live { generation } | SessionLifecycle::Closing { generation, .. } if generation == setting.change.generation);
        if self.next.setting != Some((setting, SettingWait::Wire)) || !current_run {
            if current_run
                && self.next.ports.latest_setting.get(&setting.kind) == Some(&setting)
                && let SettingObservation::Applied(value)
                | SettingObservation::Refused {
                    current: Some(value),
                } = observation
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
            SettingObservation::Refused { current } => {
                let confirmed_current = current.is_some();
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
        self.drive_prompt_line()
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
        self.drive_prompt_line()
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
            SettingWait::Wire => self.setting_on_wire(setting, SettingObservation::Unknown),
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
        self.next.setting = None;
        self.release(SessionOwnerEffect::Reply(
            SessionOwnerReply::SettingFinished {
                setting,
                remote,
                saved,
            },
        ));
    }
}
