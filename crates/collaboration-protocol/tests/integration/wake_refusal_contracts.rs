//! Every wake refusal crosses the existing error envelope without losing its own kind.
use collaboration_protocol::WakeFailure;
use serde_json::json;

#[test]
fn wake_refusals_round_trip_and_validate_against_the_published_schema() {
    let schema = serde_json::to_value(schemars::schema_for!(WakeFailure)).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    for reason in [
        json!({"kind":"wakeExists","wakeup":"01900000-0000-7000-8000-000000000001","creator":{"kind":"unrecorded"}}),
        json!({"kind":"tooFarAhead","max":86400}),
        json!({"kind":"intervalTooShort","min":300}),
        json!({"kind":"lifetimeTooLong","max":604800}),
        json!({"kind":"expiryRequired"}),
    ] {
        let mut wire = json!({"stage":"admission","message":"Wake refused","operationId":null,"wakeupId":null,"effects":{"kind":"local","mutation":"none"},"nextAction":"correctRequest"});
        wire.as_object_mut()
            .unwrap()
            .extend(reason.as_object().unwrap().clone());
        assert!(
            validator.is_valid(&wire),
            "schema omitted refusal: {wire}; {:?}",
            validator
                .iter_errors(&wire)
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
        );
        let parsed: WakeFailure = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), wire);
        let mut extra = wire.clone();
        extra["extra"] = json!(true);
        assert!(serde_json::from_value::<WakeFailure>(extra).is_err());
        let mut irrelevant = wire.clone();
        irrelevant["field"] = serde_json::Value::Null;
        assert!(
            serde_json::from_value::<WakeFailure>(irrelevant).is_err(),
            "irrelevant null field was discarded"
        );
        wire["field"] = json!("irrelevant");
        assert!(serde_json::from_value::<WakeFailure>(wire).is_err());
    }
}
