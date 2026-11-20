use serde_json::{Value, json};
use vendor_protocol::codex::{
    CodexRequest, InputModality, Model, ModelListParams, ModelListResponse,
};

type ModelListReply = <ModelListParams as CodexRequest>::Response;

fn required_model_fields() -> Value {
    // The required fields of #/definitions/v2/Model in the pinned experimental export.
    json!({
        "id": "sample-id",
        "model": "sample-model",
        "displayName": "Sample",
        "description": "A schema-derived sample",
        "hidden": false,
        "isDefault": true,
        "supportedReasoningEfforts": [
            { "reasoningEffort": "Provider/New-Éffort", "description": "A provider-owned value" }
        ],
        "defaultReasoningEffort": "Provider/New-Éffort"
    })
}

#[test]
fn request_maps_omitted_null_and_present_fields_to_distinct_wire_values() {
    assert_eq!(ModelListParams::METHOD, "model/list");
    let cases = [
        (json!({}), None, None, None),
        (
            json!({"cursor": null, "includeHidden": null, "limit": null}),
            Some(None),
            Some(None),
            Some(None),
        ),
        (
            json!({"cursor": "opaque/+=", "includeHidden": false, "limit": 0}),
            Some(Some("opaque/+=")),
            Some(Some(false)),
            Some(Some(0)),
        ),
    ];

    for (wire, cursor, hidden, limit) in cases {
        let params: ModelListParams =
            serde_json::from_value(wire.clone()).expect("valid request parameters");

        assert_eq!(params.cursor.as_ref().map(|value| value.as_deref()), cursor);
        assert_eq!(params.include_hidden, hidden);
        assert_eq!(params.limit, limit);
        assert_eq!(
            serde_json::to_value(&params).expect("encoded request parameters"),
            wire
        );
    }
    let params: ModelListParams =
        serde_json::from_value(json!({"limit": 4294967295_u32, "futureFlag": true}))
            .expect("maximum limit and an additive flag");
    assert_eq!(params.limit, Some(Some(u32::MAX)));
    assert_eq!(
        serde_json::to_value(params).expect("encoded request parameters"),
        json!({"limit": 4294967295_u32})
    );
}

#[test]
fn response_preserves_known_fields_and_ignores_additive_properties() {
    let required = required_model_fields();
    let mut wire_model = required.clone();
    wire_model["availabilityNux"] = json!({"message": "Available", "extra": []});
    wire_model["upgradeInfo"] =
        json!({"model": "next-model", "retirementAt": i64::MAX, "migrationMarkdown": null});
    wire_model["availableAccessPrograms"] = json!({"cyber": ["daybreakBlue"]});
    wire_model["multiAgentVersion"] = json!("v2");
    wire_model["inputModalities"] = json!(["audio", "text"]);
    wire_model["futureModelProperty"] = json!("optional");

    let decoded: ModelListReply = serde_json::from_value(json!({
        "data": [wire_model.clone()],
        "nextCursor": "more",
        "futureResultProperty": {}
    }))
    .expect("valid response with additive properties");
    let decoded_model = decoded.data.first().expect("one model in the response");

    assert_eq!(decoded.next_cursor, Some(Some("more".into())));
    assert_eq!(
        decoded_model.default_reasoning_effort.as_str(),
        "Provider/New-Éffort"
    );
    assert_eq!(
        decoded_model.input_modalities(),
        [InputModality::Audio, InputModality::Text]
    );
    let mut expected_model = required;
    expected_model["availabilityNux"] = json!({"message": "Available"});
    for key in [
        "upgradeInfo",
        "availableAccessPrograms",
        "multiAgentVersion",
        "inputModalities",
    ] {
        expected_model[key] = wire_model[key].clone();
    }
    assert_eq!(
        serde_json::to_value(decoded).expect("encoded response"),
        json!({"data": [expected_model], "nextCursor": "more"})
    );
}

#[test]
fn omitted_defaults_are_effective_without_changing_the_wire_presence() {
    let required = required_model_fields();
    let omitted: Model =
        serde_json::from_value(required.clone()).expect("model with omitted defaults");
    assert_eq!(omitted.input_modalities, None);
    assert_eq!(
        omitted.input_modalities(),
        [InputModality::Text, InputModality::Image]
    );
    assert!(omitted.service_tiers().is_empty());
    assert!(omitted.additional_speed_tiers().is_empty());
    assert!(!omitted.supports_personality());
    assert_eq!(omitted.available_access_programs(), None);
    assert_eq!(
        serde_json::to_value(&omitted).expect("encoded model"),
        required
    );

    let mut explicit = required;
    explicit["inputModalities"] = json!([]);
    explicit["serviceTiers"] = json!([]);
    explicit["additionalSpeedTiers"] = json!([]);
    explicit["supportsPersonality"] = json!(false);
    explicit["availableAccessPrograms"] = Value::Null;
    explicit["defaultServiceTier"] = Value::Null;
    explicit["modelSpecialty"] = Value::Null;
    let provided: Model =
        serde_json::from_value(explicit.clone()).expect("model with explicit defaults");
    assert_eq!(provided.input_modalities, Some(vec![]));
    assert!(provided.input_modalities().is_empty());
    assert_eq!(provided.supports_personality, Some(false));
    assert_eq!(provided.available_access_programs, Some(None));
    assert_eq!(
        serde_json::to_value(provided).expect("encoded model"),
        explicit
    );

    for (wire, expected) in [
        (json!({"data": []}), None),
        (json!({"data": [], "nextCursor": null}), Some(None)),
    ] {
        let response: ModelListResponse =
            serde_json::from_value(wire.clone()).expect("valid response");
        assert_eq!(response.next_cursor, expected);
        assert_eq!(
            serde_json::to_value(response).expect("encoded response"),
            wire
        );
    }
}

#[test]
fn wire_types_reject_invalid_roots_known_fields_and_closed_values() {
    for invalid in [
        "null",
        "[]",
        "[null, false, 1]",
        "true",
        r#"{"limit":4294967296}"#,
        r#"{"limit":-1}"#,
        r#"{"limit":1.5}"#,
        r#"{"includeHidden":"false"}"#,
    ] {
        assert!(
            serde_json::from_str::<ModelListParams>(invalid).is_err(),
            "{invalid}"
        );
    }
    for invalid in ["null", "[[]]", "{}", r#"{"data":null}"#, r#"{"data":[[]]}"#] {
        assert!(
            serde_json::from_str::<ModelListReply>(invalid).is_err(),
            "{invalid}"
        );
    }

    for (field, invalid) in [
        ("defaultReasoningEffort", json!("")),
        ("defaultReasoningEffort", json!(3)),
        (
            "supportedReasoningEfforts",
            json!([{"reasoningEffort": "", "description": "Empty"}]),
        ),
        ("hidden", json!("false")),
        ("serviceTiers", json!([{"name": "missing required fields"}])),
        ("inputModalities", Value::Null),
        ("inputModalities", json!(["video"])),
        ("multiAgentVersion", json!("V2")),
        (
            "availableAccessPrograms",
            json!({"cyber": ["unrecognized"]}),
        ),
        ("availabilityNux", json!(["not a named object"])),
    ] {
        let mut invalid_model = required_model_fields();
        invalid_model[field] = invalid;
        let response = json!({"data": [invalid_model]});
        assert!(
            serde_json::from_value::<ModelListReply>(response).is_err(),
            "{field}"
        );
    }
}

#[test]
fn wrong_model_list_types_fail_to_compile() {
    let examples = trybuild::TestCases::new();
    examples.compile_fail("tests/integration/codex_model_list/invalid_params.rs");
    examples.compile_fail("tests/integration/codex_model_list/invalid_response.rs");
}
