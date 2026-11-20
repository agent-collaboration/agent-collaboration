//! Translate owner-authorized settings to exact ACP values and readback.

use crate::owner_wire::OwnerWireRefusal;
use crate::{
    FastObserved, PermissionModePlan, ProviderConfigValue, ProviderSettingsCatalog,
    RequestedPermissionMode, owner_session_open::OwnerOpenedSession, plan_permission_mode,
};
use agent_client_protocol::{
    Error, JsonRpcMessage as _, UntypedMessage,
    schema::v1::{
        SessionConfigOptionValue, SetSessionConfigOptionRequest, SetSessionConfigOptionResponse,
        SetSessionModeRequest,
    },
};
use provider_session_machines::{
    ProviderSettingKind,
    session_lifecycle_machine::LoadAttempt,
    session_owner::{
        DispatchAuthorization, DispatchResult, FastReadback, ObservationCause, ObservedSetting,
        OwnerEvent, OwnerSetting, RestoreModeOutcome, RestorePreparation, RestoredMode, RunAttempt,
        SessionWireResult, SettingEvidence, SettingObservation, SettingReadback,
    },
    session_setting_machine::{SettingApplication, SettingValueId},
};
use std::sync::{Arc, Mutex, PoisonError};
use uuid::Uuid;

pub struct RestoreSettingPlan {
    pub mode: RestoredMode,
    pub accepted_mode_fallback: Option<String>,
    pub fast_off: Option<SettingApplication>,
}

impl OwnerOpenedSession {
    #[must_use]
    pub fn restore_setting_plan(&mut self, requested_mode: Option<&str>) -> RestoreSettingPlan {
        let current = self.catalog.effective_settings().mode;
        self.refresh_restored_allowance();
        let (mode, accepted_mode_fallback) = if let Some(requested) = requested_mode {
            match plan_permission_mode(
                RequestedPermissionMode::from_request(Some(requested)),
                &self.catalog.advertised_values(ProviderSettingKind::Mode),
                self.behavior.auto_permission_mode(),
            ) {
                Ok(PermissionModePlan::Set {
                    mode,
                    accepted_fallback,
                    ..
                }) if current.as_deref() != Some(mode.as_str())
                    && !accepted_fallback
                        .as_ref()
                        .is_some_and(|fallback| current.as_ref() == Some(fallback)) =>
                {
                    (
                        RestoredMode::Reapply {
                            application: SettingApplication(Uuid::now_v7()),
                            value: SettingValueId(mode),
                        },
                        accepted_fallback,
                    )
                }
                Ok(PermissionModePlan::Set { .. }) => (RestoredMode::Observed, None),
                Ok(PermissionModePlan::Keep { .. }) | Err(_) => (
                    RestoredMode::NotOffered {
                        current: current.map(SettingValueId),
                    },
                    None,
                ),
            }
        } else {
            (RestoredMode::Observed, None)
        };
        RestoreSettingPlan {
            mode,
            accepted_mode_fallback,
            fast_off: (self.catalog.fast_observed() == FastObserved::On)
                .then(|| SettingApplication(Uuid::now_v7())),
        }
    }

    pub fn apply_restore_setting_readback(&mut self, readback: RestoreSettingReadback) {
        let catalog = readback
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(catalog) = catalog {
            self.catalog = catalog;
            self.refresh_restored_allowance();
        }
    }

    fn refresh_restored_allowance(&mut self) {
        // Evaluate the adapter's Auto rule against the reported mode. The
        // saved folders still determine whether this run has a scope at all.
        self.router_auto_active = crate::provider_setting_application::current_mode_grants_auto(
            Some("auto"),
            &self.catalog,
            self.behavior.as_ref(),
        );
    }
}

/// The full catalogue readback for one restore setter. Its owner drops this
/// receipt when advancing past the write, so a late reply cannot update a run.
pub struct RestoreSettingReadback(Arc<Mutex<Option<ProviderSettingsCatalog>>>);

pub struct OwnerSettingWirePlan {
    pub(crate) message: UntypedMessage,
    response: SettingResponseShape,
}

struct SettingResponseShape {
    catalog: ProviderSettingsCatalog,
    kind: ProviderSettingKind,
    sent: String,
    accepted_fallback: Option<String>,
    current: Option<SettingValueId>,
    reads_value: bool,
    mode_confirmation: Option<ModeConfirmation>,
    restore_readback: Option<RestoreSettingReadback>,
}

#[derive(Clone, Copy)]
pub(crate) struct ModeConfirmation {
    pub(crate) router_auto: bool,
    pub(crate) reason: Option<crate::PermissionModeReason>,
}

pub(crate) enum SettingCorrelation {
    Explicit(DispatchAuthorization),
    RestoreMode {
        run: RunAttempt<LoadAttempt>,
        setting: OwnerSetting,
    },
    RestoreFastOff {
        run: RunAttempt<LoadAttempt>,
        setting: OwnerSetting,
    },
}

pub(crate) struct CorrelatedSetting {
    shape: SettingResponseShape,
    correlation: SettingCorrelation,
}

impl OwnerSettingWirePlan {
    pub fn from_catalog(
        session: String,
        catalog: &ProviderSettingsCatalog,
        kind: ProviderSettingKind,
        wanted: String,
        accepted_fallback: Option<String>,
    ) -> Result<Self, OwnerWireRefusal> {
        if catalog.unavailable_selection(kind, &wanted).is_some() {
            return Err(OwnerWireRefusal::Encoding);
        }
        let reads_value = catalog.config_option(kind).is_some();
        let message = if let Some(option) = catalog.config_option(kind) {
            let value = match option.current_value {
                ProviderConfigValue::Boolean(_) if kind == ProviderSettingKind::Fast => {
                    SessionConfigOptionValue::boolean(false)
                }
                ProviderConfigValue::Select(_) if kind == ProviderSettingKind::Fast => {
                    let off = option
                        .choices
                        .iter()
                        .find(|choice| {
                            crate::provider_session_settings::is_fast_off_select_value(
                                &choice.value,
                            )
                        })
                        .ok_or(OwnerWireRefusal::Encoding)?;
                    SessionConfigOptionValue::value_id(off.value.clone())
                }
                ProviderConfigValue::Select(_) => {
                    SessionConfigOptionValue::value_id(wanted.clone())
                }
                ProviderConfigValue::Boolean(_) => return Err(OwnerWireRefusal::Encoding),
            };
            SetSessionConfigOptionRequest::new(session, option.id.clone(), value)
                .to_untyped_message()
        } else if kind == ProviderSettingKind::Mode && !catalog.modes.is_empty() {
            SetSessionModeRequest::new(session, wanted.clone()).to_untyped_message()
        } else {
            return Err(OwnerWireRefusal::Encoding);
        }
        .map_err(|_| OwnerWireRefusal::Encoding)?;
        Ok(Self {
            message,
            response: SettingResponseShape {
                catalog: catalog.clone(),
                kind,
                sent: wanted,
                accepted_fallback,
                current: current_value(catalog, kind),
                reads_value,
                mode_confirmation: None,
                restore_readback: None,
            },
        })
    }

    pub(crate) fn with_mode_confirmation(mut self, mode: Option<ModeConfirmation>) -> Self {
        self.response.mode_confirmation = mode;
        self
    }

    #[must_use]
    pub fn retain_restore_readback(mut self) -> (Self, RestoreSettingReadback) {
        let slot = Arc::new(Mutex::new(None));
        self.response.restore_readback = Some(RestoreSettingReadback(Arc::clone(&slot)));
        (self, RestoreSettingReadback(slot))
    }

    pub(crate) fn correlate(
        self,
        correlation: SettingCorrelation,
    ) -> (UntypedMessage, CorrelatedSetting) {
        (
            self.message,
            CorrelatedSetting {
                shape: self.response,
                correlation,
            },
        )
    }
}

impl CorrelatedSetting {
    pub(crate) fn mode_confirmation(&self) -> Option<ModeConfirmation> {
        self.shape.mode_confirmation
    }

    pub(crate) fn authorization(&self) -> Option<&DispatchAuthorization> {
        match &self.correlation {
            SettingCorrelation::Explicit(authorization) => Some(authorization),
            SettingCorrelation::RestoreMode { .. } | SettingCorrelation::RestoreFastOff { .. } => {
                None
            }
        }
    }

    pub(crate) fn settle(
        self,
        reply: Option<Result<serde_json::Value, Error>>,
        behavior: &dyn crate::ProviderBehavior,
    ) -> (OwnerEvent, Option<ProviderSettingsCatalog>) {
        let Self {
            mut shape,
            correlation,
        } = self;
        let refused = matches!(reply.as_ref(), Some(Err(error)) if !agent_client_protocol::is_incoming_transport_closed(error));
        let confirmed_legacy_mode = !shape.reads_value
            && shape.kind == ProviderSettingKind::Mode
            && reply
                .as_ref()
                .is_some_and(|reply| reply.as_ref().is_ok_and(serde_json::Value::is_object));
        let catalogue = match reply {
            Some(Ok(body))
                if shape.reads_value
                    && body
                        .get("configOptions")
                        .is_some_and(serde_json::Value::is_array) =>
            {
                serde_json::from_value::<SetSessionConfigOptionResponse>(body).ok()
            }
            _ => None,
        };
        let catalogue_reported = catalogue.is_some();
        let observed = if confirmed_legacy_mode {
            shape.catalog.current_mode = Some(shape.sent.clone());
            shape.catalog.mode_observation = crate::ProviderModeObservation::FromCatalogue;
            Some(SettingValueId(shape.sent.clone()))
        } else {
            catalogue.and_then(|response| {
                crate::provider_settings_catalog_codec::replace_catalog_config_options(
                    &mut shape.catalog,
                    &response.config_options,
                );
                crate::provider_settings_catalog_codec::apply_model_metadata(
                    &mut shape.catalog,
                    behavior,
                    response.meta.as_ref(),
                );
                if shape.kind == ProviderSettingKind::Mode {
                    shape.catalog.mode_observation = crate::ProviderModeObservation::FromCatalogue;
                }
                current_value(&shape.catalog, shape.kind)
            })
        };
        if let Some(readback) = shape.restore_readback.take()
            && (catalogue_reported || confirmed_legacy_mode)
        {
            *readback.0.lock().unwrap_or_else(PoisonError::into_inner) =
                Some(shape.catalog.clone());
        }
        match correlation {
            SettingCorrelation::Explicit(authorization) => {
                let fast_refusal = shape.kind == ProviderSettingKind::Fast
                    && catalogue_reported
                    && shape.catalog.fast_observed() != FastObserved::Off;
                let current = fast_refusal.then(|| observed.clone()).flatten();
                let confirmed = observed.filter(|value| {
                    value.0 == shape.sent
                        || shape.accepted_fallback.as_deref() == Some(value.0.as_str())
                });
                let observation = if let Some(value) = confirmed {
                    let mode_reason = shape.mode_confirmation.and_then(|mode| {
                        if shape.accepted_fallback.as_deref() == Some(value.0.as_str()) {
                            Some(crate::PermissionModeReason::AgentFellBack)
                        } else {
                            mode.reason
                        }
                    });
                    let fast = match shape.catalog.fast_observed() {
                        FastObserved::NoOption => FastReadback::NotOffered,
                        FastObserved::Off => FastReadback::Off,
                        FastObserved::On => FastReadback::On {
                            application: SettingApplication(Uuid::now_v7()),
                            input: session_event_model::InputId::generate(),
                        },
                    };
                    SettingObservation::AppliedWithReadback(Box::new(SettingReadback {
                        value,
                        settings: shape.catalog.to_session_settings(),
                        fast,
                        mode_reason,
                    }))
                } else if refused || fast_refusal {
                    SettingObservation::Refused { current }
                } else {
                    SettingObservation::Unknown
                };
                let retain = matches!(observation, SettingObservation::AppliedWithReadback(_))
                    || fast_refusal;
                (
                    OwnerEvent::Dispatch(DispatchResult::SettingFinished {
                        authorization,
                        observation,
                    }),
                    retain.then_some(shape.catalog),
                )
            }
            SettingCorrelation::RestoreMode { run, setting } => {
                let outcome = match observed {
                    Some(value) if value.0 == shape.sent => RestoreModeOutcome::Confirmed(value),
                    Some(value) if shape.accepted_fallback.as_deref() == Some(value.0.as_str()) => {
                        RestoreModeOutcome::FellBack(value)
                    }
                    Some(value) => RestoreModeOutcome::ReportedOther(value),
                    None if refused => RestoreModeOutcome::Refused {
                        current: shape.current,
                    },
                    None => RestoreModeOutcome::Unconfirmed,
                };
                (
                    OwnerEvent::Wire(SessionWireResult::RestoreMode {
                        run,
                        setting,
                        outcome,
                    }),
                    None,
                )
            }
            SettingCorrelation::RestoreFastOff { run, setting } => {
                let cause = match &observed {
                    Some(value)
                        if !crate::provider_session_settings::is_fast_off_select_value(
                            &value.0,
                        ) =>
                    {
                        Some(ObservationCause::ReportedOther)
                    }
                    Some(_) => None,
                    None if refused => Some(ObservationCause::Refused),
                    None => Some(ObservationCause::Unconfirmed),
                };
                (
                    OwnerEvent::Prepare(RestorePreparation::FastOff {
                        run,
                        setting,
                        observation: SettingEvidence {
                            value: observed
                                .map_or(ObservedSetting::Unknown, ObservedSetting::Confirmed),
                            cause,
                        },
                    }),
                    None,
                )
            }
        }
    }
}

fn current_value(
    catalog: &ProviderSettingsCatalog,
    kind: ProviderSettingKind,
) -> Option<SettingValueId> {
    if kind == ProviderSettingKind::Fast {
        return match catalog.fast_observed() {
            FastObserved::Off => Some(SettingValueId("off".to_owned())),
            FastObserved::On => Some(SettingValueId("on".to_owned())),
            FastObserved::NoOption => None,
        };
    }
    if let Some(option) = catalog.config_option(kind) {
        Some(SettingValueId(match &option.current_value {
            ProviderConfigValue::Select(value) => value.clone(),
            ProviderConfigValue::Boolean(value) => value.to_string(),
        }))
    } else if kind == ProviderSettingKind::Mode {
        catalog.effective_settings().mode.map(SettingValueId)
    } else {
        None
    }
}
