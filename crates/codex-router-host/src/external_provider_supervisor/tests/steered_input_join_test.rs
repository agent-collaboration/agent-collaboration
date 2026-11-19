//! A steered API prompt joins the running turn correctly (D1 R12). V22 (Lead
//! 2026-11-16): it is taken under the input ID its request carried. The
//! request comes in through the supervisor's API backend
//! (`ProviderConversationBackend::prompt`, what the API service calls with the
//! caller's request) with a chosen input ID; the session's hub must record
//! InputAccepted with exactly that ID. Review F3 follow-up: it names the joined
//! turn's approver even when that turn ended before the steer read it.

use super::*;
use crate::provider_acp_message_fifo::ProviderAcpMessageFifo;
use collaboration_contracts::SessionEventHub as _;
use collaboration_runtime::ProviderSessionRecord;
use session_event_model::SessionEvent;

/// A steering agent that holds its first turn open, on a supervisor whose
/// session is recorded on a real hub.
struct SteeringSession {
    _root: tempfile::TempDir,
    receipt: std::path::PathBuf,
    target: SessionRef,
    hub: Arc<collaboration_runtime::ProviderSessionEventHub>,
    hub_target: message_board::SessionRef,
    supervisor: Arc<ExternalProviderSupervisor>,
    queue: Arc<ProviderAcpMessageFifo>,
}

async fn steering_session() -> SteeringSession {
    steering_session_on(
        "cursor-settings-agent.ts",
        "reactive-session-1",
        |receipt| {
            vec![
                "reactive".to_owned(),
                receipt.display().to_string(),
                "hold-first-prompt-open".to_owned(),
                "advertise-steering".to_owned(),
            ]
        },
    )
    .await
}

/// A steering session on `program`, whose first session is `session_id`,
/// launched with the arguments `arguments` gives for its receipt.
async fn steering_session_on(
    program: &str,
    session_id: &str,
    arguments: impl FnOnce(&std::path::Path) -> Vec<String>,
) -> SteeringSession {
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("wire.json");
    let target = SessionRef {
        endpoint: endpoint(),
        session_id: SessionId::try_from(session_id.to_owned()).expect("session"),
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
            program,
            arguments(&receipt),
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
            resume: Default::default(),
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
        std::sync::Arc::new(router_clock::SystemRuntimeClock::new()),
    ));
    supervisor.attach_message_fifo(&queue);
    SteeringSession {
        _root: root,
        receipt,
        target,
        hub,
        hub_target,
        supervisor,
        queue,
    }
}

/// Oracle: D1 spec R12 and V22, "the operation's input ID equals the input ID
/// of the session event that took the message (InputAccepted or
/// TurnStarted)": a prompt sent while a turn runs, carrying input ID X, is
/// steered, and the hub's InputAccepted for it carries X, never a fresh ID.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn steered_prompt_is_accepted_under_its_requests_input_id() {
    // Arrange: a running turn on a steering agent, recorded on a real hub.
    let SteeringSession {
        _root,
        receipt,
        target,
        hub,
        hub_target,
        supervisor,
        queue,
    } = steering_session().await;
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

    // Act: the caller's request names its own input ID. The hub is followed
    // from before the send: it publishes the acceptance on its own schedule.
    let mut followed = hub
        .attach(hub_target.clone())
        .await
        .expect("hub attachment");
    let chosen = session_event_model::InputId::generate();
    let steered = supervisor
        .prompt(ConversationPromptRequest {
            delivery: collaboration_protocol::MessageDelivery::Auto,
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
    let accepted = loop {
        match followed.receiver.recv().await {
            Ok(event) => {
                if let SessionEvent::InputAccepted { input_id, .. } = event.event {
                    break vec![input_id];
                }
            }
            Err(error) => panic!("the hub stopped before an acceptance: {error:?}"),
        }
    };
    node_test_fixture::release_fixture(&receipt, ".release").expect("release the turn");
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

/// Oracle: D1 spec R12 and program design C1, "the result names the running
/// turn's approver when it differs from the requested one", with the review's
/// F3 follow-ups: a mail-started turn under approver A takes an API steer
/// naming B. The injection reply, the turn's end (its settlement), and more
/// settlements than the supervisor retains (evicting the turn's record) all
/// land before the steer reads the approver. The steer still names A.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn steer_whose_turn_ended_and_was_evicted_before_it_read_the_approver_still_names_it() {
    // Arrange: mail starts the held turn under A.
    let session = steering_session().await;
    let approver_a = human("approver-a");
    let approver_b = human("approver-b");
    let mailed_operation = OperationId::generate();
    let (published, release_publication) = session
        .supervisor
        .pause_after_result_publication(mailed_operation.clone());
    let mailed = session
        .supervisor
        .submit_delivery_prompt_contents(ProviderPromptContentsRequest {
            operation_id: mailed_operation.clone(),
            input_id: session_event_model::InputId::generate(),
            target: session.target.clone(),
            requested_by: approver_a.clone(),
            approver: approver_a.clone(),
            input: acp_client_runtime::ProviderTurnInput::Prompt(
                session_event_model::ProviderSubmission::plain(vec![
                    session_event_model::PromptContent::text("first".to_owned())
                        .expect("mail text"),
                ]),
            ),
            busy: PromptBusyPolicy::RefuseBusy,
        })
        .await;
    while recorded_requests(&session.receipt, "session/prompt") < 1 {
        tokio::task::yield_now().await;
    }
    let (steer_replied, continue_steer) = session.supervisor.pause_next_steer_after_reply();

    // Act: B's prompt is injected; the turn ends and settles; only then does
    // the steer go on to read the approver.
    let steered_operation = OperationId::generate();
    let steer = tokio::spawn({
        let supervisor = Arc::clone(&session.supervisor);
        let request = ConversationPromptRequest {
            delivery: collaboration_protocol::MessageDelivery::Auto,
            operation_id: steered_operation.clone(),
            input_id: None,
            target: session.target.clone(),
            generation: None,
            requested_by: approver_b.clone(),
            approver: approver_b.clone(),
            prompt: collaboration_protocol::MessageContent::HumanUser {
                text: MessageText::try_from("second".to_owned()).expect("prompt"),
            },
        };
        async move { supervisor.prompt(request).await }
    });
    steer_replied
        .await
        .expect("the steer's injection reply arrived");
    node_test_fixture::release_fixture(&session.receipt, ".release").expect("release the turn");
    let turn_settled = session
        .supervisor
        .wait(ConversationOperationWaitRequest {
            operation_id: mailed_operation.clone(),
            timeout_seconds: PositiveSeconds::try_from(5).expect("bound"),
        })
        .await;
    published.await.expect("the turn's result is visible");
    for _ in 0..=MAX_RETAINED_SETTLEMENTS {
        let other = OperationId::generate();
        session.supervisor.hold_live_operation(other.clone());
        assert!(session.supervisor.publish_held_result(&other).await);
    }
    let evicted = session
        .supervisor
        .held_result_for_test(&mailed_operation)
        .is_none();
    let _released = release_publication.send(());
    let _continued = continue_steer.send(());
    let submitted = steer.await.expect("steer task");
    let steered = session
        .supervisor
        .wait(ConversationOperationWaitRequest {
            operation_id: steered_operation,
            timeout_seconds: PositiveSeconds::try_from(5).expect("bound"),
        })
        .await;
    session.queue.shutdown().await;
    let stopped = session.supervisor.shutdown().await;

    // Assert: the turn had settled before the read, and the steer names A.
    assert!(
        matches!(mailed, Ok(ProviderPromptDispatch::Submitted)),
        "{mailed:?}"
    );
    assert!(
        matches!(
            &turn_settled,
            Ok(collaboration_protocol::ConversationOperationWaitResult {
                output: collaboration_protocol::ConversationOperationWaitOutput::Available {
                    settlement: ConversationOperationSettlement::PromptCompleted { .. }
                },
                ..
            })
        ),
        "the mail turn settled before the steer read its approver: {turn_settled:?}"
    );
    assert!(evicted, "the turn's settlement and record were evicted");
    assert!(submitted.is_ok(), "the steer was admitted: {submitted:?}");
    match steered {
        Ok(collaboration_protocol::ConversationOperationWaitResult {
            output:
                collaboration_protocol::ConversationOperationWaitOutput::Available {
                    settlement: ConversationOperationSettlement::Steered { turn_approver, .. },
                },
            ..
        }) => assert_eq!(
            turn_approver,
            Some(approver_a),
            "the joined turn's approver"
        ),
        other => panic!("expected a steered settlement, got {other:?}"),
    }
    stopped.expect("supervisor shutdown");
}

/// Oracle: D1 spec R12 and program design C1, with the Reviewer's F3 at
/// f11cb0c38: the steer began while turn A ran, but before its injection A
/// ended and turn C (approver C) started, so the injection joined C. C then
/// ends and more settlements than are retained evict its record before the
/// steer reads it. The steer still names C, the turn it actually joined.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn steer_into_a_turn_that_replaced_the_running_one_names_that_turns_approver() {
    // Arrange: every prompt's turn is held until its own release file.
    let session = steering_session_on("provider-steering-agent.ts", "fixture-session", |receipt| {
        vec![
            "send-race".to_owned(),
            receipt.display().to_string(),
            "hold-each-prompt".to_owned(),
        ]
    })
    .await;
    let approver_a = human("approver-a");
    let approver_b = human("approver-b");
    let approver_c = human("approver-c");
    let prompt = |operation_id: &OperationId,
                  approver: &collaboration_protocol::ProviderIdentity,
                  text: &str| {
        ConversationPromptRequest {
            delivery: collaboration_protocol::MessageDelivery::Auto,
            operation_id: operation_id.clone(),
            input_id: None,
            target: session.target.clone(),
            generation: None,
            requested_by: approver.clone(),
            approver: approver.clone(),
            prompt: collaboration_protocol::MessageContent::HumanUser {
                text: MessageText::try_from(text.to_owned()).expect("prompt"),
            },
        }
    };
    let turn_a = OperationId::generate();
    session
        .supervisor
        .prompt(prompt(&turn_a, &approver_a, "first"))
        .await
        .expect("turn A starts");
    while recorded_requests(&session.receipt, "session/prompt") < 1 {
        tokio::task::yield_now().await;
    }
    let (steer_pinned, inject) = session.supervisor.pause_next_steer_before_injection();
    let (steer_replied, read_approver) = session.supervisor.pause_next_steer_after_reply();

    // Act: B's steer is held before its injection; A ends and C starts; the
    // steer joins C; C ends and its record is evicted before the read.
    let steered_operation = OperationId::generate();
    let steer = tokio::spawn({
        let supervisor = Arc::clone(&session.supervisor);
        let request = prompt(&steered_operation, &approver_b, "second");
        async move { supervisor.prompt(request).await }
    });
    steer_pinned
        .await
        .expect("the steer is pinned before its injection");
    node_test_fixture::release_fixture(&session.receipt, ".release-1").expect("release turn A");
    let a_settled = session
        .supervisor
        .wait(ConversationOperationWaitRequest {
            operation_id: turn_a.clone(),
            timeout_seconds: PositiveSeconds::try_from(5).expect("bound"),
        })
        .await;
    let turn_c = OperationId::generate();
    let (published, release_publication) = session
        .supervisor
        .pause_after_result_publication(turn_c.clone());
    session
        .supervisor
        .prompt(prompt(&turn_c, &approver_c, "third"))
        .await
        .expect("turn C starts");
    while recorded_requests(&session.receipt, "session/prompt") < 2 {
        tokio::task::yield_now().await;
    }
    let _injected = inject.send(());
    steer_replied
        .await
        .expect("the steer's injection reply arrived");
    node_test_fixture::release_fixture(&session.receipt, ".release-2").expect("release turn C");
    let c_settled = session
        .supervisor
        .wait(ConversationOperationWaitRequest {
            operation_id: turn_c.clone(),
            timeout_seconds: PositiveSeconds::try_from(5).expect("bound"),
        })
        .await;
    published.await.expect("turn C's result is visible");
    for _ in 0..=MAX_RETAINED_SETTLEMENTS {
        let other = OperationId::generate();
        session.supervisor.hold_live_operation(other.clone());
        assert!(session.supervisor.publish_held_result(&other).await);
    }
    let c_evicted = session.supervisor.held_result_for_test(&turn_c).is_none();
    let _released = release_publication.send(());
    let _read = read_approver.send(());
    let submitted = steer.await.expect("steer task");
    let steered = session
        .supervisor
        .wait(ConversationOperationWaitRequest {
            operation_id: steered_operation,
            timeout_seconds: PositiveSeconds::try_from(5).expect("bound"),
        })
        .await;
    session.queue.shutdown().await;
    let stopped = session.supervisor.shutdown().await;

    // Assert
    assert!(
        a_settled.is_ok(),
        "turn A ended before the injection: {a_settled:?}"
    );
    assert!(
        c_settled.is_ok(),
        "turn C ended before the read: {c_settled:?}"
    );
    assert!(c_evicted, "turn C's settlement was evicted");
    assert!(submitted.is_ok(), "the steer was admitted: {submitted:?}");
    match steered {
        Ok(collaboration_protocol::ConversationOperationWaitResult {
            output:
                collaboration_protocol::ConversationOperationWaitOutput::Available {
                    settlement: ConversationOperationSettlement::Steered { turn_approver, .. },
                },
            ..
        }) => assert_eq!(
            turn_approver,
            Some(approver_c),
            "the joined turn's approver"
        ),
        other => panic!("expected a steered settlement, got {other:?}"),
    }
    stopped.expect("supervisor shutdown");
}

fn human(id: &str) -> collaboration_protocol::ProviderIdentity {
    serde_json::from_value(serde_json::json!({"kind": "human", "humanId": id})).expect("human")
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
