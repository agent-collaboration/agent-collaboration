//! A queued message commits only after the running prompt's settlement is
//! written (design note §13.1), and the line's retries wait on the one
//! injected clock (step 5a). The line itself waits for each of its own
//! messages' settlements, so the race is a direct prompt with a message
//! queued behind it. The scenario holds the prompt at its terminal write, a
//! named point, instead of timing anything.

use super::loaded_only_tests::{NoLivePeer, binding, recorded_requests, target};
use super::*;
use crate::{ExternalProviderBinding, ExternalProviderRuntime, ExternalProviderSupervisor};
use collaboration_contracts::ProviderConversationBackend;
use collaboration_protocol::{
    ConversationOperationSettlement, ConversationOperationWaitOutput,
    ConversationOperationWaitRequest, ConversationPromptRequest, MessageContent, MessageDelivery,
    MessageText, PositiveSeconds, ProviderOperationStage, ProviderRequestedPolicy,
    ProviderWorkingDirectory, RouterAccess, SessionId,
};
use collaboration_runtime::{ProviderOperationStore, ProviderSessionRecord};
use router_clock::{ClockInstant, ManualClock};

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

/// Oracle: §13.1 (a prompt's turn ends only once its settlement is
/// recorded, so the line commits nothing that would see it unresolved) and
/// AGENTS.md's one injected clock. While a direct prompt is held before
/// its terminal write, the message queued behind it is not sent: its worker
/// sleeps on the injected clock. After the first settles, the second still waits until
/// that clock advances; then both complete and no admission is refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_queued_message_commits_only_after_the_previous_settlement_is_written() {
    // Arrange
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("wire.json");
    let mut target = target();
    target.session_id = SessionId::try_from("reactive-session-1".to_owned()).expect("session ID");
    let binding = binding(&target);
    let runtime = ExternalProviderRuntime::initialize(
        crate::external_provider_runtime::acp_scripted_fixture::launch_fixture_agent(
            "cursor-settings-agent.ts",
            vec!["reactive".to_owned(), receipt.display().to_string()],
        ),
        Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await
    .expect("fixture provider");
    runtime
        .create_session(root.path().to_owned())
        .await
        .expect("session/new");
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
        })
        .await
        .expect("session record");
    let supervisor = Arc::new(
        ExternalProviderSupervisor::new(
            vec![ExternalProviderBinding {
                identity: binding,
                runtime,
            }],
            Arc::clone(&store),
        )
        .expect("supervisor"),
    );
    let clock = Arc::new(ManualClock::new(chrono::DateTime::UNIX_EPOCH));
    let queue = Arc::new(ProviderAcpMessageFifo::new(
        Arc::clone(&supervisor),
        Arc::clone(&store),
        Arc::new(NoLivePeer),
        Arc::clone(&clock) as Arc<dyn router_clock::RuntimeClock>,
    ));
    supervisor.attach_message_fifo(&queue);
    let first = OperationId::generate();
    let second = OperationId::generate();
    let (first_held, release_first) = supervisor.pause_terminal_write_of(&first);
    let wait = |operation_id: &OperationId| {
        supervisor.wait(ConversationOperationWaitRequest {
            operation_id: operation_id.clone(),
            timeout_seconds: PositiveSeconds::try_from(10).expect("bound"),
        })
    };

    // Act: the first goes straight to the idle session; the agent answers
    // it and it is held before its write, its turn still open. The second
    // is queued behind that turn.
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
    supervisor
        .prompt(prompt_request(
            MessageDelivery::Queue,
            &target,
            &second,
            "second",
        ))
        .await
        .expect("second queued");
    let retry_deadline = ClockInstant::ZERO.after(MIN_RETRY_DELAY);
    // Either the queued message sleeps on the injected clock, or (a wrong
    // build) its wait ends first: sent and settled, or never on that clock.
    let second_waited = tokio::select! {
        () = clock.not_due_check_now(retry_deadline) => Ok(()),
        settled = wait(&second) => Err(settled),
    };
    let prompts_while_held = recorded_requests(&receipt, "session/prompt");
    release_first.send(()).expect("release the first");
    let first_settled = wait(&first).await.expect("first settles");
    let prompts_after_first = recorded_requests(&receipt, "session/prompt");
    clock
        .advance(MIN_RETRY_DELAY)
        .expect("advance the injected clock");
    let second_settled = wait(&second).await.expect("second settles");
    let prompts = recorded_requests(&receipt, "session/prompt");
    queue.shutdown().await;
    let stopped = supervisor.shutdown().await;

    // Assert
    assert!(
        second_waited.is_ok(),
        "the queued message didn't sleep on the injected clock while the prompt's \
         settlement was held: {second_waited:?}"
    );
    assert_eq!(prompts_while_held, 1, "only the first reached the agent");
    assert_eq!(
        prompts_after_first, 1,
        "the second waits for the injected clock, not wall time"
    );
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
