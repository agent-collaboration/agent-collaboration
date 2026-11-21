use super::*;
use agent_client_protocol::schema::v1::{
    ConfigOptionUpdate, SessionConfigOptionCategory, SessionConfigSelectGroup,
    SessionConfigSelectOption, SessionMode, SessionModeState,
};
use provider_session_machines::ProviderSettingKind;

/// Oracle: ACP v1 session config options use semantic categories when
/// present, and their response after each set is the complete current set
/// (docs/protocol/v1/session-config-options.mdx:239-281).
#[test]
fn catalog_keeps_current_mode_model_effort_and_advertised_choices() {
    let response = NewSessionResponse::new("fixture-session")
        .modes(SessionModeState::new(
            "ask",
            vec![
                SessionMode::new("ask", "Ask"),
                SessionMode::new("auto", "Auto"),
            ],
        ))
        .config_options(vec![
            SessionConfigOption::select(
                "model",
                "Model",
                "model-a",
                vec![
                    SessionConfigSelectOption::new("model-a", "Model A"),
                    SessionConfigSelectOption::new("model-b", "Model B"),
                ],
            )
            .category(SessionConfigOptionCategory::Model),
            SessionConfigOption::select(
                "reasoning",
                "Reasoning",
                "medium",
                vec![SessionConfigSelectOption::new("medium", "Medium")],
            )
            .category(SessionConfigOptionCategory::ThoughtLevel),
        ]);
    let catalog = catalog_from_session_response(
        &response,
        &crate::test_behavior::TestProviderBehavior,
        &provider_session_machines::ModelReasoningMetadata::default(),
    );
    assert_eq!(catalog.effective_settings().mode.as_deref(), Some("ask"));
    assert_eq!(
        catalog.effective_settings().model.as_deref(),
        Some("model-a")
    );
    assert_eq!(
        catalog.effective_settings().effort.as_deref(),
        Some("medium")
    );
    assert_eq!(
        catalog.advertised_values(ProviderSettingKind::Model),
        vec!["model-a", "model-b"]
    );
}

#[test]
fn catalog_preserves_grouped_offers_and_current_values_at_admission_and_update() {
    let behavior = crate::test_behavior::TestProviderBehavior;
    let response = NewSessionResponse::new("fixture-session").config_options(vec![
        SessionConfigOption::boolean("thinking", "Thinking", false)
            .category(SessionConfigOptionCategory::ThoughtLevel),
        SessionConfigOption::select(
            "effort",
            "Reasoning",
            "Provider/Unlisted",
            vec![
                SessionConfigSelectGroup::new(
                    "fast",
                    "Fast",
                    vec![SessionConfigSelectOption::new("Provider/Low", "Low")],
                ),
                SessionConfigSelectGroup::new(
                    "slow",
                    "Slow",
                    vec![SessionConfigSelectOption::new("Provider/High", "High")],
                ),
            ],
        )
        .category(SessionConfigOptionCategory::ThoughtLevel),
    ]);
    let update = SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(vec![
        SessionConfigOption::boolean("thinking", "Thinking", true)
            .category(SessionConfigOptionCategory::ThoughtLevel),
        SessionConfigOption::select(
            "effort",
            "Reasoning",
            "Provider/Current",
            Vec::<SessionConfigSelectOption>::new(),
        ),
    ]));

    let mut catalog = catalog_from_session_response(
        &response,
        &behavior,
        &provider_session_machines::ModelReasoningMetadata::default(),
    );

    assert_eq!(
        catalog.config_options,
        vec![
            ProviderConfigOption {
                id: "thinking".into(),
                name: "Thinking".into(),
                category: None,
                current_value: ProviderConfigValue::Boolean(false),
                choices: vec![],
            },
            ProviderConfigOption {
                id: "effort".into(),
                name: "Reasoning".into(),
                category: Some(ProviderSettingKind::Effort),
                current_value: ProviderConfigValue::Select("Provider/Unlisted".into()),
                choices: vec![
                    ProviderSettingChoice {
                        value: "Provider/Low".into(),
                        label: "Low".into()
                    },
                    ProviderSettingChoice {
                        value: "Provider/High".into(),
                        label: "High".into()
                    },
                ],
            },
        ]
    );

    assert!(apply_settings_update(&mut catalog, &update, &behavior));

    assert_eq!(
        catalog.config_options,
        vec![
            ProviderConfigOption {
                id: "thinking".into(),
                name: "Thinking".into(),
                category: None,
                current_value: ProviderConfigValue::Boolean(true),
                choices: vec![],
            },
            ProviderConfigOption {
                id: "effort".into(),
                name: "Reasoning".into(),
                category: Some(ProviderSettingKind::Effort),
                current_value: ProviderConfigValue::Select("Provider/Current".into()),
                choices: vec![],
            },
        ]
    );
}
