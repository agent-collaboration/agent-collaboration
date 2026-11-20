use super::*;
use acp_client_runtime::ProviderBehavior;
use provider_session_machines::ProviderSettingKind;
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
fn cursor_discovery_preserves_eligibility_preference_and_duplicate_id_scope() {
    let mut response =
        json!({"models": [{"value": "Vendor/Model", "name": "Model", "configOptions": []}]});
    let mut bare_id = json!({
        "id": "thought_level", "type": "select", "currentValue": "Vendor/Bare",
        "options": [{"value": "Vendor/Bare", "name": "Bare"}]
    });
    let decode =
        |response: &serde_json::Value| decode_cursor_models(response).for_model("Vendor/Model");
    response["models"][0]["configOptions"] = json!([bare_id]);
    assert_eq!(
        decode(&response),
        ModelReasoning::Unknown(ModelReasoningUnknown::Unavailable)
    );

    bare_id["id"] = json!("model");
    bare_id["category"] = json!("thought_level");
    response["models"][0]["configOptions"] = json!([bare_id]);
    let ModelReasoning::Select(control) = decode(&response) else {
        panic!("explicit thought_level category qualifies the model ID");
    };
    assert_eq!(control.config_id(), "model");
    assert_eq!(control.default_value(), "Vendor/Bare");

    let explicit = json!({
        "id": "effort", "type": "select", "currentValue": "Vendor/Preferred",
        "options": [{"value": "Vendor/Preferred", "name": "Preferred"}]
    });
    response["models"][0]["configOptions"] = json!([bare_id, explicit]);
    let ModelReasoning::Select(control) = decode(&response) else {
        panic!("explicit effort wins over a categorized select");
    };
    assert_eq!(control.config_id(), "effort");
    assert_eq!(control.default_value(), "Vendor/Preferred");
    assert_eq!(control.choices()[0].label, "Preferred");

    response["models"][0]["configOptions"] = json!([
        bare_id, explicit, {"id": "effort", "type": "future_kind", "currentValue": true}
    ]);
    assert_eq!(
        decode(&response),
        ModelReasoning::Unknown(ModelReasoningUnknown::Ambiguous)
    );
}

#[test]
fn cursor_discovery_excludes_thinking_from_effort_candidates() {
    let thinking = json!({
        "id": "thinking", "category": "thought_level", "type": "select",
        "currentValue": "true", "options": [{"value": "true", "name": "On"}]
    });
    let reasoning = json!({
        "id": "reasoning", "category": "thought_level", "type": "select",
        "currentValue": "high", "options": [{"value": "high", "name": "High"}]
    });
    let response = json!({"models": [
        {"value": "thinking-only", "name": "Thinking only", "configOptions": [thinking]},
        {"value": "both-controls", "name": "Both controls", "configOptions": [thinking, reasoning]}
    ]});
    let expected_reasoning = ModelReasoningSelect::try_new(
        "reasoning".into(),
        vec![ProviderSettingChoice {
            value: "high".into(),
            label: "High".into(),
        }],
        "high".into(),
    )
    .expect("valid reasoning control");

    let metadata = decode_cursor_models(&response);

    assert_eq!(
        [
            metadata.for_model("thinking-only"),
            metadata.for_model("both-controls")
        ],
        [
            ModelReasoning::Unknown(ModelReasoningUnknown::Unavailable),
            ModelReasoning::Select(expected_reasoning)
        ],
        "thinking must neither qualify on its own nor compete with the offered effort"
    );
}

#[test]
fn cursor_discovery_keeps_row_errors_distinct_from_a_malformed_response() {
    let mut response = json!({"models": [
        {"value": "valid", "name": "Valid", "configOptions": [
            {"id": "effort", "type": "select", "currentValue": "low",
             "options": [{"value": "low", "name": "Low"}]}
        ]},
        {"value": "invalid", "name": "Invalid", "configOptions": [
            {"id": "effort", "type": "select", "currentValue": false}
        ]}
    ]});

    let decoded = decode_cursor_models(&response);

    assert!(matches!(
        decoded.for_model("valid"),
        ModelReasoning::Select(_)
    ));
    assert_eq!(
        decoded.for_model("invalid"),
        ModelReasoning::Unknown(ModelReasoningUnknown::Malformed)
    );

    response["models"][1]["configOptions"][0]["options"] = json!([
        {"value": "low", "name": "Low"}, {"value": "high"}
    ]);
    assert_eq!(
        decode_cursor_models(&response),
        ModelReasoningMetadata::Unknown(ModelReasoningUnknown::Malformed)
    );
}

#[test]
fn synthetic_grouped_core_acp_offers_are_malformed_for_cursor_discovery() {
    let response = json!({"models": [
        {"value": "valid", "name": "Valid", "configOptions": [
            {"id": "effort", "type": "select", "currentValue": "low",
             "options": [{"value": "low", "name": "Low"}]}
        ]},
        {"value": "grouped", "name": "Grouped", "configOptions": [
            {"id": "effort", "name": "Effort", "type": "select", "currentValue": "high",
             "options": [{"group": "limits", "name": "Limits",
                          "options": [{"value": "high", "name": "High"}]}]}
        ]}
    ]});

    assert_eq!(
        decode_cursor_models(&response),
        ModelReasoningMetadata::Unknown(ModelReasoningUnknown::Malformed)
    );
}

#[test]
fn cursor_discovery_keeps_the_two_field_positional_choice_boundary() {
    let decode = |choice: Value| {
        decode_cursor_models(&json!({"models": [
            {"value": "sibling", "name": "Sibling", "configOptions": [
                {"id": "effort", "type": "select", "currentValue": "low",
                 "options": [{"value": "low", "name": "Low"}]}
            ]},
            {"value": "target", "name": "Target", "configOptions": [
                {"id": "effort", "type": "select", "currentValue": "high", "options": [choice]}
            ]}
        ]}))
    };

    let accepted = [
        json!(["high", "High"]),
        json!({"value": "high", "name": "High", "description": "Optional",
               "_meta": {"vendor": true}}),
    ]
    .map(decode);

    for metadata in accepted {
        assert!(matches!(
            metadata.for_model("sibling"),
            ModelReasoning::Select(_)
        ));
        let ModelReasoning::Select(target) = metadata.for_model("target") else {
            panic!("expected Cursor to retain the advertised effort");
        };
        assert_eq!(target.default_value(), "high");
        assert_eq!(
            target.choices(),
            &[ProviderSettingChoice {
                value: "high".into(),
                label: "High".into()
            }]
        );
    }

    let rejected = [
        json!(["high", "High", "Optional"]),
        json!(["high", "High", "Optional", {"vendor": true}]),
    ]
    .map(decode);
    assert_eq!(
        rejected,
        [
            ModelReasoningMetadata::Unknown(ModelReasoningUnknown::Malformed),
            ModelReasoningMetadata::Unknown(ModelReasoningUnknown::Malformed)
        ],
        "extra positional fields must invalidate the reply, including the valid named sibling"
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
