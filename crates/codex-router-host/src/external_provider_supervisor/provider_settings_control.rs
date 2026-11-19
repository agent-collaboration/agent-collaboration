//! Immediate settings actions use the durable Session inventory for authority.

use super::ExternalProviderSupervisor;
use crate::ExternalProviderRuntimeError;
use crate::provider_operation_settlement::effective_settings;
use collaboration_protocol::{
    ProviderIdentity, ProviderSettingName, ProviderSettingsAcceptRequest, ProviderSettingsFailure,
    ProviderSettingsFailureKind, ProviderSettingsMappingStatus, ProviderSettingsResult,
    ProviderSettingsSetRequest, SessionRef,
};
use collaboration_runtime::ProviderSessionRecord;

#[expect(
    clippy::result_large_err,
    reason = "The typed failure retains the full published operation payload."
)]
pub(super) async fn set(
    supervisor: &ExternalProviderSupervisor,
    request: ProviderSettingsSetRequest,
) -> Result<ProviderSettingsResult, ProviderSettingsFailure> {
    match apply(
        supervisor,
        request,
        session_event_model::SettingApplicationIntent::ExplicitChange,
        None,
    )
    .await
    {
        Ok(result) => Ok(result),
        Err(SettingControlFailure::Provider(error)) => Err(*error),
        Err(SettingControlFailure::ContextChanged(target)) => Err(failure(
            ProviderSettingsFailureKind::Unavailable,
            &target,
            "setting context unavailable",
        )),
        Err(SettingControlFailure::Unresolved { target, answer }) => Err(failure(
            ProviderSettingsFailureKind::Unavailable,
            &target,
            &answer,
        )),
    }
}

pub(super) enum SettingControlFailure {
    Provider(Box<ProviderSettingsFailure>),
    ContextChanged(SessionRef),
    Unresolved { target: SessionRef, answer: String },
}
impl From<ProviderSettingsFailure> for SettingControlFailure {
    fn from(error: ProviderSettingsFailure) -> Self {
        Self::Provider(Box::new(error))
    }
}

pub(super) async fn apply(
    supervisor: &ExternalProviderSupervisor,
    request: ProviderSettingsSetRequest,
    intent: session_event_model::SettingApplicationIntent,
    expected_model: Option<String>,
) -> Result<ProviderSettingsResult, SettingControlFailure> {
    let record = authorized_session(supervisor, &request.target, &request.actor).await?;
    // One check for every caller, before anything reaches the agent (D1 R7).
    if request.setting == ProviderSettingName::Fast
        && !matches!(request.value.as_str(), "off" | "auto")
    {
        return Err(ProviderSettingsFailure {
            kind: ProviderSettingsFailureKind::InvalidSetting,
            stage: None,
            target: Some(request.target.clone()),
            message: "fast accepts off or auto".into(),
            setting: Some(ProviderSettingName::Fast),
            value: Some(request.value.clone()),
            advertised: vec!["off".into(), "auto".into()],
        }
        .into());
    }
    if request.setting == ProviderSettingName::Mode
        && supervisor
            .provider_kind_for(&request.target.endpoint)
            .is_some_and(|provider| {
                super::provider_full_access_guard::is_full_access_mode(provider, &request.value)
            })
    {
        return Err(ProviderSettingsFailure {
            kind: ProviderSettingsFailureKind::InvalidSetting,
            stage: None,
            target: Some(request.target.clone()),
            message: super::provider_full_access_guard::FULL_ACCESS_REFUSAL.into(),
            setting: Some(ProviderSettingName::Mode),
            value: Some(request.value.clone()),
            advertised: Vec::new(),
        }
        .into());
    }
    if request.value.trim().is_empty() {
        return Err(failure(
            ProviderSettingsFailureKind::InvalidSetting,
            &request.target,
            "setting value must be nonempty",
        )
        .into());
    }
    let runtime = supervisor
        .runtime_for(&request.target.endpoint)
        .ok_or_else(|| {
            failure(
                ProviderSettingsFailureKind::Unavailable,
                &request.target,
                "provider connection is unavailable",
            )
        })?;
    let kind = match request.setting {
        ProviderSettingName::Mode => provider_session_machines::ProviderSettingKind::Mode,
        ProviderSettingName::Model => provider_session_machines::ProviderSettingKind::Model,
        ProviderSettingName::Effort => provider_session_machines::ProviderSettingKind::Effort,
        ProviderSettingName::Fast => provider_session_machines::ProviderSettingKind::Fast,
    };
    let records_mode = kind == provider_session_machines::ProviderSettingKind::Mode
        && intent == session_event_model::SettingApplicationIntent::ExplicitChange;
    let mode_order =
        records_mode.then(|| std::sync::Arc::clone(runtime.session_setup().mode_order()));
    let applying = apply_and_record(
        supervisor.clone(),
        runtime,
        request.clone(),
        kind,
        intent,
        expected_model,
        records_mode,
    );
    let observed = if let Some(mode_order) = mode_order {
        // Dropping the caller while it waits sends nothing. After taking the
        // turn, apply and record run together even when the caller stops waiting.
        mode_order
            .settle_mode_change(&String::from(request.target.session_id.clone()), applying)
            .await
            .map_err(|_| {
                SettingControlFailure::Provider(Box::new(failure(
                    ProviderSettingsFailureKind::Unavailable,
                    &request.target,
                    "the mode change ended before it settled",
                )))
            })??
    } else {
        applying.await?
    };
    Ok(result(request.target, record, observed))
}

/// Applies the change and, for an explicit mode change, records it, all
/// inside the session's mode turn: the turn is taken before the command
/// reaches the actor and held until the record is written, so the record
/// ends at the last change the agent applied (V6b).
async fn apply_and_record(
    supervisor: ExternalProviderSupervisor,
    runtime: std::sync::Arc<crate::ExternalProviderRuntime>,
    request: ProviderSettingsSetRequest,
    kind: provider_session_machines::ProviderSettingKind,
    intent: session_event_model::SettingApplicationIntent,
    expected_model: Option<String>,
    records_mode: bool,
) -> Result<acp_client_runtime::EffectiveProviderSettings, SettingControlFailure> {
    let session_id = String::from(request.target.session_id.clone());
    let observed = runtime
        .set_setting(
            session_id.clone(),
            acp_client_runtime::ProviderSettingRequest {
                kind,
                value: request.value.clone(),
                intent,
                expected_model,
            },
        )
        .await
        .map_err(|error| match error {
            ExternalProviderRuntimeError::SettingsContextChanged => {
                SettingControlFailure::ContextChanged(request.target.clone())
            }
            ExternalProviderRuntimeError::SettingsUnresolved { answer } => {
                SettingControlFailure::Unresolved {
                    target: request.target.clone(),
                    answer,
                }
            }
            other => SettingControlFailure::Provider(Box::new(setting_error(&request, other))),
        })?;
    if let Some(catalog) = runtime.settings_catalog(&session_id) {
        supervisor
            .inner
            .record_settings_catalog(request.target.clone(), catalog)
            .await;
    }
    if records_mode {
        #[cfg(any(test, feature = "test-support"))]
        hold_before_mode_record(&supervisor, &request.target, &request.value).await;
        record_explicit_mode(&supervisor, &runtime, &request.target, &request.value).await?;
    }
    Ok(observed)
}

/// A test holds this change here, applied and not yet recorded (V6b).
#[cfg(any(test, feature = "test-support"))]
async fn hold_before_mode_record(
    supervisor: &ExternalProviderSupervisor,
    target: &SessionRef,
    value: &str,
) {
    let pause = {
        let mut pause = supervisor
            .inner
            .explicit_mode_record_test_pause
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match pause.take() {
            Some((held, held_value, pause)) if &held == target && held_value == value => {
                Some(pause)
            }
            other => {
                *pause = other;
                None
            }
        }
    };
    if let Some(pause) = pause {
        let _entered = pause.entered.send(());
        let _released = pause.release.await;
    }
}

/// Keeps the session's record current after an explicit mode change the
/// agent applied: a restore puts back the mode the session runs now, never
/// the one its create asked for (V6; owner Decision 3; Lead ruling). A
/// refused or unknown change never reaches here, so it leaves the record.
async fn record_explicit_mode(
    supervisor: &ExternalProviderSupervisor,
    runtime: &crate::ExternalProviderRuntime,
    target: &SessionRef,
    value: &str,
) -> Result<(), SettingControlFailure> {
    let requested = acp_client_runtime::RequestedPermissionMode::from_request(Some(value));
    let Some(requested_mode) = requested.requested_name() else {
        return Ok(());
    };
    supervisor
        .inner
        .store
        .lock()
        .await
        .record_session_mode(target, requested_mode, super::now_ms())
        .await
        .map_err(|_| {
            failure(
                ProviderSettingsFailureKind::Unavailable,
                target,
                "the mode was applied, but the provider Session inventory is unavailable",
            )
        })?;
    runtime.session_setup().record_requested_mode(
        &String::from(target.session_id.clone()),
        crate::provider_session_setup::RequestedModeRecord {
            requested_mode: Some(requested_mode.to_owned()),
            mode_reason: None,
        },
    );
    Ok(())
}

#[expect(
    clippy::result_large_err,
    reason = "The typed failure retains the full published operation payload."
)]
pub(super) async fn accept(
    supervisor: &ExternalProviderSupervisor,
    request: ProviderSettingsAcceptRequest,
) -> Result<ProviderSettingsResult, ProviderSettingsFailure> {
    let record = authorized_session(supervisor, &request.target, &request.actor).await?;
    let runtime = supervisor
        .runtime_for(&request.target.endpoint)
        .ok_or_else(|| {
            failure(
                ProviderSettingsFailureKind::Unavailable,
                &request.target,
                "provider connection is unavailable",
            )
        })?;
    let observed = runtime
        .accept_session_settings(String::from(request.target.session_id.clone()))
        .map_err(|error| plain_error(&request.target, error))?;
    Ok(result(request.target, record, observed))
}

#[expect(
    clippy::result_large_err,
    reason = "The typed failure retains the full published operation payload."
)]
async fn authorized_session(
    supervisor: &ExternalProviderSupervisor,
    target: &SessionRef,
    actor: &ProviderIdentity,
) -> Result<ProviderSessionRecord, ProviderSettingsFailure> {
    let record = supervisor
        .inner
        .store
        .lock()
        .await
        .session_record(target)
        .await
        .map_err(|_| {
            failure(
                ProviderSettingsFailureKind::Unavailable,
                target,
                "provider Session inventory is unavailable",
            )
        })?
        .ok_or_else(|| {
            failure(
                ProviderSettingsFailureKind::NotFound,
                target,
                "provider Session is unknown",
            )
        })?;
    if actor != &record.created_by && actor != &record.approver {
        return Err(failure(
            ProviderSettingsFailureKind::WrongActor,
            target,
            "only the creator or Approver may change Session settings",
        ));
    }
    Ok(record)
}

fn result(
    target: SessionRef,
    record: ProviderSessionRecord,
    observed: acp_client_runtime::EffectiveProviderSettings,
) -> ProviderSettingsResult {
    let mut effective = effective_settings(record.requested_policy);
    effective.mapping_status = ProviderSettingsMappingStatus::Verified;
    effective.mode = observed.mode;
    effective.model = observed.model;
    effective.effort = observed.effort;
    effective.fast = observed
        .fast
        .map(crate::provider_operation_settlement::provider_fast_state);
    ProviderSettingsResult {
        target,
        effective_settings: effective,
    }
}

fn setting_error(
    request: &ProviderSettingsSetRequest,
    error: ExternalProviderRuntimeError,
) -> ProviderSettingsFailure {
    match error {
        // Setting and value come from the runtime error, so a fast failure
        // after a model change says fast and off (D1 R9).
        ExternalProviderRuntimeError::InvalidSetting {
            setting,
            value,
            advertised,
            ..
        } => ProviderSettingsFailure {
            kind: ProviderSettingsFailureKind::InvalidSetting,
            stage: None,
            target: Some(request.target.clone()),
            message: "setting value requires an unambiguous advertised provider option".into(),
            setting: Some(crate::provider_operation_settlement::provider_setting_name(
                setting,
            )),
            value: Some(value),
            advertised,
        },
        ExternalProviderRuntimeError::SettingFailed { setting, value, .. } => {
            ProviderSettingsFailure {
                kind: ProviderSettingsFailureKind::ProviderRejected,
                stage: None,
                target: Some(request.target.clone()),
                message: "provider rejected the setting; current reported values remain effective"
                    .into(),
                setting: Some(crate::provider_operation_settlement::provider_setting_name(
                    setting,
                )),
                value: Some(value),
                advertised: Vec::new(),
            }
        }
        ExternalProviderRuntimeError::SettingOutcomeUnknown { setting, value, .. } => {
            ProviderSettingsFailure {
                kind: ProviderSettingsFailureKind::OutcomeUnknown,
                stage: None,
                target: Some(request.target.clone()),
                message: "provider setting outcome is unknown; Session remains gated".into(),
                setting: Some(crate::provider_operation_settlement::provider_setting_name(
                    setting,
                )),
                value: Some(value),
                advertised: Vec::new(),
            }
        }
        other => {
            let mut failure = plain_error(&request.target, other);
            if failure.kind == ProviderSettingsFailureKind::OutcomeUnknown {
                failure.setting = Some(request.setting);
                failure.value = Some(request.value.clone());
            }
            failure
        }
    }
}

fn plain_error(
    target: &SessionRef,
    error: ExternalProviderRuntimeError,
) -> ProviderSettingsFailure {
    let busy_answer;
    let (kind, message) = match error {
        // The session's answer names its state and the valid next steps (D1 M8).
        ExternalProviderRuntimeError::LocalBusy { answer } => {
            busy_answer = answer;
            (ProviderSettingsFailureKind::Busy, busy_answer.as_str())
        }
        ExternalProviderRuntimeError::LocalNotFound => (
            ProviderSettingsFailureKind::NotFound,
            "provider Session is not loaded",
        ),
        ExternalProviderRuntimeError::TransportFailure
        | ExternalProviderRuntimeError::FrameDecodeFailure
        | ExternalProviderRuntimeError::FrameLimitExceeded => (
            ProviderSettingsFailureKind::OutcomeUnknown,
            "provider setting outcome is unknown; Session remains gated",
        ),
        _ => (
            ProviderSettingsFailureKind::Unavailable,
            "provider settings action is unavailable",
        ),
    };
    failure(kind, target, message)
}

fn failure(
    kind: ProviderSettingsFailureKind,
    target: &SessionRef,
    message: &str,
) -> ProviderSettingsFailure {
    ProviderSettingsFailure {
        kind,
        stage: None,
        target: Some(target.clone()),
        message: message.into(),
        setting: None,
        value: None,
        advertised: Vec::new(),
    }
}

impl ExternalProviderSupervisor {
    pub(crate) async fn apply_setting_command(
        &self,
        request: ProviderSettingsSetRequest,
        intent: session_event_model::SettingApplicationIntent,
        expected_model: Option<String>,
    ) -> Result<(), collaboration_contracts::CommandFailure> {
        use collaboration_contracts::CommandFailure;

        apply(self, request, intent, expected_model)
            .await
            .map(|_| ())
            .map_err(|error| match error {
                SettingControlFailure::ContextChanged(_) => CommandFailure::SettingsContextChanged,
                SettingControlFailure::Unresolved { .. } => CommandFailure::SettingsUnresolved,
                SettingControlFailure::Provider(error) => match error.kind {
                    collaboration_protocol::ProviderSettingsFailureKind::WrongActor => {
                        CommandFailure::UnauthorizedActor
                    }
                    collaboration_protocol::ProviderSettingsFailureKind::NotFound => {
                        CommandFailure::SessionNotFound
                    }
                    collaboration_protocol::ProviderSettingsFailureKind::Busy => {
                        CommandFailure::Busy
                    }
                    collaboration_protocol::ProviderSettingsFailureKind::InvalidSetting => {
                        CommandFailure::InvalidSetting {
                            advertised: error.advertised,
                        }
                    }
                    _ => CommandFailure::ProviderFailure,
                },
            })
    }
}

#[cfg(test)]
#[path = "provider_settings_control_test.rs"]
mod tests;
