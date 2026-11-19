//! Apply one advertised setting while its Session actor is idle.

use agent_client_protocol::{ActiveSession, Agent};
use provider_session_machines::ProviderSettingKind;

use crate::provider_client_contract::ExternalProviderRuntimeError;
use crate::provider_session_runtime_handles::ProviderSessionRuntimeHandles;
use crate::provider_setting_application::{FAST_OFF, SettingSetupFailure, apply_initial_settings};
use crate::{
    EffectiveProviderSettings, FastRequest, InvalidSettingSessionDisposition,
    ProviderSettingApplication, ProviderSettingsCatalog, RequestedProviderSettings,
};
use provider_session_machines::session_setting_machine::{
    SessionSettingEvent, SettingApplication, SettingValueId,
};
use session_event_model::SessionEvent;

/// The fast value that stops the Router touching the agent's fast option.
const FAST_AUTO: &str = "auto";

pub(crate) async fn apply_loaded_setting(
    session: &ActiveSession<'_, Agent>,
    request: crate::ProviderSettingRequest,
    handles: &ProviderSessionRuntimeHandles,
) -> Result<ProviderSettingApplication, ExternalProviderRuntimeError> {
    let provider_session_id = session.session_id().to_string();
    let crate::ProviderSettingRequest {
        kind,
        value,
        intent,
        expected_model,
    } = request;
    let mut catalog = handles
        .session_settings
        .read_in(&provider_session_id, handles.run_generation)
        .ok_or(ExternalProviderRuntimeError::LocalNotFound)?;
    if intent == session_event_model::SettingApplicationIntent::TurnContext
        && crate::session_cell_bookkeeping::settings_held(
            &handles.session_cells,
            &provider_session_id,
        )
    {
        return Err(ExternalProviderRuntimeError::SettingsUnresolved {
            answer: crate::session_cell_bookkeeping::settings_hold_answer_for(
                &handles.session_cells,
                &provider_session_id,
            ),
        });
    }
    let current = catalog.effective_settings();
    if expected_model
        .as_ref()
        .is_some_and(|expected| current.model.as_ref() != Some(expected))
    {
        return Err(ExternalProviderRuntimeError::SettingsContextChanged);
    }
    if kind == ProviderSettingKind::Fast && value == FAST_AUTO {
        // A policy change only: nothing reaches the agent, and every hold
        // stays exactly as it was (D1 R7).
        crate::session_cell_bookkeeping::set_keep_fast_off_in_run(
            &handles.session_cells,
            &provider_session_id,
            handles.run_generation,
            false,
        );
        return Ok(ProviderSettingApplication {
            effective_settings: current,
            confirmed_requested_mode: None,
        });
    }
    let current_value = match kind {
        ProviderSettingKind::Mode => current.mode.as_ref(),
        ProviderSettingKind::Model => current.model.as_ref(),
        ProviderSettingKind::Effort => current.effort.as_ref(),
        ProviderSettingKind::Fast => None,
    };
    if intent == session_event_model::SettingApplicationIntent::TurnContext
        && handles.behavior.can_reuse_setting_confirmation(kind)
        && current_value == Some(&value)
    {
        return Ok(ProviderSettingApplication {
            effective_settings: current,
            confirmed_requested_mode: None,
        });
    }
    let mut requested = RequestedProviderSettings::default();
    match kind {
        ProviderSettingKind::Mode => requested.mode = Some(value.clone()),
        ProviderSettingKind::Model => requested.model = Some(value.clone()),
        ProviderSettingKind::Effort => requested.effort = Some(value.clone()),
        ProviderSettingKind::Fast if value == FAST_OFF => {
            requested.fast = FastRequest::Off;
            crate::session_cell_bookkeeping::set_keep_fast_off_in_run(
                &handles.session_cells,
                &provider_session_id,
                handles.run_generation,
                true,
            );
        }
        ProviderSettingKind::Fast => {
            return Err(ExternalProviderRuntimeError::InvalidSetting {
                setting: kind,
                value,
                advertised: vec![FAST_OFF.to_owned(), FAST_AUTO.to_owned()],
                provider_session_id,
                disposition: InvalidSettingSessionDisposition::RemainsCreated,
            });
        }
    }
    // The change is in flight on the kind's cell; a prompt waits behind it
    // in this actor, as today (the Lead's 3b-5 ruling).
    let application = SettingApplication(uuid::Uuid::now_v7());
    record(
        handles,
        &provider_session_id,
        kind,
        SessionSettingEvent::SetRequested {
            to: SettingValueId(value.clone()),
            application,
        },
    );
    let outcome = apply_initial_settings(
        session,
        &requested,
        &mut catalog,
        handles.behavior.as_ref(),
        handles.clock.as_ref(),
    )
    .await;
    match outcome.failure {
        Some(SettingSetupFailure::Invalid { advertised, .. }) => {
            // An unadvertised value holds nothing, as today.
            record(
                handles,
                &provider_session_id,
                kind,
                SessionSettingEvent::Refused { application },
            );
            Err(ExternalProviderRuntimeError::InvalidSetting {
                setting: kind,
                value,
                advertised,
                provider_session_id,
                disposition: InvalidSettingSessionDisposition::RemainsCreated,
            })
        }
        Some(SettingSetupFailure::Agent { reason, .. }) => {
            if kind == ProviderSettingKind::Fast {
                // A session meant to be non-fast never quietly runs fast (R8):
                // a refused explicit off holds prompts, with the agent's catalog.
                keep_returned_catalog(handles, &provider_session_id, &catalog)?;
                record(
                    handles,
                    &provider_session_id,
                    kind,
                    SessionSettingEvent::Failed { application },
                );
            } else {
                // A refused mode, model or effort holds nothing, as today.
                record(
                    handles,
                    &provider_session_id,
                    kind,
                    SessionSettingEvent::Refused { application },
                );
            }
            Err(ExternalProviderRuntimeError::SettingFailed {
                setting: kind,
                value,
                reason,
            })
        }
        Some(SettingSetupFailure::Uncertain { .. }) => {
            record(
                handles,
                &provider_session_id,
                kind,
                SessionSettingEvent::OutcomeUnknown { application },
            );
            Err(ExternalProviderRuntimeError::SettingOutcomeUnknown {
                provider_session_id,
                setting: kind,
                value,
            })
        }
        None => {
            if kind == ProviderSettingKind::Mode {
                // The mode the agent confirmed decides whether auto's in-folder
                // grant applies; uncertain or failed changes never reach here.
                crate::in_folder_allowance::set_router_auto(
                    &handles.in_folder_grants,
                    &provider_session_id,
                    handles.run_generation,
                    outcome.router_auto_active,
                );
            }
            if outcome.applied.is_empty() {
                record(
                    handles,
                    &provider_session_id,
                    kind,
                    SessionSettingEvent::Refused { application },
                );
                return Ok(ProviderSettingApplication {
                    effective_settings: current,
                    confirmed_requested_mode: None,
                });
            }
            let confirmed_requested_mode =
                if kind == ProviderSettingKind::Mode && outcome.permission_mode.reason.is_none() {
                    outcome.permission_mode.requested
                } else {
                    None
                };
            commit_confirmed_setting(handles, &provider_session_id, kind, application, &catalog)?;
            let effective_settings = if kind == ProviderSettingKind::Model
                && catalog.offers_fast()
                && crate::session_cell_bookkeeping::keep_fast_off(
                    &handles.session_cells,
                    &provider_session_id,
                ) {
                keep_fast_off_after_model(session, handles, &provider_session_id, catalog).await?
            } else {
                catalog.effective_settings()
            };
            Ok(ProviderSettingApplication {
                effective_settings,
                confirmed_requested_mode,
            })
        }
    }
}

/// The second half of a model change for a session kept non-fast: Cursor
/// turns fast back on with every model change, so fast off follows at once.
/// The confirmed model stays in effect whatever fast's outcome (D1 R9).
async fn keep_fast_off_after_model(
    session: &ActiveSession<'_, Agent>,
    handles: &ProviderSessionRuntimeHandles,
    provider_session_id: &str,
    mut catalog: ProviderSettingsCatalog,
) -> Result<EffectiveProviderSettings, ExternalProviderRuntimeError> {
    let requested = RequestedProviderSettings {
        fast: FastRequest::Off,
        ..RequestedProviderSettings::default()
    };
    let fast = ProviderSettingKind::Fast;
    let application = SettingApplication(uuid::Uuid::now_v7());
    record(
        handles,
        provider_session_id,
        fast,
        SessionSettingEvent::SetRequested {
            to: SettingValueId(FAST_OFF.to_owned()),
            application,
        },
    );
    let outcome = apply_initial_settings(
        session,
        &requested,
        &mut catalog,
        handles.behavior.as_ref(),
        handles.clock.as_ref(),
    )
    .await;
    match outcome.failure {
        None => {
            commit_confirmed_setting(handles, provider_session_id, fast, application, &catalog)?;
            Ok(catalog.effective_settings())
        }
        Some(SettingSetupFailure::Agent { reason, .. }) => {
            keep_returned_catalog(handles, provider_session_id, &catalog)?;
            record(
                handles,
                provider_session_id,
                fast,
                SessionSettingEvent::Failed { application },
            );
            Err(ExternalProviderRuntimeError::SettingFailed {
                setting: fast,
                value: FAST_OFF.to_owned(),
                reason,
            })
        }
        Some(SettingSetupFailure::Invalid { .. }) => {
            record(
                handles,
                provider_session_id,
                fast,
                SessionSettingEvent::Failed { application },
            );
            Err(ExternalProviderRuntimeError::SettingFailed {
                setting: fast,
                value: FAST_OFF.to_owned(),
                reason: "the agent's fast option can't be turned off".to_owned(),
            })
        }
        Some(SettingSetupFailure::Uncertain { .. }) => {
            record(
                handles,
                provider_session_id,
                fast,
                SessionSettingEvent::OutcomeUnknown { application },
            );
            Err(ExternalProviderRuntimeError::SettingOutcomeUnknown {
                provider_session_id: provider_session_id.to_owned(),
                setting: fast,
                value: FAST_OFF.to_owned(),
            })
        }
    }
}

/// Records one setting event on the session's cell for this kind.
fn record(
    handles: &ProviderSessionRuntimeHandles,
    provider_session_id: &str,
    kind: ProviderSettingKind,
    event: SessionSettingEvent,
) {
    crate::session_cell_bookkeeping::apply_setting(
        &handles.session_cells,
        provider_session_id,
        kind,
        event,
    );
}

/// Publishes and keeps the catalog the agent last answered with.
fn keep_returned_catalog(
    handles: &ProviderSessionRuntimeHandles,
    provider_session_id: &str,
    catalog: &ProviderSettingsCatalog,
) -> Result<(), ExternalProviderRuntimeError> {
    handles
        .event_sink
        .publish(
            provider_session_id,
            SessionEvent::SettingsChanged {
                settings: catalog.to_session_settings(),
            },
        )
        .map_err(|_| ExternalProviderRuntimeError::SinkClosed)?;
    // Kept only for the run that asked: an ended run's continuation changes
    // nothing (design note §11.3; Lead V3).
    let _kept = handles.session_settings.write(
        provider_session_id,
        handles.run_generation,
        catalog.clone(),
    );
    Ok(())
}

/// Publishes and keeps a setting the agent confirmed, and ends its change.
fn commit_confirmed_setting(
    handles: &ProviderSessionRuntimeHandles,
    provider_session_id: &str,
    kind: ProviderSettingKind,
    application: SettingApplication,
    catalog: &ProviderSettingsCatalog,
) -> Result<(), ExternalProviderRuntimeError> {
    keep_returned_catalog(handles, provider_session_id, catalog)?;
    let confirmed = match kind {
        ProviderSettingKind::Mode => catalog.effective_settings().mode,
        ProviderSettingKind::Model => catalog.effective_settings().model,
        ProviderSettingKind::Effort => catalog.effective_settings().effort,
        ProviderSettingKind::Fast => None,
    };
    record(
        handles,
        provider_session_id,
        kind,
        SessionSettingEvent::Applied {
            application,
            value: SettingValueId(confirmed.unwrap_or_default()),
        },
    );
    if let Some(capabilities) = handles
        .session_capabilities
        .read_in(provider_session_id, handles.run_generation)
        .as_ref()
        .map(crate::ProviderCapabilityReport::to_session_model)
    {
        handles
            .event_sink
            .publish(
                provider_session_id,
                SessionEvent::CapabilitiesChanged { capabilities },
            )
            .map_err(|_| ExternalProviderRuntimeError::SinkClosed)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "provider_session_setting_update_test.rs"]
mod tests;
