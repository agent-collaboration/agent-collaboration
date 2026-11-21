use super::{
    ObservedSettingValue, SessionNotificationError, SettingCategory, decode_session_notification,
    observe_config_options,
};
use agent_client_protocol_schema::v1::{
    NewSessionResponse, SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelectGroup,
    SessionConfigSelectOption, SessionUpdate,
};
use serde_json::{Value, json};

fn settings_notification(options: Value, metadata: Value) -> Value {
    json!({"sessionId": "session", "update": {
        "sessionUpdate": "config_option_update", "configOptions": options, "_meta": metadata,
    }})
}

#[test]
fn complete_settings_allow_additions_and_preserve_provider_values() {
    let params = settings_notification(
        json!([{
            "id": " Vendor/Option ", "name": "Picker", "type": "select",
            "category": "_vendor_category", "currentValue": " Value/Current ",
            "futureOptionField": 42, "description": null,
            "options": [{
                "group": "group", "name": "Group", "futureGroupField": 42,
                "options": [
                    {"value": " Value/B ", "name": "B", "futureChoiceField": 42},
                    {"value": " Value/A ", "name": "A", "_meta": null}
                ]
            }]
        }, {
            "id": "toggle", "name": "Toggle", "type": "boolean", "currentValue": false
        }]),
        json!({"vendor": {"enabled": true}}),
    );

    let notification = decode_session_notification(&params).expect("complete settings snapshot");

    let SessionUpdate::ConfigOptionUpdate(settings) = notification.update else {
        panic!("expected a settings update");
    };
    assert_eq!(
        settings.meta.as_ref().and_then(|meta| meta.get("vendor")),
        Some(&json!({"enabled": true}))
    );
    let [selector, _] = settings.config_options.as_slice() else {
        panic!("expected selector followed by toggle");
    };
    assert_eq!(
        selector.category,
        Some(SessionConfigOptionCategory::Other(
            "_vendor_category".into()
        ))
    );
    let observed = observe_config_options(Some(&settings.config_options)).expect("options");
    let [selector, toggle] = observed.as_slice() else {
        panic!("expected selector followed by toggle");
    };
    assert_eq!(selector.id.0.as_ref(), " Vendor/Option ");
    assert_eq!(toggle.id.0.as_ref(), "toggle");
    assert_eq!(toggle.value, ObservedSettingValue::Boolean(false));
    let ObservedSettingValue::Select { current, offered } = &selector.value else {
        panic!("expected selector with choices");
    };
    assert_eq!(current.0.as_ref(), " Value/Current ");
    assert_eq!(
        offered
            .as_deref()
            .expect("offered values")
            .iter()
            .map(|choice| choice.value.0.as_ref())
            .collect::<Vec<_>>(),
        [" Value/B ", " Value/A "]
    );
}

#[test]
fn settings_reject_defaulted_fields_even_when_decoded_counts_match() {
    for option in [
        json!({"id": "toggle", "name": "Toggle", "type": "boolean", "currentValue": true, "category": {}}),
        json!({"id": "selector", "name": "Picker", "type": "select", "currentValue": "a", "options": [
            {"group": "group", "name": "Group", "options": {}}
        ]}),
    ] {
        let params = settings_notification(json!([option]), Value::Null);

        assert_eq!(
            decode_session_notification(&params),
            Err(SessionNotificationError::IncompleteSettingsOptions),
            "{option}"
        );
    }
}

#[test]
fn decorative_optional_fields_can_default_without_losing_settings() {
    let options = json!([{
        "id": "selector", "name": "Picker", "type": "select", "currentValue": "a",
        "category": "model", "description": false, "_meta": "invalid option metadata",
        "options": [{
            "group": "group", "name": "Group", "_meta": false,
            "options": [{"value": "a", "name": "A", "_meta": false, "description": false}]
        }]
    }]);
    let params = settings_notification(options, json!({"codexRouter": {"modelReasoning": 1}}));

    let decoded = decode_session_notification(&params).expect("decorative fields can default");
    let rendered = serde_json::to_value(decoded).expect("serializable notification");
    assert_eq!(
        rendered.get("update"),
        Some(&json!({
            "sessionUpdate": "config_option_update",
            "configOptions": [{
                "id": "selector", "name": "Picker", "type": "select", "currentValue": "a",
                "category": "model", "options": [{
                    "group": "group", "name": "Group", "options": [{"value": "a", "name": "A"}]
                }]
            }],
            "_meta": {"codexRouter": {"modelReasoning": 1}}
        }))
    );
}

#[test]
fn settings_reject_missing_options_and_malformed_snapshot_metadata() {
    let missing_options =
        json!({"sessionId": "session", "update": {"sessionUpdate": "config_option_update"}});
    let malformed_metadata = settings_notification(json!([]), json!(false));

    assert_eq!(
        decode_session_notification(&missing_options),
        Err(SessionNotificationError::InvalidSettingsArray)
    );
    assert_eq!(
        decode_session_notification(&malformed_metadata),
        Err(SessionNotificationError::InvalidSettingsMetadata)
    );
}

#[test]
fn settings_reject_group_rows_that_decode_as_flat_choices() {
    let flat_choice = json!({"value": "flat", "name": "Flat", "futureHint": "harmless"});
    let group = json!({
        "group": "group", "name": "Group",
        "options": [{"value": "nested", "name": "Nested"}],
        "futureHint": "harmless"
    });
    let snapshot = |choice| {
        settings_notification(
            json!([{
                "id": "selector", "name": "Picker", "type": "select", "currentValue": "nested",
                "options": [choice]
            }]),
            Value::Null,
        )
    };

    for (choice, expected) in [
        (flat_choice, json!({"value": "flat", "name": "Flat"})),
        (
            group.clone(),
            json!({
                "group": "group", "name": "Group", "options": [{"value": "nested", "name": "Nested"}]
            }),
        ),
    ] {
        let decoded = decode_session_notification(&snapshot(choice)).expect("complete select");
        let value = serde_json::to_value(decoded).expect("serializable notification");
        assert_eq!(
            value.pointer("/update/configOptions/0/options/0"),
            Some(&expected)
        );
    }

    let mut ambiguous = group;
    ambiguous["value"] = json!("group-as-flat-choice");
    assert_eq!(
        decode_session_notification(&snapshot(ambiguous)),
        Err(SessionNotificationError::IncompleteSettingsOptions)
    );
}

#[test]
fn positional_parameters_cannot_bypass_full_snapshot_classification() {
    let params = json!(["session", {
        "sessionUpdate": "config_option_update",
        "configOptions": {"mistaken": "empty"}
    }, null]);

    assert_eq!(
        decode_session_notification(&params),
        Err(SessionNotificationError::MalformedSettingsNotification)
    );
}

#[test]
fn omitted_session_options_are_distinct_from_reported_empty_options() {
    let omitted = NewSessionResponse::new("session");
    let empty = NewSessionResponse::new("session").config_options(Vec::new());

    assert_eq!(
        observe_config_options(omitted.config_options.as_deref()),
        None
    );
    assert_eq!(
        observe_config_options(empty.config_options.as_deref()),
        Some(Vec::new())
    );
}

#[test]
fn grouped_offers_keep_provider_order_and_do_not_add_the_selected_id() {
    let options = [SessionConfigOption::select(
        "Vendor/Custom",
        "Picker",
        "Current/Unlisted",
        vec![
            SessionConfigSelectGroup::new(
                "one",
                "First group",
                vec![
                    SessionConfigSelectOption::new("Choice/B", "B"),
                    SessionConfigSelectOption::new("Choice/A", "A"),
                ],
            ),
            SessionConfigSelectGroup::new(
                "two",
                "Second group",
                vec![SessionConfigSelectOption::new("Choice/C", "C")],
            ),
        ],
    )
    .category(SessionConfigOptionCategory::Other("_vendor_picker".into()))];

    let observed = observe_config_options(Some(&options)).expect("reported option set");

    assert_eq!(observed.len(), 1);
    let option = &observed[0];
    assert_eq!(option.id.0.as_ref(), "Vendor/Custom");
    assert_eq!(option.name, "Picker");
    assert_eq!(option.category, None);
    let ObservedSettingValue::Select { current, offered } = &option.value else {
        panic!("expected selected ID and choices");
    };
    assert_eq!(current.0.as_ref(), "Current/Unlisted");
    let offered = offered.as_deref().expect("reported choices");
    assert_eq!(
        offered
            .iter()
            .map(|choice| (choice.value.0.as_ref(), choice.name.as_str()))
            .collect::<Vec<_>>(),
        [("Choice/B", "B"), ("Choice/A", "A"), ("Choice/C", "C")]
    );
}

#[test]
fn cursor_thinking_boolean_and_effort_select_have_distinct_meanings() {
    for current in [false, true] {
        let options = [
            SessionConfigOption::boolean("thinking", "Thinking", current)
                .category(SessionConfigOptionCategory::ThoughtLevel),
            SessionConfigOption::select(
                "effort",
                "Effort",
                "Current/Only",
                Vec::<SessionConfigSelectOption>::new(),
            )
            .category(SessionConfigOptionCategory::ModelConfig),
        ];

        let observed = observe_config_options(Some(&options)).expect("reported option set");

        assert_eq!(observed.len(), 2);
        assert_eq!(observed[0].id.0.as_ref(), "thinking");
        assert_eq!(observed[0].category, None);
        assert_eq!(observed[0].value, ObservedSettingValue::Boolean(current));
        assert_eq!(observed[1].id.0.as_ref(), "effort");
        assert_eq!(observed[1].category, Some(SettingCategory::Effort));
        let ObservedSettingValue::Select { current, offered } = &observed[1].value else {
            panic!("expected selected ID and choices");
        };
        assert_eq!(current.0.as_ref(), "Current/Only");
        assert_eq!(offered.as_deref(), Some([].as_slice()));
    }
}

#[test]
fn fast_model_config_keeps_its_category_and_boolean_value() {
    let options = [SessionConfigOption::boolean("fast", "Fast", false)
        .category(SessionConfigOptionCategory::ModelConfig)];

    let observed = observe_config_options(Some(&options)).expect("reported option set");

    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].id.0.as_ref(), "fast");
    assert_eq!(observed[0].category, Some(SettingCategory::Fast));
    assert_eq!(observed[0].value, ObservedSettingValue::Boolean(false));
}
