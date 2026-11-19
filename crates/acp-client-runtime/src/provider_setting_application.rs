//! Apply requested settings against each newly advertised ACP option set.

use agent_client_protocol::schema::v1::{
    SessionConfigOption, SessionConfigOptionValue, SetSessionConfigOptionRequest,
    SetSessionModeRequest,
};
use agent_client_protocol::{ConnectionTo, JsonRpcMessage, UntypedMessage};
use provider_session_machines::ProviderSettingKind;
use provider_session_machines::session_setting_machine::{
    RestoreModeMismatch, RestoreModeObservation, SettingValueId,
};

use crate::provider_acp_error_mapping::acp_operation_error;
use crate::{AppliedProviderSetting, ProviderSettingsCatalog, RequestedProviderSettings};
use agent_client_protocol::{ActiveSession, Agent};
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
        restore_readback: Option<RestoreModeObservation>,
    },
}

enum SettingResponseFailure {
    Rejected(String),
    Unknown,
    RestoreReadback(RestoreModeObservation),
}

struct RestoreModeWrite<'clock> {
    claim: crate::session_cell_bookkeeping::RestoreModeWriteClaim,
    clock: &'clock dyn router_clock::RuntimeClock,
    report_named_mode: bool,
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
    apply_settings(session, requested, catalog, behavior, None).await
}

async fn apply_settings(
    session: &ActiveSession<'_, Agent>,
    requested: &RequestedProviderSettings,
    catalog: &mut ProviderSettingsCatalog,
    behavior: &dyn crate::ProviderBehavior,
    mut restore_write: Option<RestoreModeWrite<'_>>,
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
            if let Err(failure) = apply_fast_off(session, catalog, behavior).await {
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
            restore_write.take(),
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

/// Turns the agent's fast option off. A select gets "false", a boolean gets
/// false; only an answer that reads off confirms it (D1 R8).
async fn apply_fast_off(
    session: &ActiveSession<'_, Agent>,
    catalog: &mut ProviderSettingsCatalog,
    behavior: &dyn crate::ProviderBehavior,
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
            if !option.choices.iter().any(|choice| choice.value == "false") {
                return Err(invalid(
                    option
                        .choices
                        .iter()
                        .map(|choice| choice.value.clone())
                        .collect(),
                ));
            }
            SessionConfigOptionValue::value_id("false")
        }
    };
    let request =
        SetSessionConfigOptionRequest::new(session.session_id().clone(), option.id.clone(), value);
    let result = match request.to_untyped_message() {
        Ok(request) => match send_setting_request(session.connection(), request, None).await {
            Ok(response) => apply_fast_response(catalog, response, behavior),
            Err(failure) => Err(failure),
        },
        Err(_) => Err(SettingResponseFailure::Unknown),
    };
    result.map_err(|failure| match failure {
        SettingResponseFailure::Rejected(reason) => SettingSetupFailure::Agent {
            kind: ProviderSettingKind::Fast,
            value: FAST_OFF.to_owned(),
            reason,
        },
        SettingResponseFailure::Unknown | SettingResponseFailure::RestoreReadback(_) => {
            SettingSetupFailure::Uncertain {
                kind: ProviderSettingKind::Fast,
                value: FAST_OFF.to_owned(),
                restore_readback: None,
            }
        }
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
        crate::FastObserved::On => Err(SettingResponseFailure::Rejected(
            "the agent's fast option still reads on".to_owned(),
        )),
        crate::FastObserved::NoOption => Err(SettingResponseFailure::Rejected(
            "the agent's answer no longer lists a fast option".to_owned(),
        )),
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

/// A restored session's mode report and Router-auto answer (D1 R18; owner
/// Decision 3).
#[derive(Clone, Debug)]
pub(crate) struct RestoredPermissionMode {
    pub(crate) report: crate::PermissionModeReport,
    pub(crate) router_auto_active: bool,
    /// The record's named mode wasn't put back and the agent's mode allows
    /// more: the session loads but holds prompts until its mode is set
    /// (the Lead's mode-mismatch rule under owner Decision 3).
    pub(crate) mode_not_restored: Option<RestoreModeMismatch>,
}

/// Judges the mode the agent reports for a restored session against the mode
/// its record names, as a start judges the mode the agent confirmed. Nothing
/// is sent here; `reapply_recorded_mode` sets the mode back first when the
/// agent restored it in another one.
pub(crate) fn restored_permission_mode(
    requested: Option<&str>,
    catalog: &ProviderSettingsCatalog,
    behavior: &dyn crate::ProviderBehavior,
) -> RestoredPermissionMode {
    let requested_mode = crate::RequestedPermissionMode::from_request(requested);
    let actual = catalog.effective_settings().mode;
    let mut restored = RestoredPermissionMode {
        report: crate::PermissionModeReport {
            requested: requested_mode.requested_name().map(str::to_owned),
            actual: actual.clone(),
            reason: None,
        },
        router_auto_active: false,
        mode_not_restored: None,
    };
    if requested_mode == crate::RequestedPermissionMode::Unspecified {
        return restored;
    }
    let router_auto_requested = requested_mode == crate::RequestedPermissionMode::Auto;
    match crate::plan_permission_mode(
        requested_mode,
        &catalog.advertised_values(ProviderSettingKind::Mode),
        behavior.auto_permission_mode(),
    ) {
        Err(_) => {}
        Ok(crate::PermissionModePlan::Keep { reason, .. }) => restored.report.reason = reason,
        Ok(crate::PermissionModePlan::Set {
            mode,
            accepted_fallback,
            reason,
            ..
        }) => {
            let judged = judge_agent_mode(
                &PlannedMode {
                    value: mode,
                    accepted_fallback,
                    reason,
                },
                actual.as_deref(),
                router_auto_requested,
            );
            restored.report.reason = judged.reason;
            restored.router_auto_active = judged.router_auto_active;
        }
    }
    restored
}

/// Puts a restored session back in the mode its record names, the session's
/// current mode (V6; owner Decision 3: a session's mode survives a restart).
/// An agent can restore a session in another mode (Claude applies the user's
/// defaultMode on load), so when the mode it reports differs from the planned
/// one the Router sets it as a create does, before the session is registered
/// and any prompt admitted. The result is judged on the mode in force after
/// that. A set that fails leaves the session loaded without Router auto, its
/// requested and actual modes showing the mismatch (fail closed).
async fn put_back_recorded_mode(
    session: &ActiveSession<'_, Agent>,
    requested: Option<&str>,
    catalog: &mut ProviderSettingsCatalog,
    behavior: &dyn crate::ProviderBehavior,
    claim_write: impl FnOnce() -> Option<crate::session_cell_bookkeeping::RestoreModeWriteClaim>,
    clock: &dyn router_clock::RuntimeClock,
) -> RestoredPermissionMode {
    let kept = restored_permission_mode(requested, catalog, behavior);
    if !restored_mode_drifted(requested, catalog, behavior) {
        return kept;
    }
    // The write belongs to this load attempt: a close that came first means
    // nothing is sent, and the claim is held until the write is answered
    // (review F1).
    let Some(claim) = claim_write() else {
        return RestoredPermissionMode {
            report: crate::PermissionModeReport {
                requested: kept.report.requested,
                actual: catalog.effective_settings().mode,
                reason: kept.report.reason,
            },
            router_auto_active: false,
            mode_not_restored: None,
        };
    };
    let outcome = apply_settings(
        session,
        &RequestedProviderSettings {
            mode: requested.map(str::to_owned),
            ..RequestedProviderSettings::default()
        },
        catalog,
        behavior,
        Some(RestoreModeWrite {
            claim,
            clock,
            report_named_mode: matches!(
                crate::RequestedPermissionMode::from_request(requested),
                crate::RequestedPermissionMode::AgentMode(_)
            ),
        }),
    )
    .await;
    if let Some(failure) = outcome.failure {
        let actual = match failure {
            SettingSetupFailure::Uncertain {
                restore_readback: Some(readback),
                ..
            } => match readback {
                RestoreModeObservation::Confirmed(mode) => Some(mode.0),
                RestoreModeObservation::Unconfirmed => None,
            },
            _ => catalog.effective_settings().mode,
        };
        return RestoredPermissionMode {
            report: crate::PermissionModeReport {
                requested: kept.report.requested,
                actual,
                reason: kept.report.reason,
            },
            router_auto_active: false,
            mode_not_restored: None,
        };
    }
    RestoredPermissionMode {
        report: outcome.permission_mode,
        router_auto_active: outcome.router_auto_active,
        mode_not_restored: None,
    }
}

/// Marks a restore whose record names a mode the agent isn't in afterwards,
/// when the agent's mode allows more (or isn't ranked): the session holds
/// prompts until its mode is set. Router auto and a record with no mode keep
/// their own rules (the Lead's mode-mismatch rule).
fn with_mode_hold(
    mut restored: RestoredPermissionMode,
    requested: Option<&str>,
    behavior: &dyn crate::ProviderBehavior,
) -> RestoredPermissionMode {
    let crate::RequestedPermissionMode::AgentMode(recorded) =
        crate::RequestedPermissionMode::from_request(requested)
    else {
        return restored;
    };
    let actual = restored.report.actual.as_deref();
    if actual == Some(recorded.as_str()) {
        return restored;
    }
    let allows_more = match (
        actual.and_then(|mode| behavior.permission_rank(mode)),
        behavior.permission_rank(&recorded),
    ) {
        (Some(actual_rank), Some(recorded_rank)) => actual_rank > recorded_rank,
        // An unranked or unreported mode counts as allowing more.
        _ => true,
    };
    if allows_more {
        restored.mode_not_restored = Some(RestoreModeMismatch {
            recorded: SettingValueId(recorded),
            observed: actual.map_or(RestoreModeObservation::Unconfirmed, |mode| {
                RestoreModeObservation::Confirmed(SettingValueId(mode.to_owned()))
            }),
        });
    }
    restored
}

/// Puts back the recorded mode (`put_back_recorded_mode`), then marks the
/// restore when the agent is left in a more permissive mode (V6; the Lead's
/// mode-mismatch rule).
pub(crate) async fn reapply_recorded_mode(
    session: &ActiveSession<'_, Agent>,
    requested: Option<&str>,
    catalog: &mut ProviderSettingsCatalog,
    behavior: &dyn crate::ProviderBehavior,
    claim_write: impl FnOnce() -> Option<crate::session_cell_bookkeeping::RestoreModeWriteClaim>,
    clock: &dyn router_clock::RuntimeClock,
) -> RestoredPermissionMode {
    let restored =
        put_back_recorded_mode(session, requested, catalog, behavior, claim_write, clock).await;
    with_mode_hold(restored, requested, behavior)
}

/// Whether the agent restored the session in a mode other than the one its
/// record plans: neither the planned mode nor its accepted fallback.
fn restored_mode_drifted(
    requested: Option<&str>,
    catalog: &ProviderSettingsCatalog,
    behavior: &dyn crate::ProviderBehavior,
) -> bool {
    let requested_mode = crate::RequestedPermissionMode::from_request(requested);
    if requested_mode == crate::RequestedPermissionMode::Unspecified {
        return false;
    }
    let Ok(crate::PermissionModePlan::Set {
        mode,
        accepted_fallback,
        ..
    }) = crate::plan_permission_mode(
        requested_mode,
        &catalog.advertised_values(ProviderSettingKind::Mode),
        behavior.auto_permission_mode(),
    )
    else {
        return false;
    };
    let actual = catalog.effective_settings().mode;
    actual.as_deref() != Some(mode.as_str())
        && (accepted_fallback.is_none() || actual != accepted_fallback)
}

async fn apply_one_setting(
    session: &ActiveSession<'_, Agent>,
    catalog: &mut ProviderSettingsCatalog,
    behavior: &dyn crate::ProviderBehavior,
    kind: ProviderSettingKind,
    value: &String,
    accepted_fallback: Option<&str>,
    restore_write: Option<RestoreModeWrite<'_>>,
) -> Result<(), SettingSetupFailure> {
    let advertised = catalog.advertised_values(kind);
    if !advertised.iter().any(|offered| offered == value) {
        return Err(SettingSetupFailure::Invalid {
            kind,
            value: value.clone(),
            advertised,
        });
    }
    let restore = restore_write
        .as_ref()
        .is_some_and(|write| write.report_named_mode);
    let result = if let Some(option) = catalog.config_option(kind) {
        let request = SetSessionConfigOptionRequest::new(
            session.session_id().clone(),
            option.id.clone(),
            SessionConfigOptionValue::value_id(value.clone()),
        );
        match request.to_untyped_message() {
            Ok(request) => {
                match send_setting_request(session.connection(), request, restore_write).await {
                    Ok(response) => apply_config_response(
                        catalog,
                        kind,
                        value,
                        accepted_fallback,
                        response,
                        behavior,
                        restore,
                    ),
                    Err(failure) => Err(failure),
                }
            }
            Err(_) => Err(SettingResponseFailure::Unknown),
        }
    } else if kind == ProviderSettingKind::Mode && !catalog.modes.is_empty() {
        let request = SetSessionModeRequest::new(session.session_id().clone(), value.clone());
        let result = match request.to_untyped_message() {
            Ok(request) => send_setting_request(session.connection(), request, restore_write)
                .await
                .and_then(|response| {
                    response
                        .is_object()
                        .then_some(())
                        .ok_or_else(|| unconfirmed_response(restore))
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
            restore_readback: None,
        },
        SettingResponseFailure::RestoreReadback(readback) => SettingSetupFailure::Uncertain {
            kind,
            value: value.clone(),
            restore_readback: Some(readback),
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
    restore: bool,
) -> Result<(), SettingResponseFailure> {
    let updated = catalog_from_config_response(catalog, &response, behavior)
        .map_err(|_| unconfirmed_response(restore))?;
    let effective = updated.effective_settings();
    let selected = match kind {
        ProviderSettingKind::Mode if restore => {
            updated
                .config_option(kind)
                .and_then(|option| match &option.current_value {
                    crate::ProviderConfigValue::Select(value) => Some(value.as_str()),
                    crate::ProviderConfigValue::Boolean(_) => None,
                })
        }
        ProviderSettingKind::Mode => effective.mode.as_deref(),
        ProviderSettingKind::Model => effective.model.as_deref(),
        ProviderSettingKind::Effort => effective.effort.as_deref(),
        // Fast is applied by apply_fast_off, never through this path.
        ProviderSettingKind::Fast => return Err(SettingResponseFailure::Unknown),
    };
    if selected != Some(value) && (selected.is_none() || selected != accepted_fallback) {
        if restore && let Some(mode) = selected {
            let readback = RestoreModeObservation::Confirmed(SettingValueId(mode.to_owned()));
            *catalog = updated;
            return Err(SettingResponseFailure::RestoreReadback(readback));
        }
        return Err(unconfirmed_response(restore));
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
    restore_write: Option<RestoreModeWrite<'_>>,
) -> Result<serde_json::Value, SettingResponseFailure> {
    let (reply, result) = tokio::sync::oneshot::channel();
    let (claim, clock, restore) = restore_write.map_or((None, None, false), |write| {
        (
            Some(write.claim),
            Some(write.clock),
            write.report_named_mode,
        )
    });
    connection
        .send_request(request)
        .on_receiving_result(async move |response| {
            // This callback is owned by the connection's response correlation.
            // It stays registered when the local timeout drops `result`.
            drop(claim);
            let _result = reply.send(response);
            Ok(())
        })
        .map_err(|_| unconfirmed_response(restore))?;
    let response = if let Some(clock) = clock {
        router_clock::timeout(clock, SETTING_RESPONSE_TIMEOUT, result)
            .await
            .map_err(|_| unconfirmed_response(restore))?
    } else {
        tokio::time::timeout(SETTING_RESPONSE_TIMEOUT, result)
            .await
            .map_err(|_| unconfirmed_response(restore))?
    }
    .map_err(|_| unconfirmed_response(restore))?;
    match response {
        Ok(response) => Ok(response),
        Err(error) if agent_client_protocol::is_incoming_transport_closed(&error) => {
            Err(unconfirmed_response(restore))
        }
        Err(error) => Err(SettingResponseFailure::Rejected(
            acp_operation_error(error).to_string(),
        )),
    }
}

fn unconfirmed_response(restore: bool) -> SettingResponseFailure {
    if restore {
        SettingResponseFailure::RestoreReadback(RestoreModeObservation::Unconfirmed)
    } else {
        SettingResponseFailure::Unknown
    }
}
