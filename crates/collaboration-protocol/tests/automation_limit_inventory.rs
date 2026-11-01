use collaboration_protocol::{
    EndpointId, InstructionText, MessageText, NonEmptyText, SessionId,
    limits::{self, AutomationLimits, INVENTORY},
};
use serde_json::json;

#[test]
fn every_product_limit_has_its_reviewed_bounds_in_schema_validator_and_description() {
    // R5 plus the existing identity bounds. Literals are independent of the production inventory.
    let expected = [
        ("GRID_STEP", 300, 300, 300),
        ("WAKE_MIN_PERIOD", 600, 31_536_000, 300),
        ("WAKE_ONE_SHOT_MAX_AHEAD", 0, 86_400, 1),
        ("WAKE_REPEAT_MAX_LIFETIME", 1, 604_800, 1),
        ("SCHEDULE_MIN_INTERVAL", 300, 31_536_000, 300),
        ("NOTIFICATION_INTERVAL", 600, 600, 1),
        ("NOTIFICATION_LISTING_MAX_ENTRIES", 1, 50, 1),
        ("BATCH_MAX_ENTRIES", 1, 100, 1),
        ("REPORT_FLUSH_MAX", 1, 100, 1),
        ("REPORT_FLUSH_WINDOW", 500, 500, 1),
        ("INSTRUCTION_MAX_BYTES", 1, 1_048_576, 1),
        ("WAKE_MESSAGE_MAX_BYTES", 1, 1_048_576, 1),
        ("ENDPOINT_ID_MAX_BYTES", 1, 64, 1),
        ("SESSION_ID_MAX_BYTES", 1, 4096, 1),
        ("TEXT_MAX_BYTES", 1, 4096, 1),
        ("WAKE_SLOT", 1, 1, 1),
        ("JSON_SAFE_INTEGER", 1, 9_007_199_254_740_991, 1),
    ];
    let published = serde_json::to_value(schemars::schema_for!(AutomationLimits)).unwrap();
    assert_eq!(INVENTORY.len(), expected.len());
    for (limit, (name, minimum, maximum, step)) in INVENTORY.iter().zip(expected) {
        assert_eq!(limit.name.definition(), *limit);
        assert_eq!(limit.name.as_str(), name);
        assert_eq!(serde_json::to_value(limit.name).unwrap(), name);
        assert_eq!(
            (limit.minimum, limit.maximum, limit.step),
            (minimum, maximum, step)
        );
        let schema = &published["properties"][name];
        assert_eq!(schema["minimum"], minimum);
        assert_eq!(schema["maximum"], maximum);
        assert_eq!(schema["multipleOf"], step);
        let validator = jsonschema::validator_for(schema).unwrap();
        for value in [minimum, maximum, maximum + 1] {
            assert_eq!(
                validator.is_valid(&json!(value)),
                limit.validate(value).is_ok(),
                "{name}: {value}"
            );
        }
        assert!(
            limits::description().contains(&format!("{name}: {minimum}..={maximum}, step {step}"))
        );
        let error = limit.validate(maximum + 1).unwrap_err();
        assert_eq!(error.value, maximum + 1);
        assert_eq!(error.limit, limit.name);
    }
}

#[test]
fn identity_and_text_decoders_use_the_same_byte_bounds_as_their_schemas() {
    fn schema_max<T: schemars::JsonSchema>() -> u64 {
        serde_json::to_value(schemars::schema_for!(T)).unwrap()["maxLength"]
            .as_u64()
            .unwrap()
    }
    assert_eq!(schema_max::<InstructionText>(), 1_048_576);
    assert_eq!(schema_max::<MessageText>(), 1_048_576);
    assert_eq!(schema_max::<SessionId>(), 4096);
    assert_eq!(schema_max::<EndpointId>(), 64);
    assert_eq!(schema_max::<NonEmptyText>(), 4096);
    assert!(InstructionText::try_from("x".repeat(1_048_576)).is_ok());
    assert!(MessageText::try_from("x".repeat(1_048_576)).is_ok());
    assert!(SessionId::try_from("x".repeat(4096)).is_ok());
    assert!(EndpointId::try_from("x".repeat(64)).is_ok());
    assert!(NonEmptyText::try_from("x".repeat(4096)).is_ok());
    let corrupt = [
        serde_json::from_value::<InstructionText>(json!("x".repeat(1_048_577)))
            .unwrap_err()
            .to_string(),
        serde_json::from_value::<MessageText>(json!("x".repeat(1_048_577)))
            .unwrap_err()
            .to_string(),
        serde_json::from_value::<SessionId>(json!("x".repeat(4097)))
            .unwrap_err()
            .to_string(),
        serde_json::from_value::<EndpointId>(json!("x".repeat(65)))
            .unwrap_err()
            .to_string(),
        serde_json::from_value::<NonEmptyText>(json!("x".repeat(4097)))
            .unwrap_err()
            .to_string(),
    ];
    for error in corrupt {
        assert!(error.contains("limitExceeded"), "{error}");
    }
    assert!(
        MessageText::try_from("🦁".repeat(262_145)).is_err(),
        "UTF-8 bytes, not scalar count"
    );
}
