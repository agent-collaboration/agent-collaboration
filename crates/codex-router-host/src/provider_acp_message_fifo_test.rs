use super::*;
use crate::{
    ExternalProviderBinding, ExternalProviderLaunch, ExternalProviderRuntime,
    ExternalProviderSupervisor, LiveSessionOwnership, LiveSessionOwnershipCheck,
    ProviderAcpDeliveryRoute, ProviderSessionActivity,
};
use agent_automation::RouteEffectEvidence;
use collaboration_contracts::AttemptEvidenceSink;
use collaboration_contracts::DeliveryContractError;
use collaboration_contracts::DeliveryFuture;
use collaboration_contracts::DeliveryPrecondition;
use collaboration_contracts::LoadPolicy;
use collaboration_contracts::ProviderConversationBackend;
use collaboration_contracts::SessionDeliveryRoute;
use collaboration_contracts::layer_zero::DeliveryRequest;
use collaboration_contracts::layer_zero::PreparedPush;
use collaboration_protocol::{
    ChannelDescription, CodexGeneration, ConversationCloseRequest, ConversationOperationQueueState,
    ConversationOperationSettlement, ConversationOperationShowRequest,
    ConversationOperationWaitOutput, ConversationOperationWaitRequest, DeliveryCorrelationId,
    DeliveryOutcome, EndpointAvailability, EndpointDescription, EndpointId, EndpointRef,
    GenerationNumber, MessageDelivery, MessageText, NonEmptyText, ObservationTimestamp,
    OperationId, PositiveSeconds, ProviderBindingId, ProviderBindingIdentity, ProviderCapabilities,
    ProviderCapability, ProviderCapabilityEvidence, ProviderCapabilityName,
    ProviderCapabilityStatus, ProviderKind, ProviderRequestedPolicy, ProviderRuntimeIdentity,
    ProviderTransport, ProviderWorkingDirectory, PushId, RouterAccess, SessionId,
};
use collaboration_runtime::EndpointDirectory;
use collaboration_runtime::ProviderOperationStore;
use collaboration_runtime::ProviderSessionRecord;
use std::sync::Arc;

struct NoLivePeer;

impl LiveSessionOwnershipCheck for NoLivePeer {
    fn check<'a>(&'a self, _target: &'a SessionRef) -> DeliveryFuture<'a, LiveSessionOwnership> {
        Box::pin(async { Ok(LiveSessionOwnership::NotLive) })
    }
}

fn target() -> SessionRef {
    SessionRef {
        endpoint: EndpointRef {
            service_id: "0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89"
                .to_owned()
                .try_into()
                .expect("service ID"),
            endpoint_id: EndpointId::try_from("cursor-local".to_owned()).expect("endpoint ID"),
        },
        session_id: SessionId::try_from("fifo-session".to_owned()).expect("session ID"),
    }
}

fn binding(target: &SessionRef) -> ProviderBindingIdentity {
    ProviderBindingIdentity {
        endpoint: target.endpoint.clone(),
        binding_id: ProviderBindingId::try_from("fifo-binding".to_owned()).expect("binding ID"),
        runtime: ProviderRuntimeIdentity {
            provider: ProviderKind::Cursor,
            runtime_name: NonEmptyText::try_from("fifo-fixture".to_owned()).expect("runtime name"),
            runtime_version: None,
        },
        transport: ProviderTransport::StdioAcp,
        generation: CodexGeneration {
            service_epoch: "1ff962c5-7fa3-4c18-a5ca-1bbe8db09e80"
                .to_owned()
                .try_into()
                .expect("service epoch"),
            generation: GenerationNumber::try_from(1).expect("generation"),
        },
        capabilities: ProviderCapabilities::try_from(vec![
            ProviderCapability {
                name: ProviderCapabilityName::Prompt,
                status: ProviderCapabilityStatus::Supported,
                evidence: ProviderCapabilityEvidence::Advertised,
            },
            ProviderCapability {
                name: ProviderCapabilityName::Load,
                status: ProviderCapabilityStatus::Supported,
                evidence: ProviderCapabilityEvidence::Advertised,
            },
        ])
        .expect("capabilities"),
    }
}

fn close_and_load_fixture(
    close_marker: &std::path::Path,
    load_marker: &std::path::Path,
) -> ExternalProviderLaunch {
    crate::external_provider_runtime::acp_scripted_fixture::launch_fixture_agent(
        "host-fifo-load-agent.ts",
        vec![
            "close".to_owned(),
            close_marker.to_string_lossy().into_owned(),
            load_marker.to_string_lossy().into_owned(),
        ],
    )
}

#[tokio::test]
async fn queued_loaded_only_item_not_submitted_if_close_happens_before_fifo_drain() {
    let root = tempfile::tempdir().expect("fixture root");
    let close_marker = root.path().join("close-marker.txt");
    let load_marker = root.path().join("load-marker.txt");
    let target = target();
    let binding = binding(&target);
    let runtime = ExternalProviderRuntime::initialize(
        close_and_load_fixture(&close_marker, &load_marker),
        std::sync::Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await
    .expect("fixture provider");
    assert_eq!(
        runtime
            .create_session(root.path().to_owned())
            .await
            .expect("session/new"),
        "fifo-session"
    );
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

    let operation_id = OperationId::generate();
    let input_id = session_event_model::InputId::generate();
    let prompt = MessageText::try_from("held batch".to_owned()).expect("prompt line");
    let contents =
        crate::external_provider_supervisor::ProviderPromptContentsRequest::from_prepared_push(
            operation_id.clone(),
            input_id.clone(),
            target.clone(),
            target.clone().into(),
            target.clone().into(),
            &prompt,
        )
        .expect("prepared prompt contents");
    supervisor
        .queued_operation_registry()
        .record_queued_contents(
            operation_id.clone(),
            target.clone(),
            binding.clone(),
            input_id.clone(),
            match &contents.input {
                acp_client_runtime::ProviderTurnInput::Prompt(submission) => &submission.content,
                acp_client_runtime::ProviderTurnInput::Compact => panic!("queued ordinary input"),
            },
        );
    let (sender, receiver) = mpsc::channel(1);
    sender
        .send(super::QueuedEntry {
            sequence: 0,
            prompt: ProviderQueuedPrompt::Contents {
                request: contents,
                load_policy: LoadPolicy::LoadedOnly,
                admitted: None,
            },
        })
        .await
        .expect("accepted queue item");
    drop(sender);

    supervisor
        .runtime_for(&target.endpoint)
        .expect("provider runtime")
        .close_session(String::from(target.session_id.clone()))
        .await
        .expect("ConversationClose unloads provider session");
    assert_eq!(
        supervisor
            .runtime_for(&target.endpoint)
            .expect("provider runtime")
            .session_activity(String::from(target.session_id.clone()))
            .await
            .expect("session activity"),
        ProviderSessionActivity::NotLoaded
    );

    let worker = tokio::spawn(run_provider_message_fifo(
        target.clone(),
        receiver,
        Arc::clone(&supervisor),
        Arc::clone(&store),
        Arc::new(NoLivePeer),
        Arc::new(super::SessionLineState::new(CancellationToken::new())),
        Arc::new(tokio::sync::Notify::new()),
    ));
    tokio::time::timeout(std::time::Duration::from_secs(2), worker)
        .await
        .expect("FIFO worker drains accepted item")
        .expect("FIFO worker completes");

    let snapshot = supervisor
        .show(ConversationOperationShowRequest { operation_id })
        .await
        .expect("queued operation snapshot");
    assert_eq!(
        snapshot.queue_state,
        Some(
            collaboration_protocol::ConversationOperationQueueState::NotSubmitted {
                reason: NOT_LOADED_REASON.to_owned(),
            }
        )
    );
    assert!(
        close_marker.exists(),
        "fixture close must complete before FIFO drain"
    );
    assert!(
        !load_marker.exists(),
        "LoadedOnly FIFO must not call session/load"
    );
    supervisor.shutdown().await.expect("supervisor shutdown");
}

struct CloseDuringQueueAcceptance {
    supervisor: Arc<ExternalProviderSupervisor>,
    target: SessionRef,
}

impl AttemptEvidenceSink for CloseDuringQueueAcceptance {
    fn record(
        &self,
        _: RouteEffectEvidence<SessionRef, CodexGeneration>,
    ) -> DeliveryFuture<'_, ()> {
        let supervisor = Arc::clone(&self.supervisor);
        let target = self.target.clone();
        Box::pin(async move {
            let operation_id = OperationId::generate();
            supervisor
                .close(ConversationCloseRequest {
                    operation_id: operation_id.clone(),
                    target: target.clone(),
                    generation: None,
                    requested_by: target.clone().into(),
                    approver: target.clone().into(),
                })
                .await
                .map_err(|_| DeliveryContractError::ClientOperation)?;
            let timeout_seconds =
                PositiveSeconds::try_from(5).map_err(|_| DeliveryContractError::ClientOperation)?;
            let closed = supervisor
                .wait(ConversationOperationWaitRequest {
                    operation_id,
                    timeout_seconds,
                })
                .await
                .map_err(|_| DeliveryContractError::ClientOperation)?;
            if !matches!(
                closed.output,
                ConversationOperationWaitOutput::Available {
                    settlement: ConversationOperationSettlement::Closed { target: closed_target },
                } if closed_target == target
            ) {
                return Err(DeliveryContractError::ClientOperation);
            }
            Ok(())
        })
    }
}

#[tokio::test]
async fn route_queue_loaded_only_delivery_refuses_after_close_before_fifo_submission() {
    let root = tempfile::tempdir().expect("fixture root");
    let close_marker = root.path().join("close-marker.txt");
    let load_marker = root.path().join("load-marker.txt");
    let target = target();
    let provider_binding = binding(&target);
    let runtime = ExternalProviderRuntime::initialize(
        close_and_load_fixture(&close_marker, &load_marker),
        std::sync::Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await
    .expect("fixture provider");
    assert_eq!(
        runtime
            .create_session(root.path().to_owned())
            .await
            .expect("session/new"),
        "fifo-session"
    );
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
                identity: provider_binding.clone(),
                runtime,
            }],
            Arc::clone(&store),
        )
        .expect("supervisor"),
    );
    let directory = EndpointDirectory::new(target.endpoint.service_id.clone());
    directory
        .publish(EndpointDescription {
            endpoint: target.endpoint.clone(),
            label: NonEmptyText::try_from("Cursor fixture".to_owned()).expect("label"),
            availability: EndpointAvailability::Available {
                observed_at: ObservationTimestamp::try_from("2026-09-24T12:00:00Z".to_owned())
                    .expect("observation time"),
            },
            channels: vec![ChannelDescription::ExternalProvider {
                transport: ProviderTransport::StdioAcp,
                binding_id: provider_binding.binding_id.clone(),
                binding_generation: provider_binding.generation.generation,
                runtime: provider_binding.runtime.clone(),
                capabilities: provider_binding.capabilities.clone(),
                auto_realization: None,
            }],
        })
        .expect("endpoint publication");
    let route = ProviderAcpDeliveryRoute::new(
        target.endpoint.service_id.clone(),
        std::iter::once(target.endpoint.clone()).collect(),
        directory,
        Arc::clone(&supervisor),
        store,
        Arc::new(NoLivePeer),
    );
    let attempt_id = agent_automation::AttemptId::generate();
    let operation_id =
        OperationId::try_from(attempt_id.as_str().to_owned()).expect("queue operation ID");
    let push_id = PushId::try_from(attempt_id.as_str().to_owned()).expect("UUIDv7 push id");
    let correlation =
        DeliveryCorrelationId::try_from(push_id.as_str().to_owned()).expect("push id correlation");
    let line = MessageText::try_from(format!(
        "✉️ sender · \"held batch\" · router://{}/push/{}",
        String::from(target.endpoint.service_id.clone()),
        push_id.as_str()
    ))
    .expect("prepared push line");
    let request = DeliveryRequest {
        payload: PreparedPush {
            push_id,
            line,
            load_policy: LoadPolicy::LoadedOnly,
        },
        target: target.clone(),
        mode: MessageDelivery::Queue,
        precondition: DeliveryPrecondition::Unpinned,
        correlation,
        attempt: attempt_id,
    };
    let receipt = route
        .deliver(
            request,
            &CloseDuringQueueAcceptance {
                supervisor: Arc::clone(&supervisor),
                target: target.clone(),
            },
        )
        .await
        .expect("queued delivery receipt");
    assert!(matches!(
        receipt.receipt().outcome.clone(),
        DeliveryOutcome::Queued
    ));
    assert!(close_marker.exists(), "session close precedes FIFO enqueue");

    let queue_state = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        supervisor
            .queued_operation_registry()
            .wait_for_not_submitted(&operation_id),
    )
    .await
    .expect("FIFO observes the closed session");
    assert_eq!(
        queue_state,
        ConversationOperationQueueState::NotSubmitted {
            reason: collaboration_contracts::NOT_LOADED_REASON.to_owned(),
        }
    );
    assert!(
        !load_marker.exists(),
        "LoadedOnly queue submission must not call session/load"
    );
    route.shutdown_queue().await;
    supervisor.shutdown().await.expect("supervisor shutdown");
}

#[tokio::test]
async fn selected_skill_removed_while_fifo_waits_is_refused_without_a_provider_prompt() {
    use collaboration_contracts::SessionEventHub as _;
    use session_event_model::{
        InputId, PromptContent, ProviderSubmission, SelectedSkill, SkillInvocationName, SkillView,
    };
    let root = tempfile::tempdir().expect("root");
    let receipt = root.path().join("wire.jsonl");
    let mut target = target();
    target.session_id = SessionId::try_from("session-1".to_owned()).expect("session ID");
    let binding = binding(&target);
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
            environment: vec![],
            persistence_target: acp_client_runtime::ProviderPersistenceTarget::Unspecified,
        },
        acp_client_runtime::ProviderModelPicker::Standard,
        "fixture",
        "http://127.0.0.1:1/mcp",
        Arc::clone(&hub),
        hub_target.endpoint.clone(),
        Arc::new(acp_provider_adapters::CursorAcpBehavior),
    )
    .await
    .expect("provider");
    runtime
        .create_session(root.path().into())
        .await
        .expect("session");
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
            created_by: target.clone().into(),
            approver: target.clone().into(),
            updated_at_ms: 1,
        })
        .await
        .expect("record session");
    let mut skills = hub
        .skill_view(hub_target, root.path().to_path_buf())
        .await
        .expect("skill subscription");
    // session/new completes before its subsequent command advertisement is observed.
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let view = skills.receiver.borrow_and_update().clone();
            if matches!(view, SkillView::Available { skills, .. }
                if skills.iter().any(|skill| skill.invocation_name.as_str() == "review"))
            {
                break;
            }
            skills.receiver.changed().await.expect("skill view changed");
        }
    })
    .await
    .expect("initial skill advertisement is observed before submission");
    let selected = || ProviderSubmission {
        content: vec![PromptContent::text("$review exact args".into()).expect("text")],
        selected: vec![SelectedSkill {
            invocation_name: SkillInvocationName::new("review").expect("name"),
        }],
    };
    let context = crate::ExternalProviderApprovalContext {
        requester: target.clone().into(),
        approver: target.clone().into(),
        target: target.clone(),
        operation_id: OperationId::generate(),
        binding_generation: binding.generation.clone(),
        binding_retirement: runtime.retirement(),
    };
    runtime
        .submit_with_approval_dispatch_for_input(
            "session-1".into(),
            InputId::generate(),
            acp_client_runtime::ProviderTurnInput::Prompt(selected()),
            context,
            None,
        )
        .await
        .expect("skill is initially current and invocable");
    let supervisor = Arc::new(
        ExternalProviderSupervisor::new(
            vec![ExternalProviderBinding {
                identity: binding.clone(),
                runtime,
            }],
            store.clone(),
        )
        .expect("supervisor"),
    );
    let held_operation = OperationId::generate();
    supervisor
        .prompt(collaboration_protocol::ConversationPromptRequest {
            operation_id: held_operation.clone(),
            input_id: None,
            target: target.clone(),
            generation: None,
            requested_by: target.clone().into(),
            approver: target.clone().into(),
            prompt: collaboration_protocol::MessageContent::HumanUser {
                text: MessageText::try_from("hold".to_owned()).expect("held prompt"),
            },
        })
        .await
        .expect("held prompt dispatched");
    let queue =
        ProviderAcpMessageFifo::new(supervisor.clone(), store.clone(), Arc::new(NoLivePeer));
    let queued_operation = OperationId::generate();
    let input_id = InputId::generate();
    let submission = selected();
    supervisor
        .queued_operation_registry()
        .record_queued_contents(
            queued_operation.clone(),
            target.clone(),
            binding.clone(),
            input_id.clone(),
            &submission.content,
        );
    queue
        .reserve(&target)
        .expect("queue capacity")
        .send(ProviderQueuedPrompt::Contents {
            request: crate::external_provider_supervisor::ProviderPromptContentsRequest {
                operation_id: queued_operation.clone(),
                input_id,
                target: target.clone(),
                requested_by: target.clone().into(),
                approver: target.clone().into(),
                input: acp_client_runtime::ProviderTurnInput::Prompt(submission),
                busy: crate::external_provider_supervisor::PromptBusyPolicy::RefuseBusy,
            },
            load_policy: LoadPolicy::MayLoad,
            admitted: None,
        });
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        queue.wait_started.notified(),
    )
    .await
    .expect("FIFO reached its idle wait while the peer turn is held");
    supervisor
        .runtime_for(&target.endpoint)
        .expect("runtime")
        .cancel_active_prompt("session-1".into())
        .await
        .expect("release held turn, emitting skill removal before its terminal");
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        supervisor
            .queued_operation_registry()
            .wait_for_cleared(&queued_operation),
    )
    .await
    .expect("queued actor refusal settles");
    let rejected = supervisor
        .wait(ConversationOperationWaitRequest {
            operation_id: queued_operation.clone(),
            timeout_seconds: PositiveSeconds::try_from(1).expect("bound"),
        })
        .await
        .expect_err("stale selection is a closed failure");
    assert_eq!(
        rejected.kind,
        collaboration_protocol::ConversationOperationFailureKind::InvalidRequest
    );
    assert_eq!(
        rejected.effect,
        collaboration_protocol::ProviderOperationEffect::None
    );
    let stored = store
        .lock()
        .await
        .inspect(&queued_operation)
        .await
        .expect("stored result")
        .expect("admitted queued operation");
    assert_eq!(stored.stage, ProviderOperationStage::Terminal);
    let wire = std::fs::read_to_string(&receipt)
        .expect("wire receipt")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("wire JSON"))
        .collect::<Vec<_>>();
    assert_eq!(
        wire.len(),
        2,
        "only initially valid skill and held prompt, no stale queued send"
    );
    assert_eq!(wire[0]["params"]["prompt"][0]["text"], "/review exact args");
    assert_eq!(wire[1]["params"]["prompt"][0]["text"], "hold");
    queue.shutdown().await;
    supervisor
        .shutdown()
        .await
        .expect("owned provider shutdown");
}

/// How many requests with `method` the Cursor fixture recorded in `receipt`.
fn recorded_requests(receipt: &std::path::Path, method: &str) -> usize {
    std::fs::read(receipt)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<serde_json::Value>>(&bytes).ok())
        .map_or(0, |requests| {
            requests
                .iter()
                .filter(|request| request["method"] == method)
                .count()
        })
}

/// Oracle: D1 spec R14 and V27 — "close while queued: every undispatched ID
/// becomes not sent with the reason, and no later load or prompt reaches the
/// agent", with the worker holding a head and more buffered. The turn is held
/// with no operation row, so the line's own close is reached directly.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn close_while_queued_marks_head_and_buffer_not_sent() {
    // Arrange: a turn held on the runtime, then three API prompts queued
    // behind it; the worker holds the first at its idle wait.
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
    let mut queued = Vec::new();
    for text in ["queued-0", "queued-1", "queued-2"] {
        let operation_id = OperationId::generate();
        supervisor
            .prompt(collaboration_protocol::ConversationPromptRequest {
                operation_id: operation_id.clone(),
                input_id: None,
                target: target.clone(),
                generation: None,
                requested_by: target.clone().into(),
                approver: target.clone().into(),
                prompt: collaboration_protocol::MessageContent::HumanUser {
                    text: MessageText::try_from(text.to_owned()).expect("prompt"),
                },
            })
            .await
            .expect("queued behind the held turn");
        queued.push(operation_id);
    }
    queue.wait_started.notified().await;
    let mut shown_before = Vec::new();
    for operation_id in &queued {
        shown_before.push(
            supervisor
                .show(ConversationOperationShowRequest {
                    operation_id: operation_id.clone(),
                })
                .await
                .expect("queued on record")
                .queue_state,
        );
    }

    // Act: the session's line closes; then the held turn ends.
    queue.close_session(&target, "session closed");
    let mut settled = Vec::new();
    for operation_id in &queued {
        settled.push(
            supervisor
                .wait(ConversationOperationWaitRequest {
                    operation_id: operation_id.clone(),
                    timeout_seconds: PositiveSeconds::try_from(5).expect("bound"),
                })
                .await,
        );
    }
    std::fs::write(format!("{}.release", receipt.display()), b"").expect("release the turn");
    let held = held.await.expect("held turn task");
    queue.shutdown().await;
    let prompts = recorded_requests(&receipt, "session/prompt");
    let loads = recorded_requests(&receipt, "session/load");
    let stopped = supervisor.shutdown().await;

    // Assert: every undispatched ID, the held head included, is not sent
    // with the reason, and nothing more reached the agent.
    assert!(
        shown_before
            .iter()
            .all(|state| state == &Some(ConversationOperationQueueState::RouterQueued)),
        "{shown_before:?}"
    );
    for waited in &settled {
        let failure = waited
            .as_ref()
            .err()
            .unwrap_or_else(|| panic!("a waiting ID ends not sent: {waited:?}"));
        assert_eq!(
            String::from(failure.message.clone()),
            "not sent: session closed"
        );
        assert_eq!(
            failure.effect,
            collaboration_protocol::ProviderOperationEffect::None
        );
    }
    assert!(held.is_ok(), "the held turn ran to its end: {held:?}");
    assert_eq!(prompts, 1, "no queued prompt reached the agent");
    assert_eq!(loads, 0, "no later load reached the agent");
    stopped.expect("supervisor shutdown without an operation-drain timeout");
}
