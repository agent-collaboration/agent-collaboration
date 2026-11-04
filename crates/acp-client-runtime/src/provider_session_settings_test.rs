use super::*;
use session_event_model::{ConfigValue, ConfigValueState};

#[test]
fn current_config_values_preserve_select_and_boolean_kinds() {
    let catalog = ProviderSettingsCatalog {
        config_options: vec![
            ProviderConfigOption {
                id: "model".to_owned(),
                name: "Model".to_owned(),
                category: Some(ProviderSettingKind::Model),
                current_value: ProviderConfigValue::Select("model-a".to_owned()),
                choices: Vec::new(),
            },
            ProviderConfigOption {
                id: "autoRun".to_owned(),
                name: "Auto run".to_owned(),
                category: None,
                current_value: ProviderConfigValue::Boolean(true),
                choices: Vec::new(),
            },
        ],
        ..ProviderSettingsCatalog::default()
    };
    let settings = catalog.to_session_settings();
    assert_eq!(settings.model.as_deref(), Some("model-a"));
    assert_eq!(
        settings.config,
        vec![
            ConfigValue {
                id: "model".to_owned(),
                value: ConfigValueState::Select("model-a".to_owned())
            },
            ConfigValue {
                id: "autoRun".to_owned(),
                value: ConfigValueState::Boolean(true)
            },
        ]
    );
}
