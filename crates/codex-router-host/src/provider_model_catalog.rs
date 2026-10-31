//! Project agent-advertised model choices for one provider connection.

use acp_client_runtime::{ModelReasoning, ProviderSettingKind, ProviderSettingsCatalog};
use collaboration_runtime::{ProviderModelEntry, ProviderReasoningChoice};

pub(crate) struct SupervisorModelCatalog {
    supervisor: std::sync::Arc<crate::ExternalProviderSupervisor>,
}
impl SupervisorModelCatalog {
    pub(crate) fn new(supervisor: std::sync::Arc<crate::ExternalProviderSupervisor>) -> Self {
        Self { supervisor }
    }
}
impl collaboration_runtime::SessionModelCatalog for SupervisorModelCatalog {
    fn read_models(
        &self,
        session: message_board::SessionRef,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<
                        Vec<ProviderModelEntry>,
                        collaboration_runtime::SessionModelCatalogError,
                    >,
                > + Send
                + '_,
        >,
    > {
        Box::pin(async move {
            let target = serde_json::to_value(session)
                .and_then(serde_json::from_value)
                .map_err(|_| collaboration_runtime::SessionModelCatalogError::Unavailable)?;
            self.supervisor.session_model_catalog(&target).await
        })
    }
}

pub(crate) fn provider_default_model() -> Result<ProviderModelEntry, &'static str> {
    ProviderModelEntry::try_new(
        "provider-default".into(),
        "Use provider settings".into(),
        "Use provider settings".into(),
    )
    .map_err(|_| "invalid provider default model")
}

pub(crate) fn model_entries(catalog: &ProviderSettingsCatalog) -> Vec<ProviderModelEntry> {
    let advertised = catalog
        .config_options
        .iter()
        .find(|option| option.category == Some(ProviderSettingKind::Model))
        .map(|option| option.choices.as_slice())
        .unwrap_or_default();
    let entries = advertised
        .iter()
        .filter_map(|choice| {
            let entry = ProviderModelEntry::try_new(
                choice.value.clone(),
                choice.label.clone(),
                "Advertised by the provider".into(),
            )
            .ok()?;
            match catalog.model_reasoning.for_model(&choice.value) {
                ModelReasoning::Unknown(_) => Some(entry),
                ModelReasoning::Select(control) => entry
                    .with_reasoning(
                        control
                            .choices()
                            .iter()
                            .map(|choice| ProviderReasoningChoice {
                                value: choice.value.clone(),
                                label: choice.label.clone(),
                            })
                            .collect(),
                        control.default_value().to_owned(),
                    )
                    .ok(),
            }
        })
        .collect::<Vec<_>>();
    if entries.is_empty() {
        provider_default_model().into_iter().collect()
    } else {
        entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acp_client_runtime::{ProviderConfigOption, ProviderConfigValue, ProviderSettingChoice};

    #[test]
    fn model_choices_replace_provider_default_in_agent_order() {
        let catalog = ProviderSettingsCatalog {
            model_reasoning: acp_client_runtime::ModelReasoningMetadata::default(),
            current_mode: None,
            modes: Vec::new(),
            config_options: vec![ProviderConfigOption {
                id: "model".into(),
                name: "Model".into(),
                category: Some(ProviderSettingKind::Model),
                current_value: ProviderConfigValue::Select("model-b".into()),
                choices: vec![
                    ProviderSettingChoice {
                        value: "model-a".into(),
                        label: "Model A".into(),
                    },
                    ProviderSettingChoice {
                        value: "model-b".into(),
                        label: "Model B".into(),
                    },
                ],
            }],
        };
        let entries = model_entries(&catalog);
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            ProviderModelEntry::try_new(
                "model-a".into(),
                "Model A".into(),
                "Advertised by the provider".into()
            )
            .expect("entry")
        );
        assert_eq!(
            entries[1],
            ProviderModelEntry::try_new(
                "model-b".into(),
                "Model B".into(),
                "Advertised by the provider".into()
            )
            .expect("entry")
        );
    }
}
