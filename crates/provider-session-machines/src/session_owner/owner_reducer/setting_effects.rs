use super::{OwnerRejection, OwnerResult, OwnerTransition};
use crate::ProviderSettingKind;
use crate::session_cell_machine::SessionCellEvent;
use crate::session_owner::{
    EffectOutcome, OwnerSetting, RemoteEffect, SessionDeadline, SessionOwnerEffect,
    SessionOwnerRefusal, SessionOwnerReply, SessionPublication, SessionStoreRequest,
    SessionWireRequest, SettingObservation, SettingWait,
};
use crate::session_setting_machine::{SessionSettingEvent, SettingValueId};

impl OwnerTransition {
    pub(super) fn start_setting(
        &mut self,
        setting: OwnerSetting,
        value: SettingValueId,
    ) -> OwnerResult {
        self.require_live(setting.change.generation)?;
        if self.next.setting.is_some() || self.prompt_in_progress() {
            return Err(SessionOwnerRefusal::EffectInProgress.into());
        }
        self.apply_setting(
            setting,
            SessionSettingEvent::SetRequested {
                to: value.clone(),
                application: setting.change.attempt,
            },
        )?;
        self.release(SessionOwnerEffect::Wire(SessionWireRequest::Set {
            setting,
            value,
        }));
        self.wait_for_setting(setting, SettingWait::Wire);
        Ok(())
    }

    pub(super) fn setting_on_wire(
        &mut self,
        setting: OwnerSetting,
        observation: SettingObservation,
    ) -> OwnerResult {
        if self.next.setting != Some((setting, SettingWait::Wire))
            || !self.is_live(setting.change.generation)
        {
            return Err(OwnerRejection::IgnoredLate);
        }
        let application = setting.change.attempt;
        match observation {
            SettingObservation::Applied(value) => {
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
        match wait {
            SettingWait::Wire => self.setting_on_wire(setting, SettingObservation::Unknown),
            SettingWait::Store => self.setting_recorded(setting, EffectOutcome::Unknown),
        }
    }

    pub(super) fn discontinue_setting(&mut self) -> OwnerResult {
        if let Some((setting, wait)) = self.next.setting {
            let remote = match wait {
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
