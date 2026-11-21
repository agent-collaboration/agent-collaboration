//! A queued selection stays on its owner's exact skill catalogue through T2 admission.

use super::*;
use crate::session_owner_calls::InputAcceptance;
use collaboration_contracts::SessionEventHub;
use collaboration_runtime::{ProviderSessionEventHub, ProviderSessionRecord};
use session_event_model::{
    InputId, PromptContent, ProviderSubmission, SelectedSkill, SkillInvocationName,
};

#[tokio::test]
async fn selected_skill_removed_while_fifo_waits_is_refused_without_a_provider_prompt() {
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("wire.jsonl");
    let binding = binding();
    let generation = binding.generation.clone();
    let target = SessionRef {
        endpoint: binding.endpoint.clone(),
        session_id: SessionId::try_from("session-1".to_owned()).expect("fixture session ID"),
    };
    let store = Arc::new(Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("fixture store"),
    ));
    store
        .lock()
        .await
        .record_session(&ProviderSessionRecord {
            target: target.clone(),
            working_directory: ProviderWorkingDirectory::try_from(
                root.path().display().to_string(),
            )
            .expect("cwd"),
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
            created_by: requester().into(),
            approver: requester().into(),
            updated_at_ms: 0,
            resume: Default::default(),
        })
        .await
        .expect("saved setup");
    let hub = Arc::new(ProviderSessionEventHub::new(store.clone()));
    let board: message_board::SessionRef =
        collaboration_protocol::ProviderIdentity::session_to_board(&target).expect("board address");
    let runtime = ExternalProviderRuntime::initialize(
        ExternalProviderLaunch {
            executable: "node".into(),
            arguments: vec![
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../acp-client-runtime/tests/support/behavior-agent.ts"
                )
                .into(),
                receipt.display().to_string(),
                "remove-on-cancel".into(),
                "cursor_login".into(),
            ],
            environment: Vec::new(),
            persistence_target: acp_client_runtime::ProviderPersistenceTarget::Unspecified,
        },
        Arc::new(acp_provider_adapters::CursorAcpBehavior),
    )
    .await
    .expect("fixture provider");
    let supervisor = ExternalProviderSupervisor::new_with_hub(
        vec![ExternalProviderBinding {
            identity: binding.clone(),
            runtime,
        }],
        store.clone(),
        Some(hub.clone()),
        None,
    )
    .expect("Host supervisor");
    let opened = supervisor
        .open_provider_fixture(&target.endpoint, root.path().into())
        .await
        .expect("session/new");
    assert_eq!(opened, String::from(target.session_id.clone()));

    let selection = SelectedSkill {
        invocation_name: SkillInvocationName::new("review").expect("selected skill"),
    };
    let mut skills = hub
        .skill_view(board, root.path().to_owned())
        .await
        .expect("skill subscription");
    skills
        .receiver
        .wait_for(|view| view.contains_selection(&selection))
        .await
        .expect("provider advertised the selected skill");

    let contents = |operation_id: OperationId, input_id: InputId, text: &str, selected: bool| {
        ProviderPromptContentsRequest {
            operation_id,
            input_id,
            target: target.clone(),
            requested_by: requester().into(),
            approver: requester().into(),
            input: acp_client_runtime::ProviderTurnInput::Prompt(ProviderSubmission {
                content: vec![PromptContent::text(text.to_owned()).expect("prompt")],
                selected: selected.then(|| selection.clone()).into_iter().collect(),
            }),
            busy: PromptBusyPolicy::RefuseBusy,
        }
    };
    let wait = |operation_id| {
        supervisor.wait(ConversationOperationWaitRequest {
            operation_id,
            timeout_seconds: PositiveSeconds::try_from(5).expect("bound"),
        })
    };

    let first = OperationId::generate();
    assert_eq!(
        supervisor
            .submit_owned_direct_delivery(contents(
                first.clone(),
                InputId::generate(),
                "$review exact args",
                true
            ))
            .await
            .expect("first input"),
        InputAcceptance::Started
    );
    wait(first).await.expect("the initial skill runs");

    let running = OperationId::generate();
    assert_eq!(
        supervisor
            .submit_owned_direct_delivery(contents(
                running.clone(),
                InputId::generate(),
                "hold",
                false
            ))
            .await
            .expect("held turn"),
        InputAcceptance::Started
    );
    let queued = OperationId::generate();
    let queued_input = InputId::generate();
    let request = contents(
        queued.clone(),
        queued_input.clone(),
        "$review exact args",
        true,
    );
    let acp_client_runtime::ProviderTurnInput::Prompt(submission) = &request.input else {
        panic!("queued input is a prompt");
    };
    supervisor
        .queued_operation_registry()
        .record_queued_contents(
            queued.clone(),
            target.clone(),
            binding,
            queued_input,
            &submission.content,
        );
    assert_eq!(
        supervisor
            .submit_owned_delivery(request, collaboration_protocol::MessageDelivery::Queue)
            .await
            .expect("selected input waits on the owner"),
        InputAcceptance::Queued
    );

    let cancel = OperationId::generate();
    supervisor
        .cancel(ConversationCancelRequest {
            operation_id: cancel.clone(),
            target_operation_id: running,
            target,
            generation: Some(generation),
            requested_by: requester().into(),
            approver: requester().into(),
        })
        .await
        .expect("cancel admitted");
    wait(cancel).await.expect("cancel reaches the agent");
    let failure = wait(queued.clone())
        .await
        .expect_err("removed selection is known unsent");
    let not_sent = supervisor
        .queued_operation_registry()
        .wait_for_not_submitted(&queued)
        .await;
    let stored = store.lock().await.inspect(&queued).await.expect("row read");
    supervisor.shutdown().await.expect("fixture stops");

    assert_eq!(
        failure.kind,
        ConversationOperationFailureKind::InvalidRequest
    );
    assert_eq!(failure.effect, ProviderOperationEffect::None);
    assert!(
        stored.is_none(),
        "the stale selection was refused before the first dispatch mark"
    );
    assert!(matches!(not_sent,
        collaboration_protocol::ConversationOperationQueueState::NotSubmitted { reason }
        if reason == "invalid or stale skill selection"
    ));
    let packets: Vec<serde_json::Value> = std::fs::read_to_string(&receipt)
        .expect("wire receipt")
        .lines()
        .map(|line| serde_json::from_str(line).expect("wire JSON"))
        .collect();
    assert_eq!(
        packets.len(),
        2,
        "no stale selected prompt reached the provider"
    );
    assert_eq!(
        packets[0]["params"]["prompt"][0]["text"],
        "/review exact args"
    );
    assert_eq!(packets[1]["params"]["prompt"][0]["text"], "hold");
}
