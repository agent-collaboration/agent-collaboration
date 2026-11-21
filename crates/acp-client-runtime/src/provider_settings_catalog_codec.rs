//! Build and refresh the runtime settings catalog from provider observations.

use agent_client_protocol::schema::v1::{NewSessionResponse, SessionConfigOption, SessionUpdate};
use provider_session_machines::ProviderSettingKind;
use vendor_protocol::acp::settings_codec::{
    ObservedSetting, ObservedSettingValue, SettingCategory, observe_config_options,
};

use provider_session_machines::{
    ProviderConfigOption, ProviderConfigValue, ProviderSettingChoice, ProviderSettingsCatalog,
};

pub(crate) fn catalog_from_session_response(
    response: &NewSessionResponse,
    behavior: &dyn crate::ProviderBehavior,
    connection_models: &provider_session_machines::ModelReasoningMetadata,
) -> ProviderSettingsCatalog {
    let (current_mode, modes) = response.modes.as_ref().map_or_else(
        || (None, Vec::new()),
        |state| {
            (
                Some(state.current_mode_id.0.to_string()),
                state
                    .available_modes
                    .iter()
                    .map(|mode| ProviderSettingChoice {
                        value: mode.id.0.to_string(),
                        label: mode.name.clone(),
                    })
                    .collect(),
            )
        },
    );
    let mut catalog = ProviderSettingsCatalog {
        model_reasoning: connection_models.clone(),
        current_mode,
        mode_observation: provider_session_machines::ProviderModeObservation::FromCatalogue,
        modes,
        config_options: catalog_config_options(response.config_options.as_deref()),
    };
    apply_model_metadata(&mut catalog, behavior, response.meta.as_ref());
    catalog
}

pub(crate) fn apply_model_metadata(
    catalog: &mut ProviderSettingsCatalog,
    behavior: &dyn crate::ProviderBehavior,
    metadata: Option<&serde_json::Map<String, serde_json::Value>>,
) {
    if let Some(reasoning) = reported_model_metadata(behavior, metadata) {
        catalog.model_reasoning = reasoning;
    }
}

fn reported_model_metadata(
    behavior: &dyn crate::ProviderBehavior,
    metadata: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Option<provider_session_machines::ModelReasoningMetadata> {
    (behavior.model_metadata_source() == crate::ModelMetadataSource::SessionMetadata).then(|| {
        metadata
            .and_then(|meta| meta.get("codexRouter"))
            .and_then(|meta| meta.get("modelReasoning"))
            .map_or_else(
                provider_session_machines::ModelReasoningMetadata::default,
                |value| behavior.decode_model_metadata(value),
            )
    })
}

pub(crate) fn replace_catalog_config_options(
    catalog: &mut ProviderSettingsCatalog,
    options: &[SessionConfigOption],
) {
    catalog.config_options = catalog_config_options(Some(options));
    if catalog
        .config_option(ProviderSettingKind::Mode)
        .is_some_and(|option| matches!(option.current_value, ProviderConfigValue::Select(_)))
    {
        catalog.mode_observation =
            provider_session_machines::ProviderModeObservation::FromCatalogue;
    }
}

/// Apply the provider's settings during provisional replay.
pub(crate) fn apply_settings_update(
    catalog: &mut ProviderSettingsCatalog,
    update: &SessionUpdate,
    behavior: &dyn crate::ProviderBehavior,
) -> bool {
    let Some(update) = decode_settings_update(update, behavior) else {
        return false;
    };
    catalog.apply_update(update);
    true
}

pub(crate) fn decode_settings_update(
    update: &SessionUpdate,
    behavior: &dyn crate::ProviderBehavior,
) -> Option<provider_session_machines::provider_settings_catalog::ProviderSettingsUpdate> {
    use provider_session_machines::provider_settings_catalog::ProviderSettingsUpdate;

    match update {
        SessionUpdate::CurrentModeUpdate(mode) => Some(ProviderSettingsUpdate::CurrentMode(
            mode.current_mode_id.0.to_string(),
        )),
        SessionUpdate::ConfigOptionUpdate(config) => Some(ProviderSettingsUpdate::ConfigOptions {
            options: catalog_config_options(Some(&config.config_options)),
            model_reasoning: reported_model_metadata(behavior, config.meta.as_ref()),
        }),
        _ => None,
    }
}

fn catalog_config_options(options: Option<&[SessionConfigOption]>) -> Vec<ProviderConfigOption> {
    observe_config_options(options)
        .unwrap_or_default()
        .into_iter()
        .map(catalog_config_option)
        .collect()
}

fn catalog_config_option(option: ObservedSetting<'_>) -> ProviderConfigOption {
    let (current_value, choices) = match option.value {
        ObservedSettingValue::Select { current, offered } => (
            ProviderConfigValue::Select(current.0.to_string()),
            offered
                .unwrap_or_default()
                .into_iter()
                .map(|choice| ProviderSettingChoice {
                    value: choice.value.0.to_string(),
                    label: choice.name.clone(),
                })
                .collect(),
        ),
        ObservedSettingValue::Boolean(current) => {
            (ProviderConfigValue::Boolean(current), Vec::new())
        }
    };
    ProviderConfigOption {
        category: option.category.map(|category| match category {
            SettingCategory::Mode => ProviderSettingKind::Mode,
            SettingCategory::Model => ProviderSettingKind::Model,
            SettingCategory::Effort => ProviderSettingKind::Effort,
            SettingCategory::Fast => ProviderSettingKind::Fast,
        }),
        id: option.id.0.to_string(),
        name: option.name.to_owned(),
        current_value,
        choices,
    }
}

#[cfg(test)]
#[path = "provider_settings_catalog_codec_test.rs"]
mod tests;
