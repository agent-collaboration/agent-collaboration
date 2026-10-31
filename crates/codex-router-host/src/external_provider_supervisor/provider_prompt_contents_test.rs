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
