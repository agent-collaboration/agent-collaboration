//! A session the Router closed itself stays closed (D1 R14, the Reviewer's
//! C2): a send already admitted when the close lands never enters the line or
//! reaches the agent, and nothing reloads the session.

use super::loaded_only_tests::{NoLivePeer, binding, recorded_requests, target};
use super::*;
use crate::{ExternalProviderBinding, ExternalProviderRuntime, ExternalProviderSupervisor};
use collaboration_protocol::{
    ConversationCloseRequest, ConversationOperationSettlement, ConversationOperationWaitOutput,
    ConversationOperationWaitRequest, MessageText, OperationId, PositiveSeconds,
    ProviderRequestedPolicy, ProviderWorkingDirectory, RouterAccess, SessionId,
};
use collaboration_runtime::{ProviderOperationStore, ProviderSessionRecord};
use std::sync::Arc;

/// Oracle: D1 spec R14 and the Reviewer's C2: on an agent that can load but
/// can't close, an API send admitted with delivery queue is held before it
/// enters the line; the Router closes the session and finishes; then the send
/// goes on. It ends not sent, "session closed", with no effect; a load after
/// the close is refused; and the agent sees no prompt and no session/load
/// after the held one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_send_admitted_before_a_router_close_never_reaches_the_agent() {
    // Arrange: a held turn on a load-capable agent without session/close.
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("wire.json");
    let mut target = target();
    target.session_id = SessionId::try_from("reactive-session-1".to_owned()).expect("session ID");
    let binding = binding(&target);
    let runtime = ExternalProviderRuntime::initialize(
        crate::external_provider_runtime::acp_scripted_fixture::launch_fixture_agent(
            "cursor-settings-agent.ts",
            vec![
                "reactive".to_owned(),
                receipt.display().to_string(),
                "hold-first-prompt-open".to_owned(),
            ],
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
                identity: binding.clone(),
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
        binding_generation: binding.generation.clone(),
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
    let (send_admitted, enqueue) = supervisor.pause_next_api_send_before_enqueue();

    // Act: the send is admitted and held; the Router closes the session and
    // finishes; the send goes on; then a load is tried.
    let late = OperationId::generate();
    let send = tokio::spawn({
        let supervisor = Arc::clone(&supervisor);
        let request = collaboration_protocol::ConversationPromptRequest {
            delivery: collaboration_protocol::MessageDelivery::Queue,
            operation_id: late.clone(),
            input_id: None,
            target: target.clone(),
            generation: None,
            requested_by: target.clone().into(),
            approver: target.clone().into(),
            prompt: collaboration_protocol::MessageContent::HumanUser {
                text: MessageText::try_from("late".to_owned()).expect("prompt"),
            },
        };
        async move { supervisor.prompt(request).await }
    });
    send_admitted.await.expect("the send was admitted and held");
    let close = OperationId::generate();
    supervisor
        .close(ConversationCloseRequest {
            operation_id: close.clone(),
            target: target.clone(),
            generation: None,
            requested_by: target.clone().into(),
            approver: target.clone().into(),
        })
        .await
        .expect("close is admitted");
    let closed = supervisor
        .wait(ConversationOperationWaitRequest {
            operation_id: close,
            timeout_seconds: PositiveSeconds::try_from(5).expect("bound"),
        })
        .await;
    let _go = enqueue.send(());
    let sent = send.await.expect("send task");
    let settled = supervisor
        .wait(ConversationOperationWaitRequest {
            operation_id: late,
            timeout_seconds: PositiveSeconds::try_from(5).expect("bound"),
        })
        .await;
    let load = crate::provider_acp_session_loading::ensure_provider_session_loaded(
        &supervisor,
        &store,
        &NoLivePeer,
        &target,
        collaboration_contracts::LoadPolicy::MayLoad,
    )
    .await;
    let _held = held.await.expect("held turn task");
    let prompts = recorded_requests(&receipt, "session/prompt");
    let loads = recorded_requests(&receipt, "session/load");
    queue.shutdown().await;
    let stopped = supervisor.shutdown().await;

    // Assert
    assert!(
        matches!(
            closed,
            Ok(collaboration_protocol::ConversationOperationWaitResult {
                output: ConversationOperationWaitOutput::Available {
                    settlement: ConversationOperationSettlement::Closed {
                        router_closed: true,
                        ..
                    }
                },
                ..
            })
        ),
        "the Router closed the session: {closed:?}"
    );
    assert!(sent.is_err(), "the late send is refused: {sent:?}");
    let failure = settled.expect_err("the late send ends not sent");
    assert_eq!(
        String::from(failure.message.clone()),
        "not sent: session closed"
    );
    assert_eq!(
        failure.effect,
        collaboration_protocol::ProviderOperationEffect::None
    );
    assert!(
        matches!(
            load,
            crate::provider_acp_session_loading::ProviderSessionLoadOutcome::RouterClosed
        ),
        "a load after the close is refused: {load:?}"
    );
    assert_eq!(prompts, 1, "only the held prompt reached the agent");
    assert_eq!(loads, 0, "nothing reloaded the session");
    stopped.expect("the supervisor drains without a timeout");
}
