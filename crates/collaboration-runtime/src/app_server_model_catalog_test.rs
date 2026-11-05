use super::*;

#[test]
fn bootstrap_delegates_to_provider_without_fabricated_effort() {
    // Program Design Native: bootstrap is an action, never a measured medium.
    let response = render_model_list(&[]);
    assert_eq!(response["data"][0]["id"], "provider-default");
    assert_eq!(response["data"][0]["defaultReasoningEffort"], "provider");
    assert_eq!(
        response["data"][0]["supportedReasoningEfforts"],
        json!([
            {"reasoningEffort":"provider", "description":"Use provider settings"}
        ])
    );
    assert_eq!(response["data"][0]["displayName"], "Use provider settings");
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; invalid fixture constructors propagate."
)]
fn all_models_keep_independent_defaults_and_reserved_collisions_delegate()
-> Result<(), InvalidProviderModelEntry> {
    let first = ProviderModelEntry::try_new("a".into(), "A".into(), String::new())?
        .with_reasoning(
            vec![ProviderReasoningChoice {
                value: "low".into(),
                label: "Low".into(),
            }],
            "low".into(),
        )?;
    let unknown = ProviderModelEntry::try_new("b".into(), "B".into(), String::new())?;
    let collision = ProviderModelEntry::try_new("c".into(), "C".into(), String::new())?
        .with_reasoning(
            vec![ProviderReasoningChoice {
                value: "provider".into(),
                label: "Vendor level".into(),
            }],
            "provider".into(),
        )?;
    let response = render_model_list(&[first, unknown, collision]);
    assert_eq!(response["data"].as_array().map(Vec::len), Some(3));
    assert_eq!(response["data"][0]["defaultReasoningEffort"], "low");
    assert_eq!(response["data"][1]["defaultReasoningEffort"], "provider");
    assert_eq!(
        response["data"][2]["supportedReasoningEfforts"][0]["description"],
        "Use provider settings"
    );
    Ok(())
}
