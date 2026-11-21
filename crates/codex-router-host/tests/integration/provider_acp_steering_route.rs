//! A running Claude prompt identifies the operation receiving a steered message.
use crate::provider_fixture_launch;

use agent_automation::{RouteEffectEvidence, SubmissionEffect};
use codex_router_host::{
    ExternalProviderBinding, ExternalProviderLaunch, ExternalProviderRuntime,
    ExternalProviderSupervisor, LiveSessionOwnership, LiveSessionOwnershipCheck,
    ProviderAcpDeliveryRoute,
};
use collaboration_contracts::AttemptEvidenceSink;
use collaboration_contracts::AttemptReconciliation;
use collaboration_contracts::AttemptReconciliationContext;
use collaboration_contracts::DeliveryFuture;
use collaboration_contracts::DeliveryPrecondition;
use collaboration_contracts::ProviderConversationBackend;
use collaboration_contracts::SessionDeliveryRoute;
use collaboration_contracts::layer_zero::DeliveryRequest;
use collaboration_contracts::layer_zero::PreparedPush;
use collaboration_protocol::{
    AttemptId, ChannelDescription, CodexGeneration, ConversationPromptRequest,
    DeliveryClientReceipt, DeliveryCorrelationId, DeliveryOutcome, EndpointAvailability,
    EndpointDescription, EndpointId, EndpointRef, GenerationNumber, MessageContent,
    MessageDelivery, MessageText, NonEmptyText, ObservationTimestamp, OperationId,
    ProviderBindingId, ProviderBindingIdentity, ProviderCapabilities, ProviderCapability,
    ProviderCapabilityEvidence, ProviderCapabilityName, ProviderCapabilityStatus, ProviderKind,
    ProviderRequestedPolicy, ProviderRuntimeIdentity, ProviderTransport, ProviderWorkingDirectory,
    PushId, RouterAccess, SessionId, SessionRef, UuidIdentity,
};
use collaboration_runtime::EndpointDirectory;
use collaboration_runtime::ProviderOperationStore;
use collaboration_runtime::ProviderSessionRecord;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

struct NoLivePeer;

impl LiveSessionOwnershipCheck for NoLivePeer {
    fn check<'a>(&'a self, _target: &'a SessionRef) -> DeliveryFuture<'a, LiveSessionOwnership> {
        Box::pin(async { Ok(LiveSessionOwnership::NotLive) })
    }
}

struct RecordedEvidence(tokio::sync::Mutex<Vec<RouteEffectEvidence<SessionRef, CodexGeneration>>>);

impl AttemptEvidenceSink for RecordedEvidence {
    fn retain_queued_dispatch(&self) -> Option<std::sync::Arc<dyn AttemptEvidenceSink>> {
        collaboration_contracts::UnstoredAttemptEvidenceSink.retain_queued_dispatch()
    }

    fn record(
        &self,
        evidence: RouteEffectEvidence<SessionRef, CodexGeneration>,
    ) -> DeliveryFuture<'_, ()> {
        Box::pin(async move {
            self.0.lock().await.push(evidence);
            Ok(())
        })
    }
}

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
fn target() -> SessionRef {
    SessionRef {
        endpoint: EndpointRef {
            service_id: UuidIdentity::try_from("0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89".to_owned())
                .expect("service"),
            endpoint_id: EndpointId::try_from("claude-local".to_owned()).expect("endpoint"),
        },
        session_id: SessionId::try_from("fixture-session".to_owned()).expect("session"),
    }
}

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
fn binding(target: &SessionRef) -> ProviderBindingIdentity {
    ProviderBindingIdentity {
        endpoint: target.endpoint.clone(),
        binding_id: ProviderBindingId::try_from("claude-fixture-binding".to_owned())
            .expect("binding"),
        runtime: ProviderRuntimeIdentity {
            provider: ProviderKind::ClaudeCode,
            runtime_name: NonEmptyText::try_from("claude-fixture".to_owned()).expect("runtime"),
            runtime_version: None,
        },
        transport: ProviderTransport::StdioAcp,
        generation: CodexGeneration {
            service_epoch: UuidIdentity::try_from(
                "1ff962c5-7fa3-4c18-a5ca-1bbe8db09e80".to_owned(),
            )
            .expect("epoch"),
            generation: GenerationNumber::try_from(1).expect("generation"),
        },
        capabilities: ProviderCapabilities::try_from(vec![ProviderCapability {
            name: ProviderCapabilityName::Prompt,
            status: ProviderCapabilityStatus::Supported,
            evidence: ProviderCapabilityEvidence::Advertised,
        }])
        .expect("capabilities"),
    }
}

fn steering_fixture(event_socket: &Path) -> ExternalProviderLaunch {
    provider_fixture_launch::launch(
        "provider-steering-route-agent.ts",
        vec![
            "active".to_owned(),
            event_socket.to_string_lossy().into_owned(),
        ],
    )
}

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
async fn prepared_route(
    root: &Path,
    launch: ExternalProviderLaunch,
) -> (
    ProviderAcpDeliveryRoute,
    Arc<ExternalProviderSupervisor>,
    SessionRef,
    CodexGeneration,
) {
    let target = target();
    let binding = binding(&target);
    let generation = binding.generation.clone();
    let runtime = ExternalProviderRuntime::initialize(
        launch,
        std::sync::Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await
    .expect("fixture provider");
    let store = Arc::new(tokio::sync::Mutex::new(
        ProviderOperationStore::open(&root.join("operations.sqlite"))
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
    let directory = EndpointDirectory::new(target.endpoint.service_id.clone());
    directory
        .publish(EndpointDescription {
            endpoint: target.endpoint.clone(),
            label: NonEmptyText::try_from("Claude Code".to_owned()).expect("label"),
            availability: EndpointAvailability::Available {
                observed_at: ObservationTimestamp::try_from("2026-09-24T12:00:00Z".to_owned())
                    .expect("time"),
            },
            channels: vec![ChannelDescription::ExternalProvider {
                transport: ProviderTransport::StdioAcp,
                binding_id: binding.binding_id,
                binding_generation: binding.generation.generation,
                runtime: binding.runtime,
                capabilities: binding.capabilities,
                auto_realization: None,
            }],
        })
        .expect("endpoint");
    let route = ProviderAcpDeliveryRoute::new(
        target.endpoint.service_id.clone(),
        std::iter::once(target.endpoint.clone()).collect(),
        directory,
        Arc::clone(&supervisor),
        store,
        Arc::new(NoLivePeer),
        std::sync::Arc::new(router_clock::SystemRuntimeClock::new()),
    );
    (route, supervisor, target, generation)
}

#[derive(Clone, Copy)]
enum IdleSteeringScenario {
    SteerOnly,
    AutoStartsPrompt,
}

fn idle_steering_fixture(scenario: IdleSteeringScenario) -> ExternalProviderLaunch {
    let mode = match scenario {
        IdleSteeringScenario::SteerOnly => "idle",
        IdleSteeringScenario::AutoStartsPrompt => "auto",
    };
    provider_fixture_launch::launch("provider-steering-route-agent.ts", vec![mode.to_owned()])
}

#[expect(
    clippy::expect_used,
    reason = "The wire fixture must reach a held foreground prompt."
)]
async fn active_steering_route(
    root: &Path,
    mode: &str,
) -> (
    ProviderAcpDeliveryRoute,
    Arc<ExternalProviderSupervisor>,
    SessionRef,
) {
    let socket = root.join("active-events.sock");
    let wire = root.join("active-wire.jsonl");
    let listener = tokio::net::UnixListener::bind(&socket).expect("fixture events");
    let (route, supervisor, target, generation) = prepared_route(
        root,
        provider_fixture_launch::launch(
            "provider-steering-route-agent.ts",
            vec![
                mode.to_owned(),
                socket.display().to_string(),
                wire.display().to_string(),
            ],
        ),
    )
    .await;
    let operation = OperationId::generate();
    supervisor
        .prompt(ConversationPromptRequest {
            delivery: MessageDelivery::Auto,
            input_id: None,
            operation_id: operation.clone(),
            target: target.clone(),
            generation: Some(generation),
            requested_by: target.clone().into(),
            approver: target.clone().into(),
            prompt: MessageContent::Router {
                text: MessageText::try_from("held foreground".to_owned()).expect("prompt"),
            },
        })
        .await
        .expect("foreground prompt submitted");
    let stop = supervisor.wait(collaboration_protocol::ConversationOperationWaitRequest {
        operation_id: operation,
        timeout_seconds: collaboration_protocol::PositiveSeconds::try_from(30)
            .expect("wait budget"),
    });
    let (mut active, _) = tokio::select! {
        connected = listener.accept() => connected.expect("agent taking prompt"),
        stopped = stop => panic!("foreground ended before the fixture activation notice: {stopped:?}. Wire: {}", std::fs::read_to_string(wire).unwrap_or_default()),
    };
    let mut notice = [0; 6];
    active
        .read_exact(&mut notice)
        .await
        .expect("active prompt witness");
    assert_eq!(&notice, b"active");
    (route, supervisor, target)
}

fn no_steering_prompt_fixture(event_socket: &Path) -> ExternalProviderLaunch {
    provider_fixture_launch::launch(
        "provider-steering-route-agent.ts",
        vec![
            "serial".to_owned(),
            event_socket.to_string_lossy().into_owned(),
        ],
    )
}

#[tokio::test]
async fn no_steering_auto_queues_without_second_concurrent_prompt() {
    // R10: a running Session without steering queues auto delivery; a second
    // session/prompt would cancel some agents' first Turn.
    let root = tempfile::tempdir().expect("fixture root");
    let event_socket = root.path().join("no-steer-events.sock");
    let listener = tokio::net::UnixListener::bind(&event_socket).expect("fixture events");
    let (route, supervisor, target, _) =
        prepared_route(root.path(), no_steering_prompt_fixture(&event_socket)).await;
    let first_evidence = RecordedEvidence(tokio::sync::Mutex::new(Vec::new()));
    let second_evidence = RecordedEvidence(tokio::sync::Mutex::new(Vec::new()));
    let first = route
        .deliver(
            request(target.clone(), MessageDelivery::Auto),
            &first_evidence,
        )
        .await
        .expect("first delivery");
    let (mut first_event, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("first event deadline")
        .expect("first event");
    let mut first_bytes = [0_u8; 5];
    first_event
        .read_exact(&mut first_bytes)
        .await
        .expect("first event bytes");
    let second = route
        .deliver(request(target, MessageDelivery::Auto), &second_evidence)
        .await
        .expect("second delivery");
    assert!(matches!(
        first.receipt().outcome.clone(),
        DeliveryOutcome::Started
    ));
    assert!(
        matches!(second.receipt().outcome.clone(), DeliveryOutcome::Queued),
        "{second:?}"
    );
    first_event
        .write_all(b"x")
        .await
        .expect("release first turn");
    let (mut second_event, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("queued prompt deadline")
        .expect("queued prompt event");
    let mut second_bytes = [0_u8; 6];
    second_event
        .read_exact(&mut second_bytes)
        .await
        .expect("second event bytes");
    assert_eq!(&first_bytes, b"first");
    assert_eq!(&second_bytes, b"second");
    route.shutdown_queue().await;
    supervisor.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn unadvertised_steering_is_rejected_on_any_provider_endpoint() {
    // ACP v1 initialization.mdx:100-115 treats omitted capabilities as
    // unsupported. R10 rejects steer by advertisement, not endpoint ID.
    let root = tempfile::tempdir().expect("fixture root");
    let socket = root.path().join("negotiation-events.sock");
    let listener = tokio::net::UnixListener::bind(&socket).expect("fixture events");
    let (route, supervisor, target, _) =
        prepared_route(root.path(), no_steering_prompt_fixture(&socket)).await;
    route
        .deliver(
            request(target.clone(), MessageDelivery::Auto),
            &RecordedEvidence(tokio::sync::Mutex::new(Vec::new())),
        )
        .await
        .expect("foreground prompt");
    let (mut first, _) = listener.accept().await.expect("foreground received");
    let mut notice = [0; 5];
    first
        .read_exact(&mut notice)
        .await
        .expect("foreground held");
    assert_eq!(&notice, b"first");
    let evidence = RecordedEvidence(tokio::sync::Mutex::new(Vec::new()));
    let receipt = route
        .deliver(request(target, MessageDelivery::Steer), &evidence)
        .await
        .expect("steer refusal");
    assert!(
        matches!(
            receipt.receipt().outcome.clone(),
            DeliveryOutcome::Rejected(ref rejection)
                if rejection.reason == collaboration_protocol::DeliveryRejectionReason::SteerUnsupported
        ),
        "{receipt:?}"
    );
    assert!(evidence.0.lock().await.is_empty());
    first.write_all(b"x").await.expect("release foreground");
    route.shutdown_queue().await;
    supervisor.shutdown().await.expect("shutdown");
}

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
fn request(target: SessionRef, mode: MessageDelivery) -> DeliveryRequest {
    let push_id =
        PushId::try_from(AttemptId::generate().as_str().to_owned()).expect("UUIDv7 push id");
    let correlation =
        DeliveryCorrelationId::try_from(push_id.as_str().to_owned()).expect("push id correlation");
    let line = MessageText::try_from(format!(
        "✉️ sender · \"follow-up\" · router://{}/push/{}",
        String::from(target.endpoint.service_id.clone()),
        push_id.as_str()
    ))
    .expect("prepared push line");
    DeliveryRequest {
        payload: PreparedPush {
            push_id,
            line,
            load_policy: collaboration_contracts::LoadPolicy::MayLoad,
        },
        target,
        mode,
        precondition: DeliveryPrecondition::Unpinned,
        correlation,
        attempt: AttemptId::generate(),
    }
}

#[tokio::test]
async fn running_claude_auto_and_steer_name_the_running_operation() {
    let root = tempfile::tempdir().expect("fixture root");
    let event_socket = root.path().join("prompt-event.sock");
    let listener = tokio::net::UnixListener::bind(&event_socket).expect("event listener");
    let (route, supervisor, target, generation) =
        prepared_route(root.path(), steering_fixture(&event_socket)).await;
    let running_operation = OperationId::generate();
    supervisor
        .prompt(ConversationPromptRequest {
            delivery: collaboration_protocol::MessageDelivery::Auto,
            input_id: None,
            operation_id: running_operation.clone(),
            target: target.clone(),
            generation: Some(generation),
            requested_by: (target.clone()).into(),
            approver: (target.clone()).into(),
            prompt: MessageContent::Router {
                text: MessageText::try_from("hold".to_owned()).expect("prompt"),
            },
        })
        .await
        .expect("running prompt submitted");
    let (mut active_event, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("active event deadline")
        .expect("active event");
    let mut marker = [0_u8; 6];
    active_event
        .read_exact(&mut marker)
        .await
        .expect("active marker");
    for mode in [MessageDelivery::Auto, MessageDelivery::Steer] {
        let evidence = RecordedEvidence(tokio::sync::Mutex::new(Vec::new()));
        let receipt = route
            .deliver(request(target.clone(), mode), &evidence)
            .await
            .expect("steer delivery");

        assert!(matches!(
            receipt.receipt().outcome.clone(),
            DeliveryOutcome::Steered
        ));
        assert!(matches!(receipt.receipt().client,
            Some(DeliveryClientReceipt::ProviderAcp { ref operation_id }) if operation_id.as_str() == running_operation.as_str()));
        assert!(matches!(evidence.0.lock().await.as_slice(),
            [RouteEffectEvidence::ProviderAcp(before), RouteEffectEvidence::ProviderAcp(after)]
            if before.submission == SubmissionEffect::Dispatching && after.submission == SubmissionEffect::Accepted));
    }
    active_event.write_all(b"x").await.expect("release prompt");
    route.shutdown_queue().await;
    supervisor.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn idle_claude_steer_starts_a_normal_prompt() {
    let root = tempfile::tempdir().expect("fixture root");
    let (route, supervisor, target, _) = prepared_route(
        root.path(),
        idle_steering_fixture(IdleSteeringScenario::SteerOnly),
    )
    .await;
    let evidence = RecordedEvidence(tokio::sync::Mutex::new(Vec::new()));

    let sent = request(target, MessageDelivery::Steer);
    let receipt = route
        .deliver(sent.clone(), &evidence)
        .await
        .expect("idle steer");

    assert!(matches!(
        receipt.receipt().outcome,
        DeliveryOutcome::Started
    ));
    assert!(matches!(
        receipt.receipt().client,
        Some(DeliveryClientReceipt::ProviderAcp { .. })
    ));
    assert!(matches!(evidence.0.lock().await.as_slice(),
        [RouteEffectEvidence::ProviderAcp(before), RouteEffectEvidence::ProviderAcp(after)]
        if before.submission == SubmissionEffect::Dispatching && after.submission == SubmissionEffect::Accepted));
    route.shutdown_queue().await;
    supervisor.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn idle_claude_auto_starts_one_normal_prompt() {
    let root = tempfile::tempdir().expect("fixture root");
    let (route, supervisor, target, _) = prepared_route(
        root.path(),
        idle_steering_fixture(IdleSteeringScenario::AutoStartsPrompt),
    )
    .await;
    let evidence = RecordedEvidence(tokio::sync::Mutex::new(Vec::new()));

    let receipt = route
        .deliver(request(target, MessageDelivery::Auto), &evidence)
        .await
        .expect("idle auto");

    assert!(matches!(
        receipt.receipt().outcome.clone(),
        DeliveryOutcome::Started
    ));
    assert!(matches!(
        receipt.receipt().client,
        Some(DeliveryClientReceipt::ProviderAcp { .. })
    ));
    assert!(matches!(evidence.0.lock().await.as_slice(),
        [RouteEffectEvidence::ProviderAcp(before), RouteEffectEvidence::ProviderAcp(after)]
        if before.submission == SubmissionEffect::Dispatching && after.submission == SubmissionEffect::Accepted));
    route.shutdown_queue().await;
    supervisor.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn started_new_turn_steer_outcome_is_reported_as_started() {
    // R10: an agent's startedNewTurn is a new Turn even when Router asked
    // for promptRequired idle behavior.
    let root = tempfile::tempdir().expect("fixture root");
    let (route, supervisor, target) = active_steering_route(root.path(), "started").await;
    let evidence = RecordedEvidence(tokio::sync::Mutex::new(Vec::new()));
    let receipt = route
        .deliver(request(target, MessageDelivery::Auto), &evidence)
        .await
        .expect("steer outcome");
    assert!(
        matches!(receipt.receipt().outcome.clone(), DeliveryOutcome::Started),
        "{receipt:?}"
    );
    assert!(
        receipt.receipt().client.is_none(),
        "agent-owned turn has no Router operation ID"
    );
    route.shutdown_queue().await;
    supervisor.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn failed_steer_outcome_is_not_submitted_without_agent_text() {
    // R10: failed is not evidence that an Input was steered or prompted.
    let root = tempfile::tempdir().expect("fixture root");
    let (route, supervisor, target) = active_steering_route(root.path(), "failed").await;
    let evidence = RecordedEvidence(tokio::sync::Mutex::new(Vec::new()));
    let receipt = route
        .deliver(request(target, MessageDelivery::Auto), &evidence)
        .await
        .expect("steer outcome");
    assert!(
        matches!(
            receipt.receipt().outcome.clone(),
            DeliveryOutcome::NotSubmitted { ref reason, .. } if reason == "steerFailed"
        ),
        "{receipt:?}"
    );
    assert!(!format!("{receipt:?}").contains("private provider detail"));
    route.shutdown_queue().await;
    supervisor.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn dropped_steering_reply_remains_unknown() {
    let root = tempfile::tempdir().expect("fixture root");
    let (route, supervisor, target) = active_steering_route(root.path(), "drop").await;
    let evidence = RecordedEvidence(tokio::sync::Mutex::new(Vec::new()));

    let sent = request(target, MessageDelivery::Steer);
    let receipt = route
        .deliver(sent.clone(), &evidence)
        .await
        .expect("dropped steering reply");

    assert!(matches!(
        receipt.receipt().outcome.clone(),
        DeliveryOutcome::Unknown
    ));
    assert!(matches!(evidence.0.lock().await.as_slice(),
        [RouteEffectEvidence::ProviderAcp(before), RouteEffectEvidence::ProviderAcp(after)]
        if before.submission == SubmissionEffect::Dispatching && after.submission == SubmissionEffect::Unknown));
    let recorded = evidence
        .0
        .lock()
        .await
        .last()
        .expect("unknown evidence")
        .clone();
    let reconciliation = route
        .reconcile_attempt(AttemptReconciliationContext {
            target: sent.target,
            prepared_push_id: PushId::try_from("018f47d2-24d5-7a68-b9ec-6f759c3945a1".to_owned())
                .expect("UUIDv7 push id"),
            mode: sent.mode,
            recorded,
        })
        .await
        .expect("reconciliation");
    assert!(matches!(
        reconciliation,
        AttemptReconciliation::StillUnknown
    ));
    route.shutdown_queue().await;
    let _shutdown = supervisor.shutdown().await;
}
