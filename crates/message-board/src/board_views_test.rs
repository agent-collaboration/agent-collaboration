use super::CurrentWriter;

#[test]
fn central_missing_status_keeps_the_cutover_import_wording() {
    // Arrange the approved public view value, independently of its Rust variant.
    let wire = serde_json::json!({"kind":"notInCentral"});
    // Act through the same DTO and generated schema used by CLI/MCP consumers.
    let status = serde_json::from_value::<CurrentWriter>(wire.clone())
        .expect("central-missing view decodes");
    let schema = serde_json::to_value(schemars::schema_for!(CurrentWriter)).expect("writer schema");
    // Assert the distinct state survives output and describes the pending decision.
    assert_eq!(serde_json::to_value(status).expect("writer output"), wire);
    assert!(
        schema
            .to_string()
            .contains("not in central yet (run the cutover import)")
    );
}
