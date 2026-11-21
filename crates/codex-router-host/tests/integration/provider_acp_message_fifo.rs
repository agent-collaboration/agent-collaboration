//! Queue draining and inspectable provider queue operation behavior.

use agent_automation::{RouteEffectEvidence, SubmissionEffect};
use codex_router_host::{
    ExternalProviderBinding, ExternalProviderRuntime, ExternalProviderSupervisor,
    ProviderAcpDeliveryRoute,
};
use collaboration_contracts::SessionDeliveryRoute;
use collaboration_protocol::{
    DeliveryOutcome, MessageContent, MessageDelivery, MessageText, ProviderOperationKind,
    ProviderRequestedPolicy, ProviderWorkingDirectory, RouterAccess, SessionId,
};
use collaboration_runtime::ProviderOperationStore;
use collaboration_runtime::ProviderSessionRecord;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use crate::provider_acp_message_fifo_support;
use provider_acp_message_fifo_support::{
    NoLivePeer, RecordedEvidence, available_directory, cursor_prompt_fixture,
    first_prompt_refusal_fixture, multiblock_queue_fixture, provider_binding,
    refusing_load_fixture, request, target,
};

#[tokio::test]
async fn typed_queue_keeps_blocks_and_input_identity_through_fifo() {
    let root = tempfile::tempdir().expect("provider root");
    let event_socket = root.path().join("prompt-events.sock");
    let listener = tokio::net::UnixListener::bind(&event_socket).expect("fixture events");
    let target = target();
    let binding = provider_binding(&target);
    let runtime = ExternalProviderRuntime::initialize(
        multiblock_queue_fixture(&event_socket),
        std::sync::Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await
    .expect("fixture provider");
    let store = Arc::new(tokio::sync::Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("store"),
    ));
    store
        .lock()
        .await
        .record_session(&ProviderSessionRecord {
            target: target.clone(),
            working_directory: ProviderWorkingDirectory::try_from("/tmp".to_owned()).expect("cwd"),
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
            created_by: (target.clone()).into(),
            approver: (target.clone()).into(),
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
        .open_provider_fixture(&target.endpoint, PathBuf::from("/tmp"))
        .await
        .expect("session/new");
    assert_eq!(created, String::from(target.session_id.clone()));
    let route = ProviderAcpDeliveryRoute::new(
        target.endpoint.service_id.clone(),
        std::iter::once(target.endpoint.clone()).collect(),
        available_directory(&target, &binding),
        Arc::clone(&supervisor),
        store,
        Arc::new(NoLivePeer),
        std::sync::Arc::new(router_clock::SystemRuntimeClock::new()),
    );
    let first = collaboration_contracts::ProviderConversationBackend::prompt(
        supervisor.as_ref(),
        collaboration_protocol::ConversationPromptRequest {
            delivery: collaboration_protocol::MessageDelivery::Auto,
            input_id: None,
            operation_id: collaboration_protocol::OperationId::generate(),
            target: target.clone(),
            generation: Some(binding.generation),
            requested_by: (target.clone()).into(),
            approver: (target.clone()).into(),
            prompt: MessageContent::Router {
                text: MessageText::try_from("first".to_owned()).expect("prompt"),
            },
        },
    )
    .await
    .expect("first prompt admitted");
    let (mut first_event, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("first prompt deadline")
        .expect("first prompt event");
    let mut first_bytes = [0_u8; 5];
    first_event
        .read_exact(&mut first_bytes)
        .await
        .expect("first event");
    assert_eq!(&first_bytes, b"first");
    let mut other_actor = target.clone();
    other_actor.session_id = SessionId::try_from("other-session".to_owned()).expect("actor ID");

    let unsupported = route
        .queue_contents(
            target.clone(),
            other_actor.clone().into(),
            session_event_model::ProviderSubmission::plain(vec![
                session_event_model::PromptContent::image(
                    "image/png".into(),
                    "aGVsbG8=".into(),
                    None,
                )
                .expect("image"),
            ]),
        )
        .await;
    assert!(matches!(
        unsupported,
        Err(
            codex_router_host::ProviderQueueAdmissionError::UnsupportedContent {
                content_type: "image"
            }
        )
    ));

    let queued = route
        .queue_contents(
            target.clone(),
            other_actor.into(),
            session_event_model::ProviderSubmission::plain(vec![
                session_event_model::PromptContent::resource_link(
                    "https://example.test/context".into(),
                    "context".into(),
                    None,
                )
                .expect("resource link"),
                session_event_model::PromptContent::text("queued text".into()).expect("text"),
            ]),
        )
        .await
        .expect("typed queue admission");
    assert_eq!(queued.preview, "queued text");
    assert_eq!(route.queue_list(&target)[0].input_id, queued.input_id);

    first_event.write_all(b"x").await.expect("release first");
    collaboration_contracts::ProviderConversationBackend::wait(
        supervisor.as_ref(),
        collaboration_protocol::ConversationOperationWaitRequest {
            operation_id: first.operation.operation_id,
            timeout_seconds: collaboration_protocol::PositiveSeconds::try_from(2)
                .expect("wait seconds"),
        },
    )
    .await
    .expect("first prompt settles");
    let (mut blocks_event, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("queued blocks deadline")
        .expect("queued blocks event");
    let mut blocks = [0_u8; 9];
    blocks_event
        .read_exact(&mut blocks)
        .await
        .expect("blocks event");
    assert_eq!(&blocks, b"blocks-ok");
    blocks_event.write_all(b"x").await.expect("release blocks");
    route.shutdown_queue().await;
    supervisor.shutdown().await.expect("supervisor shutdown");
}

#[tokio::test]
async fn cursor_queue_drains_after_control_prompt_settles() {
    let root = tempfile::tempdir().expect("provider root");
    let event_socket = root.path().join("prompt-events.sock");
    let listener = tokio::net::UnixListener::bind(&event_socket).expect("fixture events");
    let target = target();
    let binding = provider_binding(&target);
    let runtime = ExternalProviderRuntime::initialize(
        cursor_prompt_fixture(&event_socket),
        std::sync::Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await
    .expect("fixture provider");
    let store = Arc::new(tokio::sync::Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("store"),
    ));
    store
        .lock()
        .await
        .record_session(&ProviderSessionRecord {
            target: target.clone(),
            working_directory: ProviderWorkingDirectory::try_from("/tmp".to_owned()).expect("cwd"),
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
            created_by: (target.clone()).into(),
            approver: (target.clone()).into(),
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
        .open_provider_fixture(&target.endpoint, PathBuf::from("/tmp"))
        .await
        .expect("session/new");
    assert_eq!(created, String::from(target.session_id.clone()));
    let route = ProviderAcpDeliveryRoute::new(
        target.endpoint.service_id.clone(),
        std::iter::once(target.endpoint.clone()).collect(),
        available_directory(&target, &binding),
        Arc::clone(&supervisor),
        Arc::clone(&store),
        Arc::new(NoLivePeer),
        std::sync::Arc::new(router_clock::SystemRuntimeClock::new()),
    );

    let first_operation_id = collaboration_protocol::OperationId::generate();
    let (terminal_waiting, release_terminal) =
        supervisor.pause_terminal_write_of(&target, &first_operation_id);
    let first = collaboration_contracts::ProviderConversationBackend::prompt(
        supervisor.as_ref(),
        collaboration_protocol::ConversationPromptRequest {
            delivery: collaboration_protocol::MessageDelivery::Auto,
            input_id: None,
            operation_id: first_operation_id,
            target: target.clone(),
            generation: Some(binding.generation.clone()),
            requested_by: (target.clone()).into(),
            approver: (target.clone()).into(),
            prompt: MessageContent::Router {
                text: MessageText::try_from("first".to_owned()).expect("prompt"),
            },
        },
    )
    .await
    .expect("Control conversation/prompt admission");
    assert_eq!(
        first.operation.operation,
        ProviderOperationKind::ConversationPrompt
    );

    let (mut first_event, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("first event deadline")
        .expect("first event");
    let mut first_bytes = [0_u8; 5];
    first_event
        .read_exact(&mut first_bytes)
        .await
        .expect("first bytes");
    let evidence = RecordedEvidence(tokio::sync::Mutex::new(Vec::new()));
    let queued_request = request(target.clone(), "second");
    let expected_push_preview = queued_request
        .payload
        .line
        .as_str()
        .chars()
        .take(80)
        .collect::<String>();
    let queued_operation_id =
        collaboration_protocol::OperationId::try_from(queued_request.attempt.as_str().to_owned())
            .expect("queue operation ID");
    let queued = route
        .deliver(queued_request, &evidence)
        .await
        .expect("queued delivery");
    assert!(matches!(
        queued.receipt().outcome.clone(),
        DeliveryOutcome::Queued
    ));
    let queued_snapshot = collaboration_contracts::ProviderConversationBackend::show(
        supervisor.as_ref(),
        collaboration_protocol::ConversationOperationShowRequest {
            operation_id: queued_operation_id.clone(),
        },
    )
    .await
    .expect("queued operation is inspectable");
    assert_eq!(
        queued_snapshot.queue_state,
        Some(collaboration_protocol::ConversationOperationQueueState::RouterQueued)
    );
    let queued_input_id = queued_snapshot.input_id.clone().expect("queued InputId");
    let listed = route.queue_list(&target);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].input_id, queued_input_id);
    assert_eq!(listed[0].preview, expected_push_preview);
    let dispatch_observed =
        supervisor.observe_queued_dispatch_fixture(&target, &queued_operation_id);

    first_event
        .write_all(b"x")
        .await
        .expect("release first prompt");
    terminal_waiting
        .await
        .expect("Control prompt reached its terminal write");
    tokio::select! {
        biased;
        early = listener.accept() => panic!("queued delivery reached the wire before the Control settlement: {early:?}"),
        observed = dispatch_observed => observed.expect("queued delivery offered its mark behind the held terminal write"),
    }
    assert!(
        store
            .lock()
            .await
            .inspect(&queued_operation_id)
            .await
            .expect("inspect queued operation during the hold")
            .is_none(),
        "the queued delivery's first write must wait behind the Control settlement"
    );
    release_terminal
        .send(())
        .expect("release the Control settlement");
    let first_settlement = collaboration_contracts::ProviderConversationBackend::wait(
        supervisor.as_ref(),
        collaboration_protocol::ConversationOperationWaitRequest {
            operation_id: first.operation.operation_id,
            timeout_seconds: collaboration_protocol::PositiveSeconds::try_from(2)
                .expect("wait seconds"),
        },
    )
    .await
    .expect("first prompt settles");
    assert!(matches!(
        first_settlement.output,
        collaboration_protocol::ConversationOperationWaitOutput::Available { .. }
    ));
    let (mut second_event, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("queued prompt must drain after Control prompt settles")
        .expect("second event");
    let mut second_bytes = [0_u8; 6];
    second_event
        .read_exact(&mut second_bytes)
        .await
        .expect("second bytes");
    assert_eq!(first_bytes, *b"first");
    assert_eq!(second_bytes, *b"second");
    let submitted_snapshot = collaboration_contracts::ProviderConversationBackend::show(
        supervisor.as_ref(),
        collaboration_protocol::ConversationOperationShowRequest {
            operation_id: queued_operation_id.clone(),
        },
    )
    .await
    .expect("submitted operation uses provider operation record");
    assert_eq!(submitted_snapshot.queue_state, None);
    assert_eq!(
        route.queue_cancel(&target, &queued_input_id).await,
        Err(codex_router_host::ProviderQueueCancellationError::NotQueued)
    );
    assert_eq!(
        submitted_snapshot.stage,
        collaboration_protocol::ProviderOperationStage::MayHaveDispatched
    );
    second_event
        .write_all(b"x")
        .await
        .expect("release second prompt");
    let completed = collaboration_contracts::ProviderConversationBackend::wait(
        supervisor.as_ref(),
        collaboration_protocol::ConversationOperationWaitRequest {
            operation_id: queued_operation_id,
            timeout_seconds: collaboration_protocol::PositiveSeconds::try_from(2)
                .expect("wait seconds"),
        },
    )
    .await
    .expect("drained prompt settles");
    assert!(matches!(
        completed.output,
        collaboration_protocol::ConversationOperationWaitOutput::Available { .. }
    ));
    assert_eq!(completed.operation.queue_state, None);
    assert_eq!(
        completed.operation.stage,
        collaboration_protocol::ProviderOperationStage::Terminal
    );
    assert_eq!(
        completed.operation.effect,
        collaboration_protocol::ProviderOperationEffect::Applied
    );
    assert_eq!(
        completed.operation.reconciliation,
        collaboration_protocol::ProviderReconciliationState::Confirmed
    );

    route.shutdown_queue().await;
    supervisor.shutdown().await.expect("supervisor shutdown");
}

#[tokio::test]
async fn permanent_queued_load_failure_is_inspectable_and_does_not_stop_fifo() {
    let root = tempfile::tempdir().expect("provider root");
    let load_marker = root.path().join("load-marker.txt");
    let target = target();
    let binding = provider_binding(&target);
    let clock = Arc::new(router_clock::ManualClock::new(chrono::DateTime::UNIX_EPOCH));
    let store = Arc::new(tokio::sync::Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("store"),
    ));
    let endpoint =
        serde_json::from_value(serde_json::to_value(&target.endpoint).expect("endpoint JSON"))
            .expect("board endpoint");
    let mut launch = refusing_load_fixture(&load_marker);
    launch.arguments.push("hold-first-refusal".to_owned());
    let runtime = ExternalProviderRuntime::initialize_with_fixture_clock(
        launch,
        Arc::new(acp_provider_adapters::StandardAcpBehavior),
        Arc::new(collaboration_runtime::ProviderSessionEventHub::new(
            store.clone(),
        )),
        endpoint,
        clock.clone(),
    )
    .await
    .expect("fixture provider");
    store
        .lock()
        .await
        .record_session(&ProviderSessionRecord {
            target: target.clone(),
            working_directory: ProviderWorkingDirectory::try_from("/tmp".to_owned()).expect("cwd"),
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
    let route = ProviderAcpDeliveryRoute::new(
        target.endpoint.service_id.clone(),
        std::iter::once(target.endpoint.clone()).collect(),
        available_directory(&target, &binding),
        Arc::clone(&supervisor),
        store,
        Arc::new(NoLivePeer),
        clock,
    );
    let evidence = RecordedEvidence(tokio::sync::Mutex::new(Vec::new()));
    let mut first_request = request(target.clone(), "first load failure");
    first_request.mode = MessageDelivery::Queue;
    let first_id =
        collaboration_protocol::OperationId::try_from(first_request.attempt.as_str().to_owned())
            .expect("first queue operation ID");
    let mut second_request = request(target, "second load failure");
    second_request.mode = MessageDelivery::Queue;
    let second_id =
        collaboration_protocol::OperationId::try_from(second_request.attempt.as_str().to_owned())
            .expect("second queue operation ID");
    let first = route
        .deliver(first_request, &evidence)
        .await
        .expect("first queue");
    assert_eq!(first.receipt().outcome, DeliveryOutcome::Queued);
    while !load_marker.exists() {
        tokio::task::yield_now().await;
    }

    // The second request samples A while its refusal is held. Let A settle
    // before the prepared second request is admitted to an owner.
    let (sampled, submit_second) = route.pause_next_queue_plan_for_fixture();
    let second = route.deliver(second_request, &evidence);
    tokio::pin!(second);
    let planned = tokio::select! {
        sampled = sampled => sampled.expect("the second request sampled A"),
        result = &mut second => panic!("the second request completed before the plan gate: {result:?}"),
    };
    assert_eq!(planned.generation.number(), 1);
    assert_eq!(
        evidence.0.lock().await.len(),
        1,
        "the proposed second plan recorded no intake"
    );
    node_test_fixture::release_fixture(&load_marker, ".release-first")
        .expect("release A's refusal");
    wait_for_permanent_queue_failure(&supervisor, first_id).await;
    submit_second
        .send(())
        .expect("offer the stale plan to the owner");

    let second = second.await.expect("second queue replans");
    assert_eq!(
        second.receipt().outcome,
        DeliveryOutcome::Queued,
        "the stale plan gets one pre-admission re-plan"
    );
    wait_for_permanent_queue_failure(&supervisor, second_id).await;
    assert!(matches!(evidence.0.lock().await.as_slice(),
        [RouteEffectEvidence::ProviderAcp(first), RouteEffectEvidence::ProviderAcp(second)]
            if first.submission == SubmissionEffect::RouterQueued && second.submission == SubmissionEffect::RouterQueued));
    let loads = std::fs::read_to_string(load_marker).expect("load marker");
    assert_eq!(
        loads.lines().collect::<Vec<_>>(),
        ["session/load", "session/load"]
    );

    route.shutdown_queue().await;
    supervisor.shutdown().await.expect("supervisor shutdown");
}

#[expect(
    clippy::expect_used,
    reason = "An accepted queue item must remain inspectable in the fixture."
)]
async fn wait_for_permanent_queue_failure(
    supervisor: &ExternalProviderSupervisor,
    operation_id: collaboration_protocol::OperationId,
) {
    loop {
        let snapshot = collaboration_contracts::ProviderConversationBackend::show(
            supervisor,
            collaboration_protocol::ConversationOperationShowRequest {
                operation_id: operation_id.clone(),
            },
        )
        .await
        .expect("queued operation remains inspectable");
        if matches!(
            snapshot.queue_state,
            Some(collaboration_protocol::ConversationOperationQueueState::NotSubmitted { .. })
        ) {
            return;
        }
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn failed_queued_prompt_advances_to_the_next_accepted_item() {
    let root = tempfile::tempdir().expect("provider root");
    let event_socket = root.path().join("prompt-events.sock");
    let listener = tokio::net::UnixListener::bind(&event_socket).expect("fixture events");
    let target = target();
    let binding = provider_binding(&target);
    let runtime = ExternalProviderRuntime::initialize(
        first_prompt_refusal_fixture(&event_socket),
        std::sync::Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await
    .expect("fixture provider");
    let store = Arc::new(tokio::sync::Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("store"),
    ));
    store
        .lock()
        .await
        .record_session(&ProviderSessionRecord {
            target: target.clone(),
            working_directory: ProviderWorkingDirectory::try_from("/tmp".to_owned()).expect("cwd"),
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
            created_by: (target.clone()).into(),
            approver: (target.clone()).into(),
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
        .open_provider_fixture(&target.endpoint, PathBuf::from("/tmp"))
        .await
        .expect("session/new");
    assert_eq!(created, String::from(target.session_id.clone()));
    let active_prompt = collaboration_contracts::ProviderConversationBackend::prompt(
        supervisor.as_ref(),
        collaboration_protocol::ConversationPromptRequest {
            delivery: collaboration_protocol::MessageDelivery::Auto,
            input_id: None,
            operation_id: collaboration_protocol::OperationId::generate(),
            target: target.clone(),
            generation: Some(binding.generation.clone()),
            requested_by: (target.clone()).into(),
            approver: (target.clone()).into(),
            prompt: MessageContent::Router {
                text: MessageText::try_from("active prompt".to_owned()).expect("prompt"),
            },
        },
    )
    .await
    .expect("active prompt admission");
    let (mut active_event, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("active prompt event deadline")
        .expect("active prompt event");
    let mut active_bytes = [0_u8; 6];
    active_event
        .read_exact(&mut active_bytes)
        .await
        .expect("active event bytes");
    assert_eq!(&active_bytes, b"active");
    let route = ProviderAcpDeliveryRoute::new(
        target.endpoint.service_id.clone(),
        std::iter::once(target.endpoint.clone()).collect(),
        available_directory(&target, &binding),
        Arc::clone(&supervisor),
        Arc::clone(&store),
        Arc::new(NoLivePeer),
        std::sync::Arc::new(router_clock::SystemRuntimeClock::new()),
    );
    let evidence = RecordedEvidence(tokio::sync::Mutex::new(Vec::new()));
    let mut first_request = request(target.clone(), "first queued prompt");
    first_request.mode = MessageDelivery::Queue;
    let first_id =
        collaboration_protocol::OperationId::try_from(first_request.attempt.as_str().to_owned())
            .expect("first queue operation ID");
    let mut second_request = request(target.clone(), "second queued prompt");
    second_request.mode = MessageDelivery::Queue;
    let second_id =
        collaboration_protocol::OperationId::try_from(second_request.attempt.as_str().to_owned())
            .expect("second queue operation ID");
    for queued_request in [first_request, second_request] {
        assert!(matches!(
            route
                .deliver(queued_request, &evidence)
                .await
                .expect("queued delivery")
                .receipt()
                .outcome
                .clone(),
            DeliveryOutcome::Queued
        ));
    }
    active_event
        .write_all(b"x")
        .await
        .expect("release active prompt");
    let event_result = tokio::time::timeout(Duration::from_secs(3), listener.accept()).await;
    if event_result.is_err() {
        let first_state = collaboration_contracts::ProviderConversationBackend::show(
            supervisor.as_ref(),
            collaboration_protocol::ConversationOperationShowRequest {
                operation_id: first_id.clone(),
            },
        )
        .await
        .expect("first operation diagnostic");
        let second_state = collaboration_contracts::ProviderConversationBackend::show(
            supervisor.as_ref(),
            collaboration_protocol::ConversationOperationShowRequest {
                operation_id: second_id.clone(),
            },
        )
        .await
        .expect("second operation diagnostic");
        panic!(
            "second queued prompt event timed out; first={first_state:?}; second={second_state:?}"
        );
    }
    let (mut event, _) = event_result
        .expect("checked event deadline")
        .expect("second queued prompt event");
    let mut received = [0_u8; 6];
    event.read_exact(&mut received).await.expect("event bytes");
    assert_eq!(&received, b"second");
    let second_settlement = collaboration_contracts::ProviderConversationBackend::wait(
        supervisor.as_ref(),
        collaboration_protocol::ConversationOperationWaitRequest {
            operation_id: second_id.clone(),
            timeout_seconds: collaboration_protocol::PositiveSeconds::try_from(2)
                .expect("wait seconds"),
        },
    )
    .await
    .expect("second prompt settlement");
    assert!(matches!(
        second_settlement.output,
        collaboration_protocol::ConversationOperationWaitOutput::Available { .. }
    ));
    let first = collaboration_contracts::ProviderConversationBackend::show(
        supervisor.as_ref(),
        collaboration_protocol::ConversationOperationShowRequest {
            operation_id: first_id,
        },
    )
    .await
    .expect("first operation remains inspectable");
    assert_eq!(
        first.stage,
        collaboration_protocol::ProviderOperationStage::Terminal
    );
    assert_eq!(
        first.effect,
        collaboration_protocol::ProviderOperationEffect::None
    );
    let second = collaboration_contracts::ProviderConversationBackend::show(
        supervisor.as_ref(),
        collaboration_protocol::ConversationOperationShowRequest {
            operation_id: second_id,
        },
    )
    .await
    .expect("second operation remains inspectable");
    assert_eq!(
        second.stage,
        collaboration_protocol::ProviderOperationStage::Terminal
    );
    assert_eq!(
        second.effect,
        collaboration_protocol::ProviderOperationEffect::Applied
    );
    let active_settlement = collaboration_contracts::ProviderConversationBackend::wait(
        supervisor.as_ref(),
        collaboration_protocol::ConversationOperationWaitRequest {
            operation_id: active_prompt.operation.operation_id,
            timeout_seconds: collaboration_protocol::PositiveSeconds::try_from(2)
                .expect("wait seconds"),
        },
    )
    .await
    .expect("active prompt settlement");
    assert!(matches!(
        active_settlement.output,
        collaboration_protocol::ConversationOperationWaitOutput::Available { .. }
    ));

    route.shutdown_queue().await;
    supervisor.shutdown().await.expect("supervisor shutdown");
}

#[tokio::test]
async fn provider_retirement_marks_queued_items_not_submitted() {
    let root = tempfile::tempdir().expect("provider root");
    let event_socket = root.path().join("prompt-events.sock");
    let listener = tokio::net::UnixListener::bind(&event_socket).expect("fixture events");
    let target = target();
    let binding = provider_binding(&target);
    let runtime = ExternalProviderRuntime::initialize(
        cursor_prompt_fixture(&event_socket),
        std::sync::Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await
    .expect("fixture provider");
    let store = Arc::new(tokio::sync::Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("store"),
    ));
    store
        .lock()
        .await
        .record_session(&ProviderSessionRecord {
            target: target.clone(),
            working_directory: ProviderWorkingDirectory::try_from("/tmp".to_owned()).expect("cwd"),
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
            created_by: (target.clone()).into(),
            approver: (target.clone()).into(),
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
        .open_provider_fixture(&target.endpoint, PathBuf::from("/tmp"))
        .await
        .expect("session/new");
    assert_eq!(created, String::from(target.session_id.clone()));
    let route = ProviderAcpDeliveryRoute::new(
        target.endpoint.service_id.clone(),
        std::iter::once(target.endpoint.clone()).collect(),
        available_directory(&target, &binding),
        Arc::clone(&supervisor),
        store,
        Arc::new(NoLivePeer),
        std::sync::Arc::new(router_clock::SystemRuntimeClock::new()),
    );
    let first = collaboration_contracts::ProviderConversationBackend::prompt(
        supervisor.as_ref(),
        collaboration_protocol::ConversationPromptRequest {
            delivery: collaboration_protocol::MessageDelivery::Auto,
            input_id: None,
            operation_id: collaboration_protocol::OperationId::generate(),
            target: target.clone(),
            generation: Some(binding.generation.clone()),
            requested_by: (target.clone()).into(),
            approver: (target.clone()).into(),
            prompt: MessageContent::Router {
                text: MessageText::try_from("active".to_owned()).expect("prompt"),
            },
        },
    )
    .await
    .expect("Control conversation/prompt admission");
    let (_first_event, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("first event deadline")
        .expect("first event");
    let evidence = RecordedEvidence(tokio::sync::Mutex::new(Vec::new()));
    let queued_request = request(target.clone(), "drop on retirement");
    let queued_id =
        collaboration_protocol::OperationId::try_from(queued_request.attempt.as_str().to_owned())
            .expect("queued operation ID");
    assert!(matches!(
        route
            .deliver(queued_request, &evidence)
            .await
            .expect("queued delivery")
            .receipt()
            .outcome
            .clone(),
        DeliveryOutcome::Queued
    ));
    let second_queued_request = request(target, "also drop on retirement");
    let second_queued_id = collaboration_protocol::OperationId::try_from(
        second_queued_request.attempt.as_str().to_owned(),
    )
    .expect("second queued operation ID");
    assert!(matches!(
        route
            .deliver(second_queued_request, &evidence)
            .await
            .expect("second queued delivery")
            .receipt()
            .outcome
            .clone(),
        DeliveryOutcome::Queued
    ));

    supervisor.shutdown().await.expect("provider retires");
    for operation_id in [queued_id, second_queued_id] {
        let snapshot = collaboration_contracts::ProviderConversationBackend::show(
            supervisor.as_ref(),
            collaboration_protocol::ConversationOperationShowRequest { operation_id },
        )
        .await
        .expect("retired queue item remains inspectable");
        assert!(
            matches!(
                &snapshot.queue_state,
                Some(collaboration_protocol::ConversationOperationQueueState::NotSubmitted { reason })
                    if reason == "provider connection closed"
            ),
            "{:?}",
            snapshot.queue_state
        );
    }
    let _ = first;
    route.shutdown_queue().await;
}
