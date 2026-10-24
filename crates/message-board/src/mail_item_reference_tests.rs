use super::*;
use crate::IntentState;

#[test]
fn project_mail_reference_preserves_the_frozen_uuid_without_minting_one() {
    let original = "01900000-0000-7000-8000-000000000001";
    let reference: MailItemRef = serde_json::from_value(serde_json::json!(original)).unwrap();
    assert_eq!(reference.as_str(), original);
    assert_eq!(
        serde_json::to_value(reference).unwrap(),
        serde_json::json!(original)
    );
}

#[test]
fn stored_project_mail_references_refuse_noncanonical_or_non_v7_identities() {
    for invalid in [
        "",
        "not-an-item",
        "550e8400-e29b-41d4-a716-446655440000",
        "019ABCDE-0000-7000-8000-000000000001",
        " 01900000-0000-7000-8000-000000000001",
    ] {
        assert!(
            serde_json::from_value::<MailItemRef>(serde_json::json!(invalid)).is_err(),
            "accepted {invalid:?}"
        );
    }
}

#[test]
fn project_mail_reference_schema_keeps_uuid_v7_bounds_and_a_distinct_definition_name() {
    let schema = serde_json::to_value(schemars::schema_for!(MailItemRef)).unwrap();
    assert_eq!(schema["title"], "ProjectMailItemRef");
    assert_eq!(schema["minLength"], 36);
    assert_eq!(schema["maxLength"], 36);
    assert_eq!(
        schema["pattern"],
        "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"
    );
}

#[test]
fn handed_off_intents_require_the_exact_canonical_item_reference() {
    let id = "01900000-0000-7000-8000-000000000001";
    let state: IntentState =
        serde_json::from_value(serde_json::json!({"kind":"handedOff","item":id})).unwrap();
    assert_eq!(
        serde_json::to_value(state).unwrap(),
        serde_json::json!({"kind":"handedOff","item":id})
    );
    for invalid in [
        serde_json::json!({"kind":"handedOff"}),
        serde_json::json!({"kind":"handedOff","item":"550e8400-e29b-41d4-a716-446655440000"}),
    ] {
        assert!(serde_json::from_value::<IntentState>(invalid).is_err());
    }
}
