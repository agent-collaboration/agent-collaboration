//! A queued prompt's mark follows the preceding terminal write in the
//! session store lane (T5).

#![expect(
    clippy::expect_used,
    reason = "A fixture prompt that cannot be built must fail the scenario at once."
)]

use super::provider_acp_message_fifo_support::{provider_binding, recorded_requests, target};
use super::provider_fixture_launch;
use codex_router_host::{
    ExternalProviderBinding, ExternalProviderRuntime, ExternalProviderSupervisor,
};
use collaboration_contracts::ProviderConversationBackend;
use collaboration_protocol::{
    ConversationOperationSettlement, ConversationOperationShowRequest,
    ConversationOperationWaitOutput, ConversationOperationWaitRequest, ConversationPromptRequest,
    MessageContent, MessageDelivery, MessageText, OperationId, PositiveSeconds,
    ProviderOperationStage, ProviderRequestedPolicy, ProviderWorkingDirectory, RouterAccess,
    SessionId, SessionRef,
};
use collaboration_runtime::{ProviderOperationStore, ProviderSessionRecord};
use std::sync::Arc;
use tokio::sync::Mutex;

fn prompt_request(
    delivery: MessageDelivery,
    target: &SessionRef,
    operation_id: &OperationId,
    text: &str,
) -> ConversationPromptRequest {
    ConversationPromptRequest {
        delivery,
        operation_id: operation_id.clone(),
        input_id: None,
        target: target.clone(),
        generation: None,
        requested_by: target.clone().into(),
        approver: target.clone().into(),
        prompt: MessageContent::HumanUser {
            text: MessageText::try_from(text.to_owned()).expect("prompt"),
        },
    }
}

/// T5. The owner offers a queued input's mark while the store lane holds
/// the previous terminal write. It can authorize the input only after the
/// terminal write and mark commit in order.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_queued_message_commits_only_after_the_previous_settlement_is_written() {
    // Arrange
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("wire.json");
    let mut target = target();
    target.session_id = SessionId::try_from("reactive-session-1".to_owned()).expect("session ID");
    let binding = provider_binding(&target);
    let runtime = ExternalProviderRuntime::initialize(
        provider_fixture_launch::launch(
            "cursor-settings-agent.ts",
            vec!["reactive".to_owned(), receipt.display().to_string()],
        ),
        Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await
    .expect("fixture provider");
    let store = Arc::new(Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("store"),
    ));
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
            resume: Default::default(),
        })
        .await
        .expect("session record");
    let supervisor = Arc::new(
        ExternalProviderSupervisor::new(
            vec![ExternalProviderBinding {
                identity: binding.clone(),
                runtime,
            }],
            Arc::clone(&store),
        )
        .expect("supervisor"),
    );
    let created = supervisor
        .open_provider_fixture(&target.endpoint, root.path().to_owned())
        .await
        .expect("session/new");
    assert_eq!(created, String::from(target.session_id.clone()));
    let first = OperationId::generate();
    let second = OperationId::generate();
    let (first_held, release_first) = supervisor.pause_terminal_write_of(&target, &first);
    let wait = |operation_id: &OperationId| {
        supervisor.wait(ConversationOperationWaitRequest {
            operation_id: operation_id.clone(),
            timeout_seconds: PositiveSeconds::try_from(10).expect("bound"),
        })
    };

    // Act: the first terminal write is accepted and held in the lane.
    supervisor
        .prompt(prompt_request(
            MessageDelivery::Auto,
            &target,
            &first,
            "first",
        ))
        .await
        .expect("first sent");
    first_held
        .await
        .expect("the first reaches its terminal write");
    let dispatch_observed = supervisor.observe_queued_dispatch_fixture(&target, &second);
    supervisor
        .prompt(prompt_request(
            MessageDelivery::Queue,
            &target,
            &second,
            "second",
        ))
        .await
        .expect("second queued");
    let early_prompt = async {
        while recorded_requests(&receipt, "session/prompt") < 2 {
            tokio::task::yield_now().await;
        }
    };
    tokio::select! {
        biased;
        () = early_prompt => {},
        observed = dispatch_observed => observed.expect("the owner offered the mark behind the saved result"),
    }
    let stage_while_held = supervisor
        .show(ConversationOperationShowRequest {
            operation_id: second.clone(),
        })
        .await
        .expect("the store can still be read")
        .stage;
    let prompts_while_held = recorded_requests(&receipt, "session/prompt");
    release_first.send(()).expect("release the first");
    let first_settled = wait(&first).await.expect("first settles");
    let second_settled = wait(&second).await.expect("second settles");
    let prompts = recorded_requests(&receipt, "session/prompt");
    let stopped = supervisor.shutdown().await;

    // Assert
    assert_eq!(stage_while_held, ProviderOperationStage::Admitted);
    assert_eq!(prompts_while_held, 1, "only the first reached the agent");
    for (name, settled) in [("first", &first_settled), ("second", &second_settled)] {
        assert_eq!(
            settled.operation.stage,
            ProviderOperationStage::Terminal,
            "{name}: {settled:?}"
        );
        assert!(
            matches!(
                &settled.output,
                ConversationOperationWaitOutput::Available {
                    settlement: ConversationOperationSettlement::PromptCompleted { .. }
                }
            ),
            "{name} completes, no admission refused: {settled:?}"
        );
    }
    assert_eq!(prompts, 2);
    assert_eq!(stopped, Ok(()));
}
