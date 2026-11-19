use super::ProviderPromptContentsRequest;
use collaboration_protocol::{
    ConversationPromptRequest, MessageContent, MessageText, OperationId, SessionRef,
};
use session_event_model::PromptContent;

#[test]
fn prompt_request_forwards_exact_task_text_without_router_envelope() {
    let target: SessionRef = serde_json::from_value(serde_json::json!({
        "endpoint":{"serviceId":"018f47d2-24d5-7a68-b9ec-6f759c39458f","endpointId":"codex-local"},
        "sessionId":"target-session"
    }))
    .expect("target session");
    let sender: SessionRef = serde_json::from_value(serde_json::json!({
        "endpoint":{"serviceId":"018f47d2-24d5-7a68-b9ec-6f759c39458f","endpointId":"claude-local"},
        "sessionId":"sender-session"
    }))
    .expect("sender session");
    let request = ConversationPromptRequest {
        delivery: collaboration_protocol::MessageDelivery::Auto,
        operation_id: OperationId::generate(),
        input_id: None,
        target,
        generation: None,
        requested_by: sender.clone().into(),
        approver: sender.clone().into(),
        prompt: MessageContent::Agent {
            sender,
            text: MessageText::try_from("Check the provider prompt.".to_owned())
                .expect("prompt text"),
        },
    };

    let provider_request = ProviderPromptContentsRequest::from_prompt_request(request)
        .expect("provider prompt contents");
    let acp_client_runtime::ProviderTurnInput::Prompt(submission) = provider_request.input else {
        panic!("ordinary prompt input");
    };
    let Some(PromptContent::Text { text }) = submission.content.first() else {
        panic!("one text prompt should be generated");
    };
    assert_eq!(text.as_str(), "Check the provider prompt.");
    assert!(!text.as_str().contains("Agent communication"));
    assert!(!text.as_str().contains("Self-declared sender:"));
}

/// Oracle: M8 keeps each refusal's kind and reports the session's own
/// answer (state plus valid next steps), never a bare "busy"; a dispatch
/// refusal and a runtime error carrying the same answer say the same thing.
#[test]
fn session_refusals_report_their_answer_and_keep_their_kind() {
    use acp_client_runtime::{ExternalProviderRuntimeError, ProviderDispatchRefusal};
    use collaboration_protocol::ConversationOperationFailureKind as Kind;
    // Arrange
    let target: SessionRef = serde_json::from_value(serde_json::json!({
        "endpoint":{"serviceId":"018f47d2-24d5-7a68-b9ec-6f759c39458f","endpointId":"claude-local"},
        "sessionId":"target-session"
    }))
    .expect("target session");
    let busy = "a turn is running; queue the message or cancel the turn";
    let held = "a setting change's outcome is unknown; set a value explicitly";
    let not_loaded = "session is not loaded; load it or resume it";

    // Act
    let dispatched = [
        ProviderDispatchRefusal::Busy {
            answer: busy.to_owned(),
        },
        ProviderDispatchRefusal::SettingsUnresolved {
            answer: held.to_owned(),
        },
        ProviderDispatchRefusal::NotLoaded {
            answer: not_loaded.to_owned(),
        },
    ]
    .map(|refusal| {
        let failure =
            super::dispatch_refusal_failure(refusal, OperationId::generate(), target.clone());
        (failure.kind, String::from(failure.message))
    });
    let returned = [
        ExternalProviderRuntimeError::LocalBusy {
            answer: busy.to_owned(),
        },
        ExternalProviderRuntimeError::SettingsUnresolved {
            answer: held.to_owned(),
        },
    ]
    .map(|error| {
        let failure = super::super::runtime_failure(OperationId::generate(), None, error);
        (failure.kind, String::from(failure.message))
    });

    // Assert
    assert_eq!(
        dispatched,
        [
            (Kind::Busy, busy.to_owned()),
            (Kind::SettingsUnresolved, held.to_owned()),
            (Kind::Unavailable, not_loaded.to_owned()),
        ]
    );
    assert_eq!(
        returned,
        [
            (Kind::Busy, busy.to_owned()),
            (Kind::SettingsUnresolved, held.to_owned()),
        ]
    );
}
