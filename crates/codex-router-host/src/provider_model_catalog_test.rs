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
