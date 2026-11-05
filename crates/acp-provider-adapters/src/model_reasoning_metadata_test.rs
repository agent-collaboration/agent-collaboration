use super::*;
use acp_client_runtime::{ProviderBehavior, ProviderSettingKind};
use serde_json::json;

#[test]
fn confirmation_reuse_is_qualified_by_provider_and_setting_scope() {
    for kind in [
        ProviderSettingKind::Mode,
        ProviderSettingKind::Model,
        ProviderSettingKind::Effort,
    ] {
        assert_eq!(
            crate::CursorAcpBehavior.can_reuse_setting_confirmation(kind),
            kind == ProviderSettingKind::Mode
        );
        assert!(
            crate::ClaudeAcpBehavior::new(crate::ClaudeSkillSource::Annotated)
                .can_reuse_setting_confirmation(kind)
        );
        assert!(
            !crate::ClaudeAcpBehavior::new(crate::ClaudeSkillSource::Unavailable(
                session_event_model::SkillSourceError::UnestablishedClassification
            ))
            .can_reuse_setting_confirmation(kind)
        );
        assert!(!crate::StandardAcpBehavior.can_reuse_setting_confirmation(kind));
    }
}

#[test]
fn literal_cursor_rows_keep_unknown_models_and_their_own_vendor_defaults() {
    // Installed Cursor7096 list_available_models response, not live current settings.
    let decoded = decode_cursor_models(&json!({"models":[
        {"value":"model-a","name":"A","configOptions":[
            {"id":"reasoning","name":"Reasoning","category":"thought_level","type":"select",
             "currentValue":"low","options":[{"value":"low","name":"Low"},{"value":"high","name":"High"}]}
        ]},
        {"value":"model-b","name":"B","configOptions":[]}
    ]}));
    let ModelReasoning::Select(control) = decoded.for_model("model-a") else {
        panic!("qualified select")
    };
    assert_eq!(control.config_id(), "reasoning");
    assert_eq!(control.default_value(), "low");
    assert_eq!(control.choices()[1].value, "high");
    assert_eq!(
        decoded.for_model("model-b"),
        ModelReasoning::Unknown(ModelReasoningUnknown::Unavailable)
    );
    assert_eq!(
        decoded.for_model("foreign"),
        ModelReasoning::Unknown(ModelReasoningUnknown::Unavailable)
    );
}

#[test]
fn literal_claude_annotation_requires_offered_default_and_unique_model_identity() {
    let valid = json!({"version":1,"kind":"available","models":[
        {"modelId":"sonnet","kind":"select","configId":"effort","defaultValue":"default",
         "choices":[{"value":"default","label":"Default"},{"value":"high","label":"High"}]},
        {"modelId":"haiku","kind":"unknown","reason":"Unavailable"}
    ]});
    let decoded = decode_claude_models(&valid);
    let ModelReasoning::Select(control) = decoded.for_model("sonnet") else {
        panic!("qualified select")
    };
    assert_eq!(control.default_value(), "default");
    assert_eq!(
        decoded.for_model("haiku"),
        ModelReasoning::Unknown(ModelReasoningUnknown::Unavailable)
    );
    let mut invalid = valid.clone();
    invalid["models"][0]["defaultValue"] = json!("medium");
    assert_eq!(
        decode_claude_models(&invalid),
        ModelReasoningMetadata::Unknown(ModelReasoningUnknown::Malformed)
    );
    invalid = valid;
    invalid["models"][1]["modelId"] = json!("sonnet");
    assert_eq!(
        decode_claude_models(&invalid),
        ModelReasoningMetadata::Unknown(ModelReasoningUnknown::Ambiguous)
    );
}
