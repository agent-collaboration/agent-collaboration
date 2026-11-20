//! A steered API prompt joins the running turn correctly (D1 R12). V22 (Lead
//! 2026-11-16): it is taken under the input ID its request carried. The
//! request comes in through the supervisor's API backend
//! (`ProviderConversationBackend::prompt`, what the API service calls with the
//! caller's request) with a chosen input ID; the session's hub must record
//! InputAccepted with exactly that ID. Review F3 follow-up: it names the joined
//! turn's approver even when that turn ended before the steer read it.

use super::*;
use collaboration_contracts::SessionEventHub as _;
use session_event_model::SessionEvent;

/// A steering agent that holds its first turn open, on a supervisor whose
/// session is recorded on a real hub.
async fn create_test_session(
    supervisor: &ExternalProviderSupervisor,
    expected: &SessionRef,
    cwd: &std::path::Path,
) {
    let operation_id = OperationId::generate();
    supervisor
        .create(ConversationCreateRequest {
            operation_id: operation_id.clone(),
            endpoint: expected.endpoint.clone(),
            generation: None,
            working_directory: ProviderWorkingDirectory::try_from(cwd.display().to_string())
                .expect("fixture working directory"),
            created_by: requester().into(),
            approver: requester().into(),
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
            settings: Some(collaboration_protocol::ProviderRequestedSettings {
                mode: Some("auto".to_owned()),
                ..Default::default()
            }),
            work_roots: Vec::new(),
        })
        .await
        .expect("admit fixture create");
    let created = supervisor
        .wait(ConversationOperationWaitRequest {
            operation_id,
            timeout_seconds: PositiveSeconds::try_from(10).expect("bound"),
        })
        .await
        .expect("settle fixture create");
    assert!(
        matches!(
            &created.output,
            ConversationOperationWaitOutput::Available {
                settlement: ConversationOperationSettlement::Created { target, .. },
            } if target == expected
        ),
        "created fixture identity: {:?}",
        created.output
    );
}

struct SteeringSession {
    _root: tempfile::TempDir,
    receipt: std::path::PathBuf,
    target: SessionRef,
    hub: Arc<collaboration_runtime::ProviderSessionEventHub>,
    hub_target: message_board::SessionRef,
    supervisor: Arc<ExternalProviderSupervisor>,
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
    steering_session_on_clock(
        program,
        session_id,
        arguments,
        Arc::new(router_clock::SystemRuntimeClock::new()),
    )
    .await
}

async fn steering_session_on_clock(
    program: &str,
    session_id: &str,
    arguments: impl FnOnce(&std::path::Path) -> Vec<String>,
    clock: Arc<dyn router_clock::RuntimeClock>,
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
        clock,
    )
    .await
    .expect("fixture provider");
    let supervisor = Arc::new(
        ExternalProviderSupervisor::new_with_hub(
            vec![ExternalProviderBinding {
                identity: binding(),
                runtime,
            }],
            Arc::clone(&store),
            Some(hub.clone()),
            None,
        )
        .expect("supervisor"),
    );
    create_test_session(&supervisor, &target, root.path()).await;
    SteeringSession {
        _root: root,
        receipt,
        target,
        hub,
        hub_target,
        supervisor,
    }
}

fn send_race_arguments(receipt: &std::path::Path, behavior: &str) -> Vec<String> {
    vec![
        "send-race".into(),
        receipt.display().to_string(),
        behavior.into(),
    ]
}

#[tokio::test]
async fn busy_with_steering_uses_the_running_owner_answer() {
    use provider_session_machines::session_refusal_answers::session_busy_answer;

    let rig = steering_session_on_clock(
        "provider-steering-agent.ts",
        "fixture-session",
        |receipt| send_race_arguments(receipt, "first-turn-ends-max-tokens"),
        Arc::new(router_clock::ManualClock::new(chrono::DateTime::UNIX_EPOCH)),
    )
    .await;
    let prompt = |text: &str| ConversationPromptRequest {
        delivery: collaboration_protocol::MessageDelivery::Auto,
        operation_id: OperationId::generate(),
        input_id: None,
        target: rig.target.clone(),
        generation: None,
        requested_by: rig.target.clone().into(),
        approver: rig.target.clone().into(),
        prompt: collaboration_protocol::MessageContent::HumanUser {
            text: MessageText::try_from(text.to_owned()).expect("prompt text"),
        },
    };
    rig.supervisor
        .prompt(prompt("held foreground"))
        .await
        .expect("foreground admitted");
    while recorded_requests(&rig.receipt, "session/prompt") == 0 {
        tokio::task::yield_now().await;
    }
    let snapshot = rig
        .supervisor
        .owner_session_snapshot(&rig.target)
        .expect("live snapshot");
    let steer = snapshot
        .projection
        .capabilities
        .as_ref()
        .is_some_and(|report| report.steer);
    assert!(steer, "fixture advertises steering");
    let mut direct =
        super::provider_prompt_contents::ProviderPromptContentsRequest::from_prompt_request(
            prompt("direct request"),
        )
        .expect("input");
    direct.busy = super::provider_prompt_contents::PromptBusyPolicy::RefuseBusy;
    let operation = direct.operation_id.clone();
    let failure = match rig.supervisor.prompt_contents(direct).await {
        Ok(_) => rig
            .supervisor
            .wait(ConversationOperationWaitRequest {
                operation_id: operation,
                timeout_seconds: PositiveSeconds::try_from(5).expect("wait budget"),
            })
            .await
            .expect_err("direct prompt can't take the running turn"),
        Err(failure) => failure,
    };
    let answer = String::from(failure.message);
    assert_eq!(
        answer,
        "a turn is running; steer it, queue the message or cancel the turn"
    );
    assert_eq!(
        answer,
        session_busy_answer(&snapshot.lifecycle, snapshot.turn, steer)
    );
    node_test_fixture::release_fixture(&rig.receipt, ".release").expect("release foreground");
    rig.supervisor.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn a_fenced_setting_uses_the_setting_in_progress_answer() {
    use collaboration_protocol::{
        ProviderSettingName, ProviderSettingsAcceptRequest, ProviderSettingsFailureKind,
        ProviderSettingsSetRequest,
    };
    use provider_session_machines::{
        session_refusal_answers::{SettingWaitObservation, setting_in_progress_answer},
        session_setting_machine::{SessionSetting, SettingValueId, UnresolvedCause},
    };
    use router_clock::RuntimeClock as _;

    let clock = Arc::new(router_clock::ManualClock::new(chrono::DateTime::UNIX_EPOCH));
    let rig = steering_session_on_clock(
        "provider-steering-agent.ts",
        "fixture-session",
        |receipt| send_race_arguments(receipt, "hold-effort-change"),
        clock.clone(),
    )
    .await;
    let request = ProviderSettingsSetRequest {
        target: rig.target.clone(),
        actor: requester().into(),
        setting: ProviderSettingName::Effort,
        value: "high".into(),
    };
    let first = rig.supervisor.settings_set(request.clone());
    tokio::pin!(first);
    tokio::select! {
        () = async {
            while recorded_requests(&rig.receipt, "session/set_config_option") == 0 {
                tokio::task::yield_now().await;
            }
        } => {}
        result = &mut first => panic!("the first setting must wait for its held reply: {result:?}"),
    }
    let budget = std::time::Duration::from_secs(30);
    clock
        .not_due_check_now(clock.monotonic_now().after(budget))
        .await;
    clock
        .advance(budget)
        .expect("expire the first setting reply");
    assert_eq!(
        first
            .await
            .expect_err("the setting outcome is unknown")
            .kind,
        ProviderSettingsFailureKind::OutcomeUnknown
    );
    let failure = rig
        .supervisor
        .settings_set(request)
        .await
        .expect_err("the same kind is still fenced");
    assert_eq!(failure.kind, ProviderSettingsFailureKind::Busy);
    assert_eq!(
        failure.message,
        "a setting change is in progress; wait for the earlier write's reply"
    );
    assert_eq!(
        failure.message,
        setting_in_progress_answer(
            Some(&SessionSetting::Unresolved {
                cause: UnresolvedCause::OutcomeUnknown,
                observed: SettingValueId("medium".into()),
            }),
            |_| false,
            SettingWaitObservation::EarlierWriteReply,
        )
    );
    let accept = rig
        .supervisor
        .settings_accept(ProviderSettingsAcceptRequest {
            target: rig.target.clone(),
            actor: requester().into(),
        })
        .await
        .expect_err("the unknown outcome also prevents accepting settings");
    assert_eq!(accept.kind, ProviderSettingsFailureKind::OutcomeUnknown);
    assert_eq!(
        recorded_requests(&rig.receipt, "session/set_config_option"),
        1
    );
    node_test_fixture::release_fixture(&rig.receipt, ".setting-release")
        .expect("release first reply");
    rig.supervisor.shutdown().await.expect("shutdown");
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
    } = steering_session().await;
    let held = OperationId::generate();
    supervisor
        .prompt(ConversationPromptRequest {
            delivery: collaboration_protocol::MessageDelivery::Auto,
            operation_id: held.clone(),
            input_id: None,
            target: target.clone(),
            generation: None,
            requested_by: target.clone().into(),
            approver: target.clone().into(),
            prompt: collaboration_protocol::MessageContent::HumanUser {
                text: MessageText::try_from("first".to_owned()).expect("prompt"),
            },
        })
        .await
        .expect("held prompt admitted");
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
        if let Some(input) = followed
            .snapshot
            .iter()
            .find_map(|observed| match &observed.event {
                SessionEvent::InputAccepted { input_id, .. } => Some(input_id.clone()),
                _ => None,
            })
        {
            break vec![input];
        }
        match followed
            .receiver
            .recv()
            .await
            .expect("the hub follows the turn")
            .event
        {
            SessionEvent::InputAccepted { input_id, .. } => break vec![input_id],
            SessionEvent::ResyncRequired { replay_epoch } => {
                followed = hub
                    .attach(hub_target.clone())
                    .await
                    .expect("follow the current epoch");
                assert!(followed.epoch >= replay_epoch);
            }
            _ => {}
        }
    };
    node_test_fixture::release_fixture(&receipt, ".release").expect("release the turn");
    let held = supervisor
        .wait(ConversationOperationWaitRequest {
            operation_id: held,
            timeout_seconds: PositiveSeconds::try_from(5).expect("bound"),
        })
        .await;
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

#[tokio::test]
async fn operation_cancel_interrupts_an_input_taken_while_cancel_waited_for_admission() {
    use collaboration_protocol::{
        ConversationCancelRequest, MessageDelivery, ProviderPromptStopReason,
    };

    let rig = steering_session_on_clock(
        "cursor-settings-agent.ts",
        "reactive-session-1",
        |receipt| {
            vec![
                "reactive".to_owned(),
                receipt.display().to_string(),
                "hold-first-prompt-open".to_owned(),
                "prompt-directives".to_owned(),
            ]
        },
        Arc::new(router_clock::ManualClock::new(chrono::DateTime::UNIX_EPOCH)),
    )
    .await;
    let prompt = |text: &str, delivery| ConversationPromptRequest {
        delivery,
        operation_id: OperationId::generate(),
        input_id: None,
        target: rig.target.clone(),
        generation: Some(binding().generation),
        requested_by: rig.target.clone().into(),
        approver: rig.target.clone().into(),
        prompt: collaboration_protocol::MessageContent::HumanUser {
            text: MessageText::try_from(text.to_owned()).expect("prompt text"),
        },
    };
    rig.supervisor
        .prompt(prompt("first", MessageDelivery::Auto))
        .await
        .expect("first prompt");
    while recorded_requests(&rig.receipt, "session/prompt") != 1 {
        tokio::task::yield_now().await;
    }
    let queued_request = prompt("hold-open", MessageDelivery::Queue);
    let requested = queued_request.operation_id.clone();
    let requested_id = uuid::Uuid::parse_str(requested.as_str()).expect("prompt operation UUID");
    rig.supervisor
        .prompt(queued_request)
        .await
        .expect("queued prompt");
    let owner = rig
        .supervisor
        .session_owner_for(&rig.target)
        .expect("session owner");
    let mut snapshots = owner.snapshots();
    snapshots
        .wait_for(|snapshot| {
            snapshot
                .queue
                .iter()
                .any(|entry| !entry.started && entry.input.operation == requested_id)
        })
        .await
        .expect("target still queued");

    let (entered, reached) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    *rig.supervisor
        .inner
        .cancel_admission_test_pause
        .lock()
        .expect("test pause") = Some(AdmissionTestPause {
        entered,
        release: released,
    });
    let cancel = OperationId::generate();
    rig.supervisor
        .cancel(ConversationCancelRequest {
            operation_id: cancel.clone(),
            target_operation_id: requested.clone(),
            target: rig.target.clone(),
            generation: Some(binding().generation),
            requested_by: rig.target.clone().into(),
            approver: rig.target.clone().into(),
        })
        .await
        .expect("cancel operation admitted at Host");
    reached.await.expect("cancel waiting at owner admission");
    node_test_fixture::release_fixture(&rig.receipt, ".release").expect("finish first prompt");
    snapshots
        .wait_for(|snapshot| {
            snapshot
                .foreground
                .as_ref()
                .is_some_and(|input| input.operation == requested_id)
        })
        .await
        .expect("queued input became the foreground");
    while recorded_requests(&rig.receipt, "session/prompt") != 2 {
        tokio::task::yield_now().await;
    }
    release
        .send(())
        .expect("send cancellation after writer takes the queued input");

    let wait_request = |operation_id| ConversationOperationWaitRequest {
        operation_id,
        timeout_seconds: PositiveSeconds::try_from(10).expect("bounded wait"),
    };
    let cancelled = rig
        .supervisor
        .wait(wait_request(cancel))
        .await
        .expect("cancel the foreground by operation");
    assert!(
        matches!(cancelled.output, ConversationOperationWaitOutput::Available {
        settlement: ConversationOperationSettlement::CancelRequested { target_operation_id, .. }
    } if target_operation_id == requested)
    );
    let stopped = rig
        .supervisor
        .wait(wait_request(requested))
        .await
        .expect("prompt finishes");
    assert!(matches!(
        stopped.output,
        ConversationOperationWaitOutput::Available {
            settlement: ConversationOperationSettlement::PromptCompleted {
                stop_reason: ProviderPromptStopReason::Cancelled,
                ..
            }
        }
    ));
    assert_eq!(recorded_requests(&rig.receipt, "session/cancel"), 1);
    rig.supervisor.shutdown().await.expect("fixture shutdown");
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
