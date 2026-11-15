//! Apply requested settings against each newly advertised ACP option set.

use agent_client_protocol::schema::v1::{
    SessionConfigOption, SessionConfigOptionValue, SetSessionConfigOptionRequest,
    SetSessionModeRequest,
};
use agent_client_protocol::{ConnectionTo, JsonRpcMessage, UntypedMessage};

use super::{ActiveSession, Agent, acp_operation_error};
use crate::{
    AppliedProviderSetting, ProviderSettingKind, ProviderSettingsCatalog, RequestedProviderSettings,
};
use std::time::Duration;

const SETTING_RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) enum SettingSetupFailure {
    Invalid {
        kind: ProviderSettingKind,
        value: String,
        advertised: Vec<String>,
    },
    Agent {
        kind: ProviderSettingKind,
        value: String,
        reason: String,
    },
    Uncertain {
        kind: ProviderSettingKind,
        value: String,
    },
}

enum SettingResponseFailure {
    Rejected(String),
    Unknown,
}

/// The outcome of applying a start's settings in order.
pub(crate) struct InitialSettingsOutcome {
    pub(crate) applied: Vec<AppliedProviderSetting>,
    pub(crate) failure: Option<SettingSetupFailure>,
    /// Finished as soon as the mode is known, so a later failure carries it.
    pub(crate) permission_mode: crate::PermissionModeReport,
    /// True only when `auto` was requested, the plan was `Set`, the mode
    /// setting succeeded, and the agent confirmed the planned mode or its
    /// accepted fallback.
    pub(crate) router_auto_active: bool,
}

/// The mode value to send, and how its outcome is judged and reported.
struct PlannedMode {
    value: String,
    accepted_fallback: Option<String>,
    reason: Option<crate::PermissionModeReason>,
}

/// Applies the requested settings in order. A request without a mode leaves
/// the agent's mode alone; a named mode that fails returns its failure under
/// the explicit-setting rules (D1 spec R1, R2).
pub(crate) async fn apply_initial_settings(
    session: &ActiveSession<'_, Agent>,
    requested: &RequestedProviderSettings,
    catalog: &mut ProviderSettingsCatalog,
    behavior: &dyn crate::ProviderBehavior,
) -> InitialSettingsOutcome {
    let requested_mode = crate::RequestedPermissionMode::from_request(requested.mode.as_deref());
    let router_auto_requested = requested_mode == crate::RequestedPermissionMode::Auto;
    let mut outcome = InitialSettingsOutcome {
        applied: Vec::new(),
        failure: None,
        permission_mode: crate::PermissionModeReport {
            requested: requested_mode.requested_name().map(str::to_owned),
            actual: None,
            reason: None,
        },
        router_auto_active: false,
    };
    let planned_mode = if requested_mode == crate::RequestedPermissionMode::Unspecified {
        outcome.permission_mode.actual = catalog.effective_settings().mode;
        None
    } else {
        match crate::plan_permission_mode(
            requested_mode,
            &catalog.advertised_values(ProviderSettingKind::Mode),
            behavior.auto_permission_mode(),
        ) {
            Err(refusal) => {
                outcome.failure = Some(SettingSetupFailure::Invalid {
                    kind: ProviderSettingKind::Mode,
                    value: refusal.value,
                    advertised: refusal.accepted,
                });
                return outcome;
            }
            Ok(crate::PermissionModePlan::Keep { reason, .. }) => {
                outcome.permission_mode.actual = catalog.effective_settings().mode;
                outcome.permission_mode.reason = reason;
                None
            }
            Ok(crate::PermissionModePlan::Set {
                mode,
                accepted_fallback,
                reason,
                ..
            }) => Some(PlannedMode {
                value: mode,
                accepted_fallback,
                reason,
            }),
        }
    };
    for kind in [
        ProviderSettingKind::Mode,
        ProviderSettingKind::Model,
        ProviderSettingKind::Effort,
    ] {
        let (value, accepted_fallback) = match kind {
            ProviderSettingKind::Mode => match &planned_mode {
                Some(planned) => (&planned.value, planned.accepted_fallback.as_deref()),
                None => continue,
            },
            ProviderSettingKind::Model => match requested.model.as_ref() {
                Some(value) => (value, None),
                None => continue,
            },
            ProviderSettingKind::Effort => match requested.effort.as_ref() {
                Some(value) => (value, None),
                None => continue,
            },
        };
        if let Err(failure) =
            apply_one_setting(session, catalog, behavior, kind, value, accepted_fallback).await
        {
            outcome.failure = Some(failure);
            return outcome;
        }
        outcome.applied.push(AppliedProviderSetting {
            kind,
            value: value.clone(),
        });
        if kind == ProviderSettingKind::Mode
            && let Some(planned) = &planned_mode
        {
            record_confirmed_mode(&mut outcome, planned, catalog, router_auto_requested);
        }
    }
    outcome
}

/// Finishes the mode report right after the agent confirmed the mode.
fn record_confirmed_mode(
    outcome: &mut InitialSettingsOutcome,
    planned: &PlannedMode,
    catalog: &ProviderSettingsCatalog,
    router_auto_requested: bool,
) {
    let actual = catalog.effective_settings().mode;
    let fell_back = planned.accepted_fallback.is_some()
        && actual.as_deref() == planned.accepted_fallback.as_deref();
    outcome.permission_mode.reason = if fell_back {
        Some(crate::PermissionModeReason::AgentFellBack)
    } else {
        planned.reason
    };
    outcome.router_auto_active =
        router_auto_requested && (fell_back || actual.as_deref() == Some(planned.value.as_str()));
    outcome.permission_mode.actual = actual;
}

async fn apply_one_setting(
    session: &ActiveSession<'_, Agent>,
    catalog: &mut ProviderSettingsCatalog,
    behavior: &dyn crate::ProviderBehavior,
    kind: ProviderSettingKind,
    value: &String,
    accepted_fallback: Option<&str>,
) -> Result<(), SettingSetupFailure> {
    let advertised = catalog.advertised_values(kind);
    if !advertised.iter().any(|offered| offered == value) {
        return Err(SettingSetupFailure::Invalid {
            kind,
            value: value.clone(),
            advertised,
        });
    }
    let result = if let Some(option) = catalog.config_option(kind) {
        let request = SetSessionConfigOptionRequest::new(
            session.session_id().clone(),
            option.id.clone(),
            SessionConfigOptionValue::value_id(value.clone()),
        );
        match request.to_untyped_message() {
            Ok(request) => match send_setting_request(session.connection(), request).await {
                Ok(response) => apply_config_response(
                    catalog,
                    kind,
                    value,
                    accepted_fallback,
                    response,
                    behavior,
                ),
                Err(failure) => Err(failure),
            },
            Err(_) => Err(SettingResponseFailure::Unknown),
        }
    } else if kind == ProviderSettingKind::Mode && !catalog.modes.is_empty() {
        let request = SetSessionModeRequest::new(session.session_id().clone(), value.clone());
        let result = match request.to_untyped_message() {
            Ok(request) => send_setting_request(session.connection(), request)
                .await
                .and_then(|response| {
                    response
                        .is_object()
                        .then_some(())
                        .ok_or(SettingResponseFailure::Unknown)
                }),
            Err(_) => Err(SettingResponseFailure::Unknown),
        };
        if result.is_ok() {
            catalog.current_mode = Some(value.clone());
        }
        result
    } else {
        return Err(SettingSetupFailure::Invalid {
            kind,
            value: value.clone(),
            advertised,
        });
    };
    result.map_err(|failure| match failure {
        SettingResponseFailure::Rejected(reason) => SettingSetupFailure::Agent {
            kind,
            value: value.clone(),
            reason,
        },
        SettingResponseFailure::Unknown => SettingSetupFailure::Uncertain {
            kind,
            value: value.clone(),
        },
    })
}

fn apply_config_response(
    catalog: &mut ProviderSettingsCatalog,
    kind: ProviderSettingKind,
    value: &str,
    accepted_fallback: Option<&str>,
    response: serde_json::Value,
    behavior: &dyn crate::ProviderBehavior,
) -> Result<(), SettingResponseFailure> {
    let options = response
        .get("configOptions")
        .and_then(serde_json::Value::as_array)
        .ok_or(SettingResponseFailure::Unknown)?;
    let options = options
        .iter()
        .cloned()
        .map(serde_json::from_value::<SessionConfigOption>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| SettingResponseFailure::Unknown)?;
    let mut updated = catalog.clone();
    crate::provider_settings_catalog_codec::replace_catalog_config_options(&mut updated, &options);
    crate::provider_settings_catalog_codec::apply_model_metadata(
        &mut updated,
        behavior,
        response.get("_meta").and_then(serde_json::Value::as_object),
    );
    let effective = updated.effective_settings();
    let selected = match kind {
        ProviderSettingKind::Mode => effective.mode.as_deref(),
        ProviderSettingKind::Model => effective.model.as_deref(),
        ProviderSettingKind::Effort => effective.effort.as_deref(),
    };
    if selected != Some(value) && (selected.is_none() || selected != accepted_fallback) {
        return Err(SettingResponseFailure::Unknown);
    }
    *catalog = updated;
    Ok(())
}

async fn send_setting_request(
    connection: &ConnectionTo<Agent>,
    request: UntypedMessage,
) -> Result<serde_json::Value, SettingResponseFailure> {
    let (reply, result) = tokio::sync::oneshot::channel();
    connection
        .send_request(request)
        .on_receiving_result(async move |response| {
            let _result = reply.send(response);
            Ok(())
        })
        .map_err(|_| SettingResponseFailure::Unknown)?;
    let response = tokio::time::timeout(SETTING_RESPONSE_TIMEOUT, result)
        .await
        .map_err(|_| SettingResponseFailure::Unknown)?
        .map_err(|_| SettingResponseFailure::Unknown)?;
    match response {
        Ok(response) => Ok(response),
        Err(error) if agent_client_protocol::is_incoming_transport_closed(&error) => {
            Err(SettingResponseFailure::Unknown)
        }
        Err(error) => Err(SettingResponseFailure::Rejected(
            acp_operation_error(error).to_string(),
        )),
    }
}
