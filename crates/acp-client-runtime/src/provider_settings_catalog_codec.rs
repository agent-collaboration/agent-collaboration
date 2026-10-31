//! Decode ACP session options once into a provider-neutral settings catalog.

use agent_client_protocol::schema::v1::{
    NewSessionResponse, SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory,
    SessionConfigSelectOptions, SessionUpdate,
};

use crate::{
    ProviderConfigOption, ProviderConfigValue, ProviderSettingChoice, ProviderSettingKind,
    ProviderSettingsCatalog,
};

pub(crate) fn catalog_from_session_response(
    response: &NewSessionResponse,
    behavior: &dyn crate::ProviderBehavior,
    connection_models: &crate::ModelReasoningMetadata,
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
        modes,
        config_options: response
            .config_options
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter_map(project_config_option)
            .collect(),
    };
    apply_model_metadata(&mut catalog, behavior, response.meta.as_ref());
    catalog
}

pub(crate) fn apply_model_metadata(
    catalog: &mut ProviderSettingsCatalog,
    behavior: &dyn crate::ProviderBehavior,
    metadata: Option<&serde_json::Map<String, serde_json::Value>>,
) {
    if behavior.model_metadata_source() == crate::ModelMetadataSource::SessionMetadata {
        catalog.model_reasoning = metadata
            .and_then(|meta| meta.get("codexRouter"))
            .and_then(|meta| meta.get("modelReasoning"))
            .map_or_else(crate::ModelReasoningMetadata::default, |value| {
                behavior.decode_model_metadata(value)
            });
    }
}

pub(crate) fn replace_catalog_config_options(
    catalog: &mut ProviderSettingsCatalog,
    options: &[SessionConfigOption],
) {
    catalog.config_options = options.iter().filter_map(project_config_option).collect();
}

/// Apply agent-reported settings without losing the initial choice catalog.
/// Replay and live updates use the same rule.
pub(crate) fn apply_settings_update(
    catalog: &mut ProviderSettingsCatalog,
    update: &SessionUpdate,
    behavior: &dyn crate::ProviderBehavior,
) -> bool {
    match update {
        SessionUpdate::CurrentModeUpdate(mode) => {
            let current_mode = mode.current_mode_id.0.to_string();
            catalog.current_mode = Some(current_mode.clone());
            if let Some(option) = catalog
                .config_options
                .iter_mut()
                .find(|option| option.category == Some(ProviderSettingKind::Mode))
            {
                option.current_value = ProviderConfigValue::Select(current_mode);
            }
            true
        }
        SessionUpdate::ConfigOptionUpdate(config) => {
            apply_model_metadata(catalog, behavior, config.meta.as_ref());
            replace_catalog_config_options(catalog, &config.config_options);
            if let Some(mode) = catalog.config_option(ProviderSettingKind::Mode)
                && let ProviderConfigValue::Select(value) = &mode.current_value
            {
                catalog.current_mode = Some(value.clone());
            }
            true
        }
        _ => false,
    }
}

fn project_config_option(option: &SessionConfigOption) -> Option<ProviderConfigOption> {
    let (current_value, choices) = match &option.kind {
        SessionConfigKind::Select(select) => {
            let choices = match &select.options {
                SessionConfigSelectOptions::Ungrouped(options) => options
                    .iter()
                    .map(|choice| ProviderSettingChoice {
                        value: choice.value.0.to_string(),
                        label: choice.name.clone(),
                    })
                    .collect(),
                SessionConfigSelectOptions::Grouped(groups) => groups
                    .iter()
                    .flat_map(|group| &group.options)
                    .map(|choice| ProviderSettingChoice {
                        value: choice.value.0.to_string(),
                        label: choice.name.clone(),
                    })
                    .collect(),
                _ => Vec::new(),
            };
            (
                ProviderConfigValue::Select(select.current_value.0.to_string()),
                choices,
            )
        }
        SessionConfigKind::Boolean(boolean) => (
            ProviderConfigValue::Boolean(boolean.current_value),
            Vec::new(),
        ),
        _ => return None,
    };
    let id = option.id.0.to_string();
    Some(ProviderConfigOption {
        category: setting_category(option.category.as_ref(), &id),
        id,
        name: option.name.clone(),
        current_value,
        choices,
    })
}

fn setting_category(
    category: Option<&SessionConfigOptionCategory>,
    id: &str,
) -> Option<ProviderSettingKind> {
    // Cursor gives both toggled thinking and reasoning effort the broad
    // thought_level category. Their advertised identities remain distinct.
    if id == "thinking" {
        return None;
    }
    if id == "effort" {
        return Some(ProviderSettingKind::Effort);
    }
    match category {
        Some(SessionConfigOptionCategory::Mode) => return Some(ProviderSettingKind::Mode),
        Some(SessionConfigOptionCategory::Model) => return Some(ProviderSettingKind::Model),
        Some(SessionConfigOptionCategory::ThoughtLevel) => {
            return Some(ProviderSettingKind::Effort);
        }
        Some(SessionConfigOptionCategory::ModelConfig)
        | Some(SessionConfigOptionCategory::Other(_))
        | None => {}
        _ => {}
    }
    match id {
        "mode" => Some(ProviderSettingKind::Mode),
        "model" => Some(ProviderSettingKind::Model),
        "effort" | "thought_level" => Some(ProviderSettingKind::Effort),
        _ => None,
    }
}

#[cfg(test)]
#[path = "provider_settings_catalog_codec_test.rs"]
mod tests;
