//! Catalogue-qualified model-only decoding, independent of RPC/lifecycle fixtures.
use super::*;
use crate::ProviderReasoningChoice;
use serde_json::json;

fn catalogue_model(model_id: &str, choices: Option<&[&str]>) -> ProviderModelEntry {
    let entry =
        ProviderModelEntry::try_new(model_id.into(), "New".into(), String::new()).expect("model");
    match choices {
        None => entry,
        Some(values) => entry
            .with_reasoning(
                values
                    .iter()
                    .map(|value| ProviderReasoningChoice {
                        value: (*value).into(),
                        label: (*value).into(),
                    })
                    .collect(),
                "low".into(),
            )
            .expect("reasoning"),
    }
}

fn request(hint: Option<Value>, top_effort: Option<Value>) -> Value {
    let mut params = json!({"model":"new","collaborationMode":{"mode":"default","settings":{
        "model":"new","developer_instructions":null}}});
    if let Some(hint) = hint {
        params["collaborationMode"]["settings"]["reasoning_effort"] = hint;
    }
    if let Some(effort) = top_effort {
        params["effort"] = effort;
    }
    params
}

#[test]
fn model_only_effort_uses_the_selected_rows_own_choices() {
    // Arrange: both models and choices are data; no provider or model literal selects policy.
    // Columns are: omitted, null, high, low, provider, custom. These are the PD's
    // target-row dispositions, including the two offered-value/default controls.
    for (choices, expected_efforts) in [
        (
            None,
            [
                "provider", "provider", "provider", "provider", "provider", "provider",
            ],
        ),
        (
            Some(&["low"][..]),
            ["low", "low", "provider", "low", "provider", "provider"],
        ),
        (
            Some(&["low", "high"][..]),
            ["low", "low", "high", "low", "provider", "provider"],
        ),
    ] {
        for model_id in ["new", "alternate"] {
            for top_effort in [None, Some(Value::Null)] {
                for (column, hint) in [
                    None,
                    Some(Value::Null),
                    Some(json!("high")),
                    Some(json!("low")),
                    Some(json!("provider")),
                    Some(json!("custom hint")),
                ]
                .into_iter()
                .enumerate()
                {
                    let mut params = request(hint.clone(), top_effort.clone());
                    params["model"] = json!(model_id);
                    params["collaborationMode"]["settings"]["model"] = json!(model_id);
                    let catalog = [catalogue_model(model_id, choices)];

                    // Act: enter the production decoder's exact SettingsUpdate family.
                    let decoded = parse_native_settings(
                        &params,
                        &catalog,
                        NativeSettingsSource::SettingsUpdate,
                    )
                    .expect("valid model-only request");

                    // Assert: unavailable hints delegate; offered hints/defaults retain real writes.
                    assert_eq!(decoded.model.as_deref(), Some(model_id));
                    let expected = expected_efforts[column];
                    match decoded.effort {
                        Some(NativeEffortIntent::ProviderSettings) => {
                            assert_eq!(expected, "provider")
                        }
                        Some(NativeEffortIntent::ProviderValue(value)) => {
                            assert_eq!(value, expected)
                        }
                        None => panic!("model mask must retain its effort disposition"),
                    }
                    assert!(matches!(
                        decoded.intent,
                        session_event_model::SettingApplicationIntent::ExplicitChange
                    ));
                }
            }
        }
    }
}

#[test]
fn explicit_mask_only_and_unsupported_requests_do_not_gain_delegation() {
    let catalog = [catalogue_model("new", None)];
    let mut explicit = request(Some(json!("high")), Some(json!("high")));
    let mut mask_only = request(Some(json!("high")), None);
    mask_only.as_object_mut().expect("params").remove("model");
    let mut different = request(Some(json!("high")), None);
    different["model"] = json!("another-model");
    for params in [&mut explicit, &mut mask_only, &mut different] {
        assert!(matches!(
            parse_native_settings(params, &catalog, NativeSettingsSource::SettingsUpdate),
            Err(ThreadMethodError::Unsupported)
        ));
    }
    for (field, value) in [
        ("serviceTier", Value::Null),
        ("cwd", json!("/unsupported")),
        ("permissions", json!("workspace-write")),
        ("summary", json!("private")),
        ("approvalPolicy", json!("never")),
        ("approvalsReviewer", json!("automatic")),
        ("sandboxPolicy", json!("workspace-write")),
        ("disabledPluginIds", json!([])),
        ("personality", json!("custom")),
        ("multiAgentMode", json!(true)),
    ] {
        let mut params = request(Some(json!("high")), None);
        params[field] = value;
        assert!(matches!(
            parse_native_settings(&params, &catalog, NativeSettingsSource::SettingsUpdate),
            Err(ThreadMethodError::Unsupported)
        ));
    }
    for hint in [json!(""), json!("\0"), json!({"invalid":"hint"})] {
        assert!(
            parse_native_settings(
                &request(Some(hint), None),
                &catalog,
                NativeSettingsSource::SettingsUpdate
            )
            .is_err()
        );
    }
    for model_id in [
        json!("missing"),
        json!("provider-default"),
        json!(""),
        json!("\0"),
        json!(42),
        Value::Null,
    ] {
        let mut params = request(Some(json!("high")), None);
        params["model"] = model_id.clone();
        params["collaborationMode"]["settings"]["model"] = model_id;
        assert!(
            parse_native_settings(&params, &catalog, NativeSettingsSource::SettingsUpdate).is_err()
        );
    }
    for (field, value) in [
        ("mode", json!("plan")),
        ("developer_instructions", json!("")),
    ] {
        let mut params = request(Some(json!("high")), None);
        if field == "mode" {
            params["collaborationMode"][field] = value;
        } else {
            params["collaborationMode"]["settings"][field] = value;
        }
        assert!(matches!(
            parse_native_settings(&params, &catalog, NativeSettingsSource::SettingsUpdate),
            Err(ThreadMethodError::Unsupported)
        ));
    }
    let params = request(Some(json!("high")), None);
    for source in [
        NativeSettingsSource::Resume,
        NativeSettingsSource::TurnStart,
    ] {
        let decoded = parse_native_settings(&params, &catalog, source).expect("retained context");
        assert!(
            matches!(decoded.effort, Some(NativeEffortIntent::ProviderValue(value)) if value == "high")
        );
    }
}
