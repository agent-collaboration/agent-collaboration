use serde_json::json;

#[test]
fn message_result_schema_preserves_closed_rejection_diagnostics() {
    let schema = serde_json::to_value(schemars::schema_for!(
        collaboration_protocol::DeliveryReceipt
    ))
    .unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let mut response = json!({
        "outcome":{"kind":"rejected","reason":"childThread","nextAction":"inspectTarget","clientCode":-32000,"detail":null},
        "reachability":"codexAppServer","client":null
    });
    assert!(validator.is_valid(&response));
    response["outcome"]
        .as_object_mut()
        .unwrap()
        .remove("nextAction");
    assert!(!validator.is_valid(&response));
    response["outcome"]["nextAction"] = json!("inspectTarget");
    response["outcome"]["reason"] = json!("inventedReason");
    assert!(!validator.is_valid(&response));
}
