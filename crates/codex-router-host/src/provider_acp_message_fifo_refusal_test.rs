//! A queued entry the agent refuses at dispatch ends not sent with the reason
//! (D1 R14, Lead 2026-11-16): the line records it, so a caller waiting on the
//! input's outcome learns it at once, and the record is kept for show.

use super::loaded_only_tests::{NoLivePeer, binding, recorded_requests, target};
use super::*;
use crate::{ExternalProviderBinding, ExternalProviderRuntime, ExternalProviderSupervisor};
use collaboration_protocol::{
    ConversationOperationQueueState, OperationId, ProviderRequestedPolicy,
    ProviderWorkingDirectory, RouterAccess, SessionId,
};
use collaboration_runtime::{ProviderOperationStore, ProviderSessionRecord};
use std::sync::Arc;

/// Oracle: D1 spec R14, "not sent with the reason", on every exit from the
/// line: an entry whose dispatch the runtime answers without submitting (here
/// an image to an agent that takes none, enqueued past admission's own check)
/// is recorded not sent with a reason, never cleared as if it ran, and nothing
/// reaches the agent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn entry_refused_at_dispatch_is_recorded_not_sent_with_the_reason() {
    // Arrange: an idle session on an agent whose prompts accept no image.
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
    let queue = Arc::new(ProviderAcpMessageFifo::new(
        Arc::clone(&supervisor),
        Arc::clone(&store),
        Arc::new(NoLivePeer),
        std::sync::Arc::new(router_clock::SystemRuntimeClock::new()),
    ));
    supervisor.attach_message_fifo(&queue);
    let image = session_event_model::PromptContent::image(
        "image/png".to_owned(),
        "aGVsbG8=".to_owned(),
        None,
    )
    .expect("image");
    let submission = session_event_model::ProviderSubmission::plain(vec![image]);

    // Act: enqueue it as the route's queue_contents does, past its content check.
    let operation_id = OperationId::generate();
    let input_id = session_event_model::InputId::generate();
    let permit = queue
        .reserve(&target, &operation_id, LoadPolicy::MayLoad)
        .await
        .expect("room in the line");
    supervisor
        .queued_operation_registry()
        .record_queued_contents(
            operation_id.clone(),
            target.clone(),
            binding,
            input_id.clone(),
            &submission.content,
        );
    permit.send(ProviderQueuedPrompt::Contents {
        request: crate::ProviderPromptContentsRequest {
            operation_id: operation_id.clone(),
            input_id,
            target: target.clone(),
            requested_by: target.clone().into(),
            approver: target.clone().into(),
            input: acp_client_runtime::ProviderTurnInput::Prompt(submission),
            busy: crate::external_provider_supervisor::PromptBusyPolicy::RefuseBusy,
        },
        admitted: None,
    });
    // The line either records why it never ran, or drops the entry as if it had.
    let outcome = loop {
        match supervisor
            .queued_operation_registry()
            .snapshot(&operation_id)
            .expect("registry snapshot")
        {
            Some(snapshot) => {
                if let Some(state @ ConversationOperationQueueState::NotSubmitted { .. }) =
                    snapshot.queue_state
                {
                    break Some(state);
                }
            }
            None => break None,
        }
        tokio::task::yield_now().await;
    };
    queue.shutdown().await;
    let prompts = recorded_requests(&receipt, "session/prompt");
    let stopped = supervisor.shutdown().await;

    // Assert
    match outcome {
        Some(ConversationOperationQueueState::NotSubmitted { reason }) => {
            assert!(!reason.is_empty(), "the reason is named");
        }
        other => panic!("the refused entry is recorded not sent, not cleared: {other:?}"),
    }
    assert_eq!(prompts, 0, "nothing reached the agent");
    stopped.expect("supervisor shutdown");
}
