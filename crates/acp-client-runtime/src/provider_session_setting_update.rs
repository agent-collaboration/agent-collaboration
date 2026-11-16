//! Apply one advertised setting while its Session actor is idle.

use agent_client_protocol::{ActiveSession, Agent};

use crate::agent_session_client::ExternalProviderRuntimeError;
use crate::agent_session_client::provider_setting_application::{
    FAST_OFF, SettingSetupFailure, apply_initial_settings,
};
use crate::provider_session_actor::ProviderSessionRuntimeHandles;
use crate::provider_session_settings::UnresolvedSettingCause;
use crate::{
    EffectiveProviderSettings, FastRequest, InvalidSettingSessionDisposition, ProviderSettingKind,
    ProviderSettingsCatalog, RequestedProviderSettings,
};
use session_event_model::SessionEvent;

/// The fast value that stops the Router touching the agent's fast option.
const FAST_AUTO: &str = "auto";

pub(crate) async fn apply_loaded_setting(
    session: &ActiveSession<'_, Agent>,
    request: crate::ProviderSettingRequest,
    handles: &ProviderSessionRuntimeHandles,
) -> Result<EffectiveProviderSettings, ExternalProviderRuntimeError> {
    let provider_session_id = session.session_id().to_string();
    let crate::ProviderSettingRequest {
        kind,
        value,
        intent,
        expected_model,
    } = request;
    let mut catalog = handles
        .session_settings
        .read()
        .await
        .get(&provider_session_id)
        .cloned()
        .ok_or(ExternalProviderRuntimeError::LocalNotFound)?;
    if intent == session_event_model::SettingApplicationIntent::TurnContext
        && handles
            .settings_unresolved
            .read()
            .await
            .contains_key(&provider_session_id)
    {
        return Err(ExternalProviderRuntimeError::SettingsUnresolved);
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
        handles
            .keep_fast_off
            .write()
            .await
            .remove(&provider_session_id);
        return Ok(current);
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
        return Ok(current);
    }
    let mut requested = RequestedProviderSettings::default();
    match kind {
        ProviderSettingKind::Mode => requested.mode = Some(value.clone()),
        ProviderSettingKind::Model => requested.model = Some(value.clone()),
        ProviderSettingKind::Effort => requested.effort = Some(value.clone()),
        ProviderSettingKind::Fast if value == FAST_OFF => {
            requested.fast = FastRequest::Off;
            handles
                .keep_fast_off
                .write()
                .await
                .insert(provider_session_id.clone());
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
    let outcome =
        apply_initial_settings(session, &requested, &mut catalog, handles.behavior.as_ref()).await;
    match outcome.failure {
        Some(SettingSetupFailure::Invalid { advertised, .. }) => {
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
                keep_returned_catalog(handles, &provider_session_id, &catalog).await?;
                hold(
                    handles,
                    &provider_session_id,
                    kind,
                    UnresolvedSettingCause::DefiniteFailure,
                )
                .await;
            }
            Err(ExternalProviderRuntimeError::SettingFailed {
                setting: kind,
                value,
                reason,
            })
        }
        Some(SettingSetupFailure::Uncertain { .. }) => {
            hold(
                handles,
                &provider_session_id,
                kind,
                UnresolvedSettingCause::OutcomeUnknown,
            )
            .await;
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
                crate::agent_session_client::set_router_auto(
                    &handles.in_folder_grants,
                    &provider_session_id,
                    outcome.router_auto_active,
                );
            }
            commit_confirmed_setting(handles, &provider_session_id, kind, &catalog).await?;
            if kind == ProviderSettingKind::Model
                && catalog.offers_fast()
                && handles
                    .keep_fast_off
                    .read()
                    .await
                    .contains(&provider_session_id)
            {
                return keep_fast_off_after_model(session, handles, &provider_session_id, catalog)
                    .await;
            }
            Ok(catalog.effective_settings())
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
    let outcome =
        apply_initial_settings(session, &requested, &mut catalog, handles.behavior.as_ref()).await;
    let fast = ProviderSettingKind::Fast;
    match outcome.failure {
        None => {
            commit_confirmed_setting(handles, provider_session_id, fast, &catalog).await?;
            Ok(catalog.effective_settings())
        }
        Some(SettingSetupFailure::Agent { reason, .. }) => {
            keep_returned_catalog(handles, provider_session_id, &catalog).await?;
            hold(
                handles,
                provider_session_id,
                fast,
                UnresolvedSettingCause::DefiniteFailure,
            )
            .await;
            Err(ExternalProviderRuntimeError::SettingFailed {
                setting: fast,
                value: FAST_OFF.to_owned(),
                reason,
            })
        }
        Some(SettingSetupFailure::Invalid { .. }) => {
            hold(
                handles,
                provider_session_id,
                fast,
                UnresolvedSettingCause::DefiniteFailure,
            )
            .await;
            Err(ExternalProviderRuntimeError::SettingFailed {
                setting: fast,
                value: FAST_OFF.to_owned(),
                reason: "the agent's fast option can't be turned off".to_owned(),
            })
        }
        Some(SettingSetupFailure::Uncertain { .. }) => {
            hold(
                handles,
                provider_session_id,
                fast,
                UnresolvedSettingCause::OutcomeUnknown,
            )
            .await;
            Err(ExternalProviderRuntimeError::SettingOutcomeUnknown {
                provider_session_id: provider_session_id.to_owned(),
                setting: fast,
                value: FAST_OFF.to_owned(),
            })
        }
    }
}

/// Records an unresolved setting, so prompts are held until it's resolved.
async fn hold(
    handles: &ProviderSessionRuntimeHandles,
    provider_session_id: &str,
    kind: ProviderSettingKind,
    cause: UnresolvedSettingCause,
) {
    handles
        .settings_unresolved
        .write()
        .await
        .entry(provider_session_id.to_owned())
        .or_default()
        .insert(kind, cause);
}

/// Publishes and keeps the catalog the agent last answered with.
async fn keep_returned_catalog(
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
    handles
        .session_settings
        .write()
        .await
        .insert(provider_session_id.to_owned(), catalog.clone());
    Ok(())
}

/// Publishes and keeps a setting the agent confirmed, and ends its hold.
async fn commit_confirmed_setting(
    handles: &ProviderSessionRuntimeHandles,
    provider_session_id: &str,
    kind: ProviderSettingKind,
    catalog: &ProviderSettingsCatalog,
) -> Result<(), ExternalProviderRuntimeError> {
    keep_returned_catalog(handles, provider_session_id, catalog).await?;
    let mut unresolved = handles.settings_unresolved.write().await;
    if let Some(kinds) = unresolved.get_mut(provider_session_id) {
        kinds.remove(&kind);
        if kinds.is_empty() {
            unresolved.remove(provider_session_id);
        }
    }
    drop(unresolved);
    if let Some(capabilities) = handles
        .session_capabilities
        .read()
        .await
        .get(provider_session_id)
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
