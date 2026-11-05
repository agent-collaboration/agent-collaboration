use super::*;
use agent_client_protocol::schema::v1::{SessionConfigSelectOption, SessionMode, SessionModeState};

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
        &crate::ModelReasoningMetadata::default(),
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
