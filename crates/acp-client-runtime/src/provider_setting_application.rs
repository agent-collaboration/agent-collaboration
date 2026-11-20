//! Apply requested settings against each newly advertised ACP option set.

use agent_client_protocol::schema::v1::{
    SessionConfigOption, SessionConfigOptionValue, SessionId, SetSessionConfigOptionRequest,
    SetSessionModeRequest,
};
use agent_client_protocol::{ConnectionTo, JsonRpcMessage, UntypedMessage};
use provider_session_machines::ProviderSettingKind;

use crate::provider_acp_error_mapping::acp_operation_error;
use crate::{AppliedProviderSetting, ProviderSettingsCatalog, RequestedProviderSettings};
use agent_client_protocol::Agent;
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
        confirmed_current: bool,
    },
    Uncertain {
        kind: ProviderSettingKind,
        value: String,
    },
}

enum SettingResponseFailure {
    Rejected {
        reason: String,
        confirmed_current: bool,
    },
    Unknown,
}

struct SettingResponseWait<'clock> {
    clock: &'clock dyn router_clock::RuntimeClock,
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

#[derive(Clone, Copy)]
pub(crate) struct InitialSettingTarget<'connection> {
    pub(crate) connection: &'connection ConnectionTo<Agent>,
    pub(crate) session_id: &'connection SessionId,
}

/// Applies the requested settings in order. A request without a mode leaves
/// the agent's mode alone; a named mode that fails returns its failure under
/// the explicit-setting rules (D1 spec R1, R2).
pub(crate) async fn apply_initial_settings(
    session: InitialSettingTarget<'_>,
    requested: &RequestedProviderSettings,
    catalog: &mut ProviderSettingsCatalog,
    behavior: &dyn crate::ProviderBehavior,
    clock: &dyn router_clock::RuntimeClock,
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
        ProviderSettingKind::Fast,
    ] {
        if kind == ProviderSettingKind::Fast {
            if !fast_off_requested(requested.fast, catalog) {
                continue;
            }
            if let Err(failure) = apply_fast_off(session, catalog, behavior, clock).await {
                outcome.failure = Some(failure);
                return outcome;
            }
            outcome.applied.push(AppliedProviderSetting {
                kind,
                value: FAST_OFF.to_owned(),
            });
            continue;
        }
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
            ProviderSettingKind::Fast => continue,
        };
        if let Err(failure) = apply_one_setting(
            session,
            catalog,
            behavior,
            kind,
            value,
            accepted_fallback,
            SettingResponseWait { clock },
        )
        .await
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

/// The value a fast change reports: the Router only ever turns fast off.
pub(crate) const FAST_OFF: &str = "off";

/// Whether this request turns fast off: an explicit off always runs (and is
/// invalid without a fast option); the create default runs only when the
/// agent offers fast, so it never becomes a pending hold (D1 R7, R10).
pub(crate) fn fast_off_requested(
    fast: crate::FastRequest,
    catalog: &ProviderSettingsCatalog,
) -> bool {
    match fast {
        crate::FastRequest::Leave => false,
        crate::FastRequest::OffIfOffered => catalog.offers_fast(),
        crate::FastRequest::Off => true,
    }
}

/// Sends the advertised off identifier for a select and false for a boolean.
/// Only an answer that reads off confirms the change.
async fn apply_fast_off(
    session: InitialSettingTarget<'_>,
    catalog: &mut ProviderSettingsCatalog,
    behavior: &dyn crate::ProviderBehavior,
    clock: &dyn router_clock::RuntimeClock,
) -> Result<(), SettingSetupFailure> {
    let invalid = |advertised: Vec<String>| SettingSetupFailure::Invalid {
        kind: ProviderSettingKind::Fast,
        value: FAST_OFF.to_owned(),
        advertised,
    };
    let Some(option) = catalog.config_option(ProviderSettingKind::Fast) else {
        return Err(invalid(Vec::new()));
    };
    let value = match option.current_value {
        crate::ProviderConfigValue::Boolean(_) => SessionConfigOptionValue::boolean(false),
        crate::ProviderConfigValue::Select(_) => {
            let Some(off) = option.choices.iter().find(|choice| {
                crate::provider_session_settings::is_fast_off_select_value(&choice.value)
            }) else {
                return Err(invalid(
                    option
                        .choices
                        .iter()
                        .map(|choice| choice.value.clone())
                        .collect(),
                ));
            };
            SessionConfigOptionValue::value_id(off.value.clone())
        }
    };
    let request =
        SetSessionConfigOptionRequest::new(session.session_id.clone(), option.id.clone(), value);
    let result = match request.to_untyped_message() {
        Ok(request) => {
            match send_setting_request(session.connection, request, SettingResponseWait { clock })
                .await
            {
                Ok(response) => apply_fast_response(catalog, response, behavior),
                Err(failure) => Err(failure),
            }
        }
        Err(_) => Err(SettingResponseFailure::Unknown),
    };
    result.map_err(|failure| match failure {
        SettingResponseFailure::Rejected {
            reason,
            confirmed_current,
        } => SettingSetupFailure::Agent {
            kind: ProviderSettingKind::Fast,
            value: FAST_OFF.to_owned(),
            reason,
            confirmed_current,
        },
        SettingResponseFailure::Unknown => SettingSetupFailure::Uncertain {
            kind: ProviderSettingKind::Fast,
            value: FAST_OFF.to_owned(),
        },
    })
}

/// For fast only, a readable answer that still reads on, or that no longer
/// lists a fast option, is a definite refusal (D1 R8); an unreadable one is
/// unknown.
fn apply_fast_response(
    catalog: &mut ProviderSettingsCatalog,
    response: serde_json::Value,
    behavior: &dyn crate::ProviderBehavior,
) -> Result<(), SettingResponseFailure> {
    let updated = catalog_from_config_response(catalog, &response, behavior)?;
    let observed = updated.fast_observed();
    // The agent answered readably: its catalog is the truth from here on,
    // whether or not it confirms off (D1 V14), so nothing reports a stale one.
    *catalog = updated;
    match observed {
        crate::FastObserved::Off => Ok(()),
        crate::FastObserved::On => Err(SettingResponseFailure::Rejected {
            reason: "the agent's fast option still reads on".to_owned(),
            confirmed_current: true,
        }),
        crate::FastObserved::NoOption => Err(SettingResponseFailure::Rejected {
            reason: "the agent's answer no longer lists a fast option".to_owned(),
            confirmed_current: true,
        }),
    }
}

/// Finishes the mode report right after the agent confirmed the mode.
fn record_confirmed_mode(
    outcome: &mut InitialSettingsOutcome,
    planned: &PlannedMode,
    catalog: &ProviderSettingsCatalog,
    router_auto_requested: bool,
) {
    let actual = catalog.effective_settings().mode;
    let judged = judge_agent_mode(planned, actual.as_deref(), router_auto_requested);
    outcome.permission_mode.reason = judged.reason;
    outcome.router_auto_active = judged.router_auto_active;
    outcome.permission_mode.actual = actual;
}

/// How the mode an agent reports answers a planned mode.
struct JudgedMode {
    reason: Option<crate::PermissionModeReason>,
    /// Router auto was asked for and the agent runs the planned mode or its
    /// accepted fallback.
    router_auto_active: bool,
}

fn judge_agent_mode(
    planned: &PlannedMode,
    actual: Option<&str>,
    router_auto_requested: bool,
) -> JudgedMode {
    let fell_back =
        planned.accepted_fallback.is_some() && actual == planned.accepted_fallback.as_deref();
    JudgedMode {
        reason: if fell_back {
            Some(crate::PermissionModeReason::AgentFellBack)
        } else {
            planned.reason
        },
        router_auto_active: router_auto_requested
            && (fell_back || actual == Some(planned.value.as_str())),
    }
}

async fn apply_one_setting(
    session: InitialSettingTarget<'_>,
    catalog: &mut ProviderSettingsCatalog,
    behavior: &dyn crate::ProviderBehavior,
    kind: ProviderSettingKind,
    value: &String,
    accepted_fallback: Option<&str>,
    wait: SettingResponseWait<'_>,
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
            session.session_id.clone(),
            option.id.clone(),
            SessionConfigOptionValue::value_id(value.clone()),
        );
        match request.to_untyped_message() {
            Ok(request) => match send_setting_request(session.connection, request, wait).await {
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
        let request = SetSessionModeRequest::new(session.session_id.clone(), value.clone());
        let result = match request.to_untyped_message() {
            Ok(request) => send_setting_request(session.connection, request, wait)
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
            catalog.mode_observation = crate::ProviderModeObservation::FromCatalogue;
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
        SettingResponseFailure::Rejected {
            reason,
            confirmed_current,
        } => SettingSetupFailure::Agent {
            kind,
            value: value.clone(),
            reason,
            confirmed_current,
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
    let updated = catalog_from_config_response(catalog, &response, behavior)?;
    let effective = updated.effective_settings();
    let selected = match kind {
        ProviderSettingKind::Mode => effective.mode.as_deref(),
        ProviderSettingKind::Model => effective.model.as_deref(),
        ProviderSettingKind::Effort => effective.effort.as_deref(),
        // Fast is applied by apply_fast_off, never through this path.
        ProviderSettingKind::Fast => return Err(SettingResponseFailure::Unknown),
    };
    if selected != Some(value) && (selected.is_none() || selected != accepted_fallback) {
        return Err(SettingResponseFailure::Unknown);
    }
    *catalog = updated;
    Ok(())
}

/// The catalog a set_config_option answer describes, or Unknown when the
/// answer can't be read.
fn catalog_from_config_response(
    catalog: &ProviderSettingsCatalog,
    response: &serde_json::Value,
    behavior: &dyn crate::ProviderBehavior,
) -> Result<ProviderSettingsCatalog, SettingResponseFailure> {
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
    Ok(updated)
}

async fn send_setting_request(
    connection: &ConnectionTo<Agent>,
    request: UntypedMessage,
    wait: SettingResponseWait<'_>,
) -> Result<serde_json::Value, SettingResponseFailure> {
    let (reply, result) = tokio::sync::oneshot::channel();
    let clock = wait.clock;
    connection
        .send_request(request)
        .on_receiving_result(async move |response| {
            // This callback is owned by the connection's response correlation.
            // It stays registered when the local timeout drops `result`.
            let _result = reply.send(response);
            Ok(())
        })
        .map_err(|_| SettingResponseFailure::Unknown)?;
    let response = router_clock::timeout(clock, SETTING_RESPONSE_TIMEOUT, result)
        .await
        .map_err(|_| SettingResponseFailure::Unknown)?
        .map_err(|_| SettingResponseFailure::Unknown)?;
    match response {
        Ok(response) => Ok(response),
        Err(error) if agent_client_protocol::is_incoming_transport_closed(&error) => {
            Err(SettingResponseFailure::Unknown)
        }
        Err(error) => Err(SettingResponseFailure::Rejected {
            reason: acp_operation_error(error).to_string(),
            confirmed_current: false,
        }),
    }
}

pub(crate) fn current_mode_grants_auto(
    requested: Option<&str>,
    catalog: &ProviderSettingsCatalog,
    behavior: &dyn crate::ProviderBehavior,
) -> bool {
    let requested_mode = crate::RequestedPermissionMode::from_request(requested);
    if requested_mode != crate::RequestedPermissionMode::Auto {
        return false;
    }
    let Ok(crate::PermissionModePlan::Set {
        mode,
        accepted_fallback,
        reason,
        ..
    }) = crate::plan_permission_mode(
        requested_mode,
        &catalog.advertised_values(ProviderSettingKind::Mode),
        behavior.auto_permission_mode(),
    )
    else {
        return false;
    };
    judge_agent_mode(
        &PlannedMode {
            value: mode,
            accepted_fallback,
            reason,
        },
        catalog.effective_settings().mode.as_deref(),
        true,
    )
    .router_auto_active
}
