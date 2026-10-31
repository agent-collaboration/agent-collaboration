use super::*;

#[test]
fn unknown_stop_reason_keeps_output_and_safe_value() {
    let outcome = acp_client_runtime::ExternalProviderPromptOutcome {
        output: "known output".to_owned(),
        stop_reason: session_event_model::StopReason::Unknown("future_stop".to_owned()),
        permission_refusal_reason: None,
    };
    let projected = host_prompt_outcome(outcome).expect("unknown reason is a terminal outcome");
    assert_eq!(projected.output, "known output");
    assert_eq!(
        projected.stop_reason,
        ProviderPromptStopReason::Unknown("future_stop".into())
    );

    let untrusted = acp_client_runtime::ExternalProviderPromptOutcome {
        output: String::new(),
        stop_reason: session_event_model::StopReason::Unknown("../token=value".to_owned()),
        permission_refusal_reason: None,
    };
    assert_eq!(
        host_prompt_outcome(untrusted)
            .expect("terminal outcome")
            .stop_reason,
        ProviderPromptStopReason::Unknown("unrecognized".into())
    );
}
