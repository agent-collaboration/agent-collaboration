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

#[test]
fn published_utf8_byte_bounds_match_multibyte_entry_and_stored_decode() {
    fn boundary<T>(name: &str, maximum: usize)
    where
        T: schemars::JsonSchema + serde::de::DeserializeOwned + std::fmt::Debug,
    {
        // Literal four-byte characters distinguish the byte contract from maxLength.
        let accepted = "🦁".repeat(maximum / 4);
        let refused = format!("{accepted}🦁");
        assert_eq!(accepted.len(), maximum);
        assert_eq!(refused.len(), maximum + 4);
        let schema = serde_json::to_value(schemars::schema_for!(T)).unwrap();
        assert_eq!(schema["x-maxUtf8Bytes"], maximum, "{name}");
        let validator = jsonschema::validator_for(&schema).unwrap();
        assert!(
            validator.is_valid(&json!(accepted)),
            "{name}: schema must not be stricter than entry"
        );
        assert!(
            serde_json::from_value::<T>(json!(accepted)).is_ok(),
            "{name}: exact bytes accepted"
        );
        let error = serde_json::from_value::<T>(json!(refused))
            .unwrap_err()
            .to_string();
        assert!(error.contains("limitExceeded"), "{name}: {error}");
        assert!(error.contains(name), "{name}: {error}");
        assert!(
            error.contains(&(maximum + 4).to_string()),
            "{name}: actual byte count: {error}"
        );
        let description = schema["description"].as_str().unwrap_or("");
        assert!(
            description.contains("UTF-8 bytes"),
            "{name}: published description must name bytes"
        );
        assert!(
            description.contains(&maximum.to_string()),
            "{name}: published description must state its bound"
        );
    }

    boundary::<InstructionText>("INSTRUCTION_MAX_BYTES", 1_048_576);
    boundary::<MessageText>("WAKE_MESSAGE_MAX_BYTES", 1_048_576);
    boundary::<SessionId>("SESSION_ID_MAX_BYTES", 4096);
    boundary::<NonEmptyText>("TEXT_MAX_BYTES", 4096);

    // Endpoint IDs admit ASCII only, so character and byte counts coincide.
    let schema = serde_json::to_value(schemars::schema_for!(EndpointId)).unwrap();
    assert_eq!(schema["x-maxUtf8Bytes"], 64);
    assert!(
        schema["description"]
            .as_str()
            .unwrap()
            .contains("64 ASCII bytes")
    );
    let validator = jsonschema::validator_for(&schema).unwrap();
    for (text, accepted) in [
        ("x".repeat(64), true),
        ("x".repeat(65), false),
        ("🦁".repeat(16), false),
    ] {
        assert_eq!(validator.is_valid(&json!(text)), accepted);
        assert_eq!(
            serde_json::from_value::<EndpointId>(json!(text)).is_ok(),
            accepted
        );
    }
}
