use super::*;
use collaboration_protocol::{MessageContent, MessageDelivery};

#[tokio::test]
async fn prompt_request_forwards_exact_task_text_without_router_envelope() {
    let rig = steering_session_on_clock(
        "cursor-settings-agent.ts",
        "reactive-session-1",
        |receipt| vec!["reactive".to_owned(), receipt.display().to_string()],
        Arc::new(router_clock::ManualClock::new(chrono::DateTime::UNIX_EPOCH)),
    )
    .await;
    let sender = SessionRef {
        endpoint: rig.target.endpoint.clone(),
        session_id: SessionId::try_from("task-sender".to_owned()).expect("requester ID"),
    };
    let text = "Check the provider prompt.";
    let operation_id = OperationId::generate();
    rig.supervisor
        .prompt(ConversationPromptRequest {
            delivery: MessageDelivery::Auto,
            operation_id: operation_id.clone(),
            input_id: None,
            target: rig.target.clone(),
            generation: Some(binding().generation),
            requested_by: sender.clone().into(),
            approver: rig.target.clone().into(),
            prompt: MessageContent::Agent {
                sender,
                text: MessageText::try_from(text.to_owned()).expect("task text"),
            },
        })
        .await
        .expect("request through the owning prompt path");
    rig.supervisor
        .wait(ConversationOperationWaitRequest {
            operation_id,
            timeout_seconds: PositiveSeconds::try_from(10).expect("wait budget"),
        })
        .await
        .expect("wire prompt settles");
    let requests: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(&rig.receipt).expect("ACP receipt"))
            .expect("recorded requests");
    let prompts: Vec<_> = requests
        .iter()
        .filter(|request| request["method"] == "session/prompt")
        .collect();
    assert_eq!(prompts.len(), 1);
    let content = prompts[0]["params"]["prompt"]
        .as_array()
        .expect("prompt content");
    assert_eq!(content.len(), 1);
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[0]["text"], text);
    rig.supervisor.shutdown().await.expect("fixture shutdown");
}
