//! A Router-side close (an agent without session/close) ends the session
//! NotLoaded like any close (the owner's close decision, design note §6):
//! messages waiting in its line end not sent, and an old run's late load or
//! continuation publishes nothing into a later run. These cases drive the
//! crate-private loading function (ensure_provider_session_loaded) and its
//! pauses, so they stay unit tests: they prove a private implementation
//! detail (AGENTS.md: the sibling *_test.rs). The release cases that need
//! only the public runtime are in tests/integration/provider_router_close.rs.

use super::loaded_only_tests::{NoLivePeer, binding, recorded_requests, target};
use super::*;
use crate::{ExternalProviderBinding, ExternalProviderRuntime, ExternalProviderSupervisor};
use collaboration_contracts::ProviderConversationBackend;
use collaboration_protocol::{
    ConversationCloseRequest, ConversationOperationSettlement, ConversationOperationWaitOutput,
    ConversationOperationWaitRequest, MessageText, OperationId, PositiveSeconds,
    ProviderRequestedPolicy, ProviderWorkingDirectory, RouterAccess, SessionId,
};
use collaboration_runtime::{ProviderOperationStore, ProviderSessionRecord};
use std::sync::Arc;

/// Oracle: the owner's close decision (a Router-side close ends NotLoaded
/// like any close, and the session can be loaded again) and the accepted
/// door rule (a MayLoad send loads the session first, as mail does). On an
/// agent that can load but can't close, an API send admitted with delivery
/// queue is held before it enters the line; the Router closes the session
/// and finishes; then the send goes on. It meets a NotLoaded session, loads
/// it again and is sent: the agent sees one session/load and the late
/// prompt after the held one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_send_reaching_the_line_after_a_router_close_loads_the_session_again() {
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
    assert!(sent.is_ok(), "the late send is admitted: {sent:?}");
    assert!(settled.is_ok(), "the late send settles: {settled:?}");
    assert!(
        matches!(
            load,
            crate::provider_acp_session_loading::ProviderSessionLoadOutcome::Ready
        ),
        "the session is loaded again: {load:?}"
    );
    assert_eq!(prompts, 2, "the held prompt, then the late one");
    assert_eq!(loads, 1, "the late send loaded the closed session once");
    stopped.expect("the supervisor drains without a timeout");
}

/// Oracle: D1 spec R14 and the Reviewer's C2 residual: on an agent that can
/// load but can't close, an implicit load of a recorded session reaches the
/// agent and its reply is held; the Router closes the session and finishes;
/// then the agent answers the load. The late restore is dropped: the load
/// answers RouterClosed, the runtime holds no actor for the session, the hub
/// shows no Idle for it, and the agent sees no prompt.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_load_in_flight_when_the_router_closes_the_session_stays_closed() {
    // Arrange: a recorded, unloaded session on a load-capable agent without
    // session/close whose load replies wait for a release, on a real hub.
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("wire.json");
    let mut target = target();
    target.session_id = SessionId::try_from("reactive-session-1".to_owned()).expect("session ID");
    let binding = binding(&target);
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
                "hold-load".to_owned(),
            ],
        ),
        acp_client_runtime::ProviderModelPicker::Standard,
        "fixture",
        "http://127.0.0.1:1/mcp",
        Arc::clone(&hub),
        hub_target.endpoint.clone(),
        Arc::new(acp_provider_adapters::StandardAcpBehavior),
        std::sync::Arc::new(router_clock::SystemRuntimeClock::new()),
    )
    .await
    .expect("fixture provider");
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
    let load = tokio::spawn({
        let supervisor = Arc::clone(&supervisor);
        let store = Arc::clone(&store);
        let target = target.clone();
        async move {
            crate::provider_acp_session_loading::ensure_provider_session_loaded(
                &supervisor,
                &store,
                &NoLivePeer,
                &target,
                collaboration_contracts::LoadPolicy::MayLoad,
            )
            .await
        }
    });
    while recorded_requests(&receipt, "session/load") < 1 {
        tokio::task::yield_now().await;
    }

    // Act: the Router closes the session and finishes; then the agent
    // answers the held load.
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
    node_test_fixture::release_fixture(&receipt, ".release-load").expect("release the load");
    let loaded = load.await.expect("load task");
    let runtime = supervisor.runtime_for(&target.endpoint).expect("runtime");
    let activity = runtime
        .session_activity("reactive-session-1".to_owned())
        .await;
    let history = collaboration_contracts::SessionEventHub::attach(hub.as_ref(), hub_target)
        .await
        .expect("hub attachment")
        .snapshot;
    let prompts = recorded_requests(&receipt, "session/prompt");
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
    assert!(
        matches!(
            loaded,
            crate::provider_acp_session_loading::ProviderSessionLoadOutcome::ClosedWhileLoading { .. }
        ),
        "the load that landed after the close is refused: {loaded:?}"
    );
    assert!(
        matches!(activity, Ok(crate::ProviderSessionActivity::NotLoaded)),
        "the runtime holds no actor for the closed session: {activity:?}"
    );
    assert!(
        !history.iter().any(|event| matches!(
            event.event,
            session_event_model::SessionEvent::StateChanged {
                state: session_event_model::SessionState::Idle,
            }
        )),
        "the hub shows no Idle for the closed session: {history:?}"
    );
    assert_eq!(prompts, 0, "no work reached the agent");
    stopped.expect("the supervisor drains without a timeout");
}

/// Oracle: D1 spec R14 and the Lead's terminal-per-generation rule: on an
/// agent that can load but can't close, an implicit load's client succeeds
/// (the actor is registered) and its continuation is held before it
/// publishes the session active; the Router closes the session and finishes;
/// then the continuation goes on. Nothing for that generation is published
/// after its Closed: the hub stays Closed with no CapabilitiesChanged or
/// Idle, and the load answers RouterClosed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_load_continuation_held_past_a_router_close_publishes_nothing() {
    // Arrange: a recorded, unloaded session on a load-capable agent without
    // session/close, on a real hub; the next load stops before activation.
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("wire.json");
    let mut target = target();
    target.session_id = SessionId::try_from("reactive-session-1".to_owned()).expect("session ID");
    let binding = binding(&target);
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
    let hub = Arc::new(collaboration_runtime::ProviderSessionEventHub::new(
        Arc::clone(&store),
    ));
    let hub_target: message_board::SessionRef =
        serde_json::from_value(serde_json::to_value(&target).expect("target JSON"))
            .expect("hub target");
    let runtime = ExternalProviderRuntime::initialize_with_mcp_http_and_hub(
        crate::external_provider_runtime::acp_scripted_fixture::launch_fixture_agent(
            "cursor-settings-agent.ts",
            vec!["reactive".to_owned(), receipt.display().to_string()],
        ),
        acp_client_runtime::ProviderModelPicker::Standard,
        "fixture",
        "http://127.0.0.1:1/mcp",
        Arc::clone(&hub),
        hub_target.endpoint.clone(),
        Arc::new(acp_provider_adapters::StandardAcpBehavior),
        std::sync::Arc::new(router_clock::SystemRuntimeClock::new()),
    )
    .await
    .expect("fixture provider");
    let (activation_reached, activation_release) = runtime.pause_next_activation();
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
    let load = tokio::spawn({
        let supervisor = Arc::clone(&supervisor);
        let store = Arc::clone(&store);
        let target = target.clone();
        async move {
            crate::provider_acp_session_loading::ensure_provider_session_loaded(
                &supervisor,
                &store,
                &NoLivePeer,
                &target,
                collaboration_contracts::LoadPolicy::MayLoad,
            )
            .await
        }
    });
    activation_reached
        .await
        .expect("the load's client succeeded and its continuation is held");

    // Act: the Router closes the session and finishes, and the hub applies
    // its Closed; the hub is followed from there; then the held continuation
    // goes on.
    let mut before_close =
        collaboration_contracts::SessionEventHub::attach(hub.as_ref(), hub_target.clone())
            .await
            .expect("hub attachment");
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
    loop {
        match before_close.receiver.recv().await {
            Ok(event)
                if matches!(
                    event.event,
                    session_event_model::SessionEvent::StateChanged {
                        state: session_event_model::SessionState::Closed,
                    }
                ) =>
            {
                break;
            }
            Ok(_) => {}
            Err(error) => panic!("the hub stopped before the close: {error:?}"),
        }
    }
    let mut after_close =
        collaboration_contracts::SessionEventHub::attach(hub.as_ref(), hub_target.clone())
            .await
            .expect("hub attachment");
    let state_at_close =
        collaboration_contracts::SessionEventHub::state(hub.as_ref(), hub_target.clone()).await;
    let _go = activation_release.send(());
    let loaded = load.await.expect("load task");
    // The activation, had it gone out, was accepted by the hub before the
    // load returned, so it would be on this follower already.
    let mut published_after_close = after_close.snapshot.clone();
    while let Ok(event) = after_close.receiver.try_recv() {
        published_after_close.push(event);
    }
    let state_after =
        collaboration_contracts::SessionEventHub::state(hub.as_ref(), hub_target).await;
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
    assert!(
        matches!(
            state_at_close,
            Ok(session_event_model::SessionState::Closed)
        ),
        "the hub saw the close: {state_at_close:?}"
    );
    assert!(
        matches!(
            loaded,
            crate::provider_acp_session_loading::ProviderSessionLoadOutcome::ClosedWhileLoading { .. }
        ),
        "the load whose continuation outlived the close answers closed: {loaded:?}"
    );
    assert!(
        !published_after_close.iter().any(|event| matches!(
            event.event,
            session_event_model::SessionEvent::CapabilitiesChanged { .. }
                | session_event_model::SessionEvent::StateChanged { .. }
        )),
        "nothing for the closed generation was published after Closed: {published_after_close:?}"
    );
    assert!(
        matches!(state_after, Ok(session_event_model::SessionState::Closed)),
        "the hub still says Closed: {state_after:?}"
    );
    stopped.expect("the supervisor drains without a timeout");
}
