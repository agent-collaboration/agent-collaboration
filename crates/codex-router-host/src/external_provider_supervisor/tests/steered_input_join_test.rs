//! D1 V22 (Lead 2026-11-16): a steered API prompt is taken into the running
//! turn under the input ID its request carried. The request comes in through
//! the supervisor's API backend (`ProviderConversationBackend::prompt`, what
//! the API service calls with the caller's request) with a chosen input ID; the
//! session's hub must record InputAccepted with exactly that ID.

use super::*;
use crate::provider_acp_message_fifo::ProviderAcpMessageFifo;
use collaboration_contracts::SessionEventHub as _;
use collaboration_runtime::ProviderSessionRecord;
use session_event_model::SessionEvent;

/// Oracle: D1 spec R12 and V22, "the operation's input ID equals the input ID
/// of the session event that took the message (InputAccepted or
/// TurnStarted)": a prompt sent while a turn runs, carrying input ID X, is
/// steered, and the hub's InputAccepted for it carries X, never a fresh ID.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn steered_prompt_is_accepted_under_its_requests_input_id() {
    // Arrange: a running turn on a steering agent, recorded on a real hub.
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("wire.json");
    let target = SessionRef {
        endpoint: endpoint(),
        session_id: SessionId::try_from("reactive-session-1".to_owned()).expect("session"),
    };
    let store = Arc::new(Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("store"),
    ));
    let hub = Arc::new(collaboration_runtime::ProviderSessionEventHub::new(
        Arc::clone(&store),
    ));
    let hub_target: message_board::SessionRef =
        serde_json::from_value(serde_json::to_value(&target).expect("target JSON"))
            .expect("hub target");
    let runtime = ExternalProviderRuntime::initialize_with_mcp_http_and_hub(
        crate::external_provider_runtime::acp_scripted_fixture::launch_fixture_agent(
            "cursor-settings-agent.ts",
            vec![
                "reactive".to_owned(),
                receipt.display().to_string(),
                "hold-first-prompt-open".to_owned(),
                "advertise-steering".to_owned(),
            ],
        ),
        acp_client_runtime::ProviderModelPicker::Standard,
        "fixture",
        "http://127.0.0.1:1/mcp",
        Arc::clone(&hub),
        hub_target.endpoint.clone(),
        Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await
    .expect("fixture provider");
    runtime
        .create_session(root.path().to_owned())
        .await
        .expect("session/new");
    store
        .lock()
        .await
        .record_session(&ProviderSessionRecord {
            target: target.clone(),
            working_directory: ProviderWorkingDirectory::try_from(
                root.path().display().to_string(),
            )
            .expect("working directory"),
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
            created_by: target.clone().into(),
            approver: target.clone().into(),
            updated_at_ms: 1,
        })
        .await
        .expect("session record");
    let supervisor = Arc::new(
        ExternalProviderSupervisor::new(
            vec![ExternalProviderBinding {
                identity: binding(),
                runtime,
            }],
            Arc::clone(&store),
        )
        .expect("supervisor"),
    );
    let queue = Arc::new(ProviderAcpMessageFifo::new(
        Arc::clone(&supervisor),
        Arc::clone(&store),
        Arc::new(NoLivePeer),
    ));
    supervisor.attach_message_fifo(&queue);
    let runtime = supervisor.runtime_for(&target.endpoint).expect("runtime");
    let held_context = crate::ExternalProviderApprovalContext {
        requester: target.clone().into(),
        approver: target.clone().into(),
        target: target.clone(),
        operation_id: OperationId::generate(),
        binding_generation: binding().generation,
        binding_retirement: runtime.retirement(),
    };
    let held = tokio::spawn(async move {
        runtime
            .prompt_with_approval_context(
                "reactive-session-1".to_owned(),
                "first".to_owned(),
                held_context,
            )
            .await
    });
    while recorded_requests(&receipt, "session/prompt") < 1 {
        tokio::task::yield_now().await;
    }

    // Act: the caller's request names its own input ID.
    let chosen = session_event_model::InputId::generate();
    let steered = supervisor
        .prompt(ConversationPromptRequest {
            operation_id: OperationId::generate(),
            input_id: Some(chosen.clone()),
            target: target.clone(),
            generation: None,
            requested_by: target.clone().into(),
            approver: target.clone().into(),
            prompt: collaboration_protocol::MessageContent::HumanUser {
                text: MessageText::try_from("steer me".to_owned()).expect("prompt"),
            },
        })
        .await;
    let accepted: Vec<session_event_model::InputId> = hub
        .attach(hub_target.clone())
        .await
        .expect("hub attachment")
        .snapshot
        .into_iter()
        .filter_map(|event| match event.event {
            SessionEvent::InputAccepted { input_id, .. } => Some(input_id),
            _ => None,
        })
        .collect();
    std::fs::write(format!("{}.release", receipt.display()), b"").expect("release the turn");
    let held = held.await.expect("held turn task");
    queue.shutdown().await;
    let steers = recorded_requests(&receipt, "_session/steering");
    let stopped = supervisor.shutdown().await;

    // Assert: steered once, and accepted under the request's own input ID.
    assert!(steered.is_ok(), "the prompt was taken: {steered:?}");
    assert_eq!(steers, 1, "one steer reached the agent");
    assert_eq!(
        accepted,
        vec![chosen],
        "the running turn took it under the request's input ID"
    );
    assert!(held.is_ok(), "the held turn ran to its end: {held:?}");
    stopped.expect("supervisor shutdown");
}

/// Requests with `method` the fixture read, from its receipt.
fn recorded_requests(receipt: &std::path::Path, method: &str) -> usize {
    std::fs::read(receipt)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<serde_json::Value>>(&bytes).ok())
        .map_or(0, |requests| {
            requests
                .iter()
                .filter(|request| {
                    request.get("method").and_then(serde_json::Value::as_str) == Some(method)
                })
                .count()
        })
}
