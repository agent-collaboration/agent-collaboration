//! A queued API prompt the worker has claimed keeps one completion owner even
//! when the worker is cancelled (D1 R14, review F1). The two windows the
//! review names are forced with the supervisor's test pauses: right after the
//! claim, before the dispatch marker; and with the submission spawned, before
//! its dispatch observation. A stop before the submission boundary wins: the
//! operation ends not sent. A stop after it leaves the submission its real
//! outcome. Either way the supervisor's drain must not time out.

use super::loaded_only_tests::{NoLivePeer, binding, recorded_requests, target};
use super::*;
use crate::{ExternalProviderBinding, ExternalProviderRuntime, ExternalProviderSupervisor};
use collaboration_protocol::{
    ConversationOperationSettlement, ConversationOperationWaitOutput,
    ConversationOperationWaitRequest, MessageText, OperationId, PositiveSeconds,
    ProviderRequestedPolicy, ProviderWorkingDirectory, RouterAccess, SessionId,
};
use collaboration_runtime::{ProviderOperationStore, ProviderSessionRecord};
use std::sync::Arc;

/// Where the queued dispatch is paused when the line shuts down.
#[derive(Clone, Copy, Debug)]
enum PausedAt {
    AfterClaim,
    BeforeObservation,
}

struct Shutdown {
    settled: Result<
        collaboration_protocol::ConversationOperationWaitResult,
        collaboration_protocol::ConversationOperationFailure,
    >,
    queued_prompts: usize,
    queue_state: Option<collaboration_protocol::ConversationOperationQueueState>,
    stopped: Result<(), &'static str>,
}

/// Holds a turn, queues one API prompt behind it, pauses its dispatch at
/// `paused_at`, shuts the line down there, then lets the dispatch go on.
async fn shut_down_the_line_at(paused_at: PausedAt) -> Shutdown {
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
    let queued = OperationId::generate();
    supervisor
        .prompt(collaboration_protocol::ConversationPromptRequest {
            delivery: collaboration_protocol::MessageDelivery::Queue,
            operation_id: queued.clone(),
            input_id: None,
            target: target.clone(),
            generation: None,
            requested_by: target.clone().into(),
            approver: target.clone().into(),
            prompt: collaboration_protocol::MessageContent::HumanUser {
                text: MessageText::try_from("queued".to_owned()).expect("prompt"),
            },
        })
        .await
        .expect("queued behind the held turn");
    let (entered, release) = match paused_at {
        PausedAt::AfterClaim => supervisor.pause_next_queued_dispatch_after_claim(),
        PausedAt::BeforeObservation => supervisor.pause_next_dispatch_before_observation(),
    };

    // Act: the turn ends, the worker reaches the paused window, the line
    // shuts down there, and only then does the dispatch go on.
    node_test_fixture::release_fixture(&receipt, ".release").expect("release the turn");
    entered
        .await
        .expect("the queued dispatch reached its paused window");
    queue.shutdown().await;
    let _released = release.send(());
    let settled = supervisor
        .wait(ConversationOperationWaitRequest {
            operation_id: queued.clone(),
            timeout_seconds: PositiveSeconds::try_from(5).expect("bound"),
        })
        .await;
    let queue_state = supervisor
        .queued_operation_registry()
        .snapshot(&queued)
        .expect("queued operation snapshot")
        .and_then(|snapshot| snapshot.queue_state);
    let _held = held.await.expect("held turn task");
    let queued_prompts = recorded_requests(&receipt, "session/prompt").saturating_sub(1);
    let stopped = supervisor.shutdown().await;
    Shutdown {
        settled,
        queued_prompts,
        queue_state,
        stopped,
    }
}

fn completed(settled: &Shutdown) -> bool {
    matches!(
        &settled.settled,
        Ok(collaboration_protocol::ConversationOperationWaitResult {
            output: ConversationOperationWaitOutput::Available {
                settlement: ConversationOperationSettlement::PromptCompleted { .. }
            },
            ..
        })
    )
}

/// Oracle: D1 spec R14 and the review's F1 follow-up, "stop wins before
/// submission → not sent, effect None, zero prompts at the fixture": a
/// dispatch the worker claimed, whose line shut down before its submission
/// left, ends not sent with the line's reason under its one owner, the line
/// shows it not submitted, and the supervisor's drain completes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn claimed_dispatch_stopped_before_submission_ends_not_sent() {
    // Arrange, Act
    let shut_down = shut_down_the_line_at(PausedAt::AfterClaim).await;

    // Assert
    let failure = shut_down.settled.as_ref().err().unwrap_or_else(|| {
        panic!(
            "the stopped dispatch ends not sent: {:?}",
            shut_down.settled
        )
    });
    assert_eq!(
        String::from(failure.message.clone()),
        "not sent: Router shutdown before provider submission"
    );
    assert_eq!(
        failure.effect,
        collaboration_protocol::ProviderOperationEffect::None
    );
    assert_eq!(shut_down.queued_prompts, 0, "nothing reached the agent");
    assert_eq!(
        shut_down.queue_state,
        Some(
            collaboration_protocol::ConversationOperationQueueState::NotSubmitted {
                reason: "Router shutdown before provider submission".into(),
            }
        )
    );
    shut_down
        .stopped
        .expect("the supervisor drains without a timeout");
}

/// Oracle: D1 spec R14 and review F1, "an already-dispatched operation keeps
/// its real outcome": a dispatch whose submission was already spawned when
/// the worker was cancelled settles with that submission's outcome, never
/// marked unsent, and the supervisor's drain completes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dispatch_cancelled_with_its_observation_pending_keeps_its_real_outcome() {
    // Arrange, Act
    let shut_down = shut_down_the_line_at(PausedAt::BeforeObservation).await;

    // Assert
    assert!(completed(&shut_down), "{:?}", shut_down.settled);
    assert_eq!(
        shut_down.queued_prompts, 1,
        "the spawned submission ran once"
    );
    shut_down
        .stopped
        .expect("the supervisor drains without a timeout");
}
