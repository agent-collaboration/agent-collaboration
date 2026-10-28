//! Provider route decisions before any client effect.

#[path = "support/provider_fixture_launch.rs"]
mod provider_fixture_launch;

use agent_automation::{
    ProviderAcpEffectEvidence, ProviderBindingReference, ProviderSettlementEffect,
    RouteEffectEvidence, SubmissionEffect,
};
use codex_router_host::{
    ExternalProviderBinding, ExternalProviderLaunch, ExternalProviderRuntime,
    ExternalProviderSupervisor, ProviderAcpDeliveryRoute,
};
use collaboration_contracts::{
    AttemptReconciliation, AttemptReconciliationContext, DeliveryPrecondition, LoadPolicy,
    NOT_LOADED_REASON, RoutePresence, SessionDeliveryRoute, SessionMessageDelivery,
    layer_zero::{DeliveryRequest as PreparedDeliveryRequest, PreparedPush},
};
use collaboration_protocol::{
    AttemptId, CodexGeneration, DeliveryClientReceipt, DeliveryCorrelationId, DeliveryOutcome,
    DeliveryRejectionReason, GenerationNumber, MessageDelivery, MessageText, OperationId,
    ProviderOperationEffect, ProviderOperationKind, ProviderReconciliationState,
    ProviderRequestedPolicy, ProviderWorkingDirectory, PushId, RouterAccess, SessionRef,
};
use collaboration_runtime::{
    EndpointDirectory, ProviderOperationAdmission, ProviderOperationStore, ProviderSessionRecord,
    SessionDeliveryRouter,
};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

#[path = "support/provider_delivery_fixture.rs"]
mod provider_delivery_fixture;
use provider_delivery_fixture::{
    LivePeer, NoLivePeer, RecordedEvidence, StaleRunningClaimRoute,
    assert_uuid_v7_provider_reference, available_directory, cursor_prompt_fixture,
    exited_provider_fixture, no_load_fixture, provider_binding, refusing_load_fixture, request,
    target,
};

#[tokio::test]
async fn cursor_steer_rejects_before_evidence_or_client_io() {
    let root = tempfile::tempdir().expect("provider root");
    let target = target();
    let binding = provider_binding(&target);
    let runtime = ExternalProviderRuntime::initialize(no_load_fixture())
        .await
        .expect("fixture provider");
    runtime
        .create_session(PathBuf::from("/tmp"))
        .await
        .expect("session/new");
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
    );

    let evidence = RecordedEvidence(tokio::sync::Mutex::new(Vec::new()));

    let mut request = request(target, "hello");
    request.mode = MessageDelivery::Steer;
    let receipt = route
        .deliver(request, &evidence)
        .await
        .expect("delivery refusal")
        .into_receipt();

    assert!(
        matches!(receipt.outcome, DeliveryOutcome::Rejected(rejection)
        if rejection.reason == DeliveryRejectionReason::SteerUnsupported)
    );
    assert!(evidence.0.lock().await.is_empty());
    route.shutdown_queue().await;
    supervisor.shutdown().await.expect("supervisor shutdown");
}

#[tokio::test]
async fn cursor_auto_queues_a_prepared_push_with_exact_line_and_push_id() {
    let root = tempfile::tempdir().expect("provider root");
    let event_socket = root.path().join("prompt-events.sock");
    let captured_prepared_prompt = root.path().join("prepared-prompt.json");
    let listener = tokio::net::UnixListener::bind(&event_socket).expect("fixture events");
    let target = target();
    let binding = provider_binding(&target);
    let runtime = ExternalProviderRuntime::initialize(cursor_prompt_fixture(
        &event_socket,
        &captured_prepared_prompt,
    ))
    .await
    .expect("fixture provider");
    runtime
        .create_session(PathBuf::from("/tmp"))
        .await
        .expect("session/new");
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
    let directory = available_directory(&target, &binding);
    let route = Arc::new(ProviderAcpDeliveryRoute::new(
        target.endpoint.service_id.clone(),
        std::iter::once(target.endpoint.clone()).collect(),
        directory,
        Arc::clone(&supervisor),
        store,
        Arc::new(NoLivePeer),
    ));
    let router = SessionDeliveryRouter::new(vec![route.clone()]);
    let first_evidence = RecordedEvidence(tokio::sync::Mutex::new(Vec::new()));
    let second_evidence = RecordedEvidence(tokio::sync::Mutex::new(Vec::new()));
    let stale_evidence = RecordedEvidence(tokio::sync::Mutex::new(Vec::new()));
    let mut stale_request = request(target.clone(), "stale");
    stale_request.precondition = DeliveryPrecondition::EndpointGeneration {
        expected: CodexGeneration {
            service_epoch: binding.generation.service_epoch.clone(),
            generation: GenerationNumber::try_from(2).expect("stale generation"),
        },
    };

    let stale = router
        .deliver(stale_request, &stale_evidence)
        .await
        .expect("stale guard")
        .into_receipt();
    assert!(matches!(stale.outcome, DeliveryOutcome::Rejected(rejection)
        if rejection.reason == DeliveryRejectionReason::StaleGeneration));
    assert!(stale_evidence.0.lock().await.is_empty());

    let first = router
        .deliver(request(target.clone(), "first"), &first_evidence)
        .await
        .expect("first delivery")
        .into_receipt();
    let (mut first_event, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("first event deadline")
        .expect("first event");
    let mut first_bytes = [0_u8; 5];
    first_event
        .read_exact(&mut first_bytes)
        .await
        .expect("first bytes");
    let push_id = PushId::try_from("018f47d2-24d5-7a68-b9ec-6f759c394599".to_owned())
        .expect("UUIDv7 push id");
    let line = MessageText::try_from(format!(
        "✉️ Main · \"prepared while busy\" · router://00000000-0000-4000-8000-000000000001/push/{}",
        push_id.as_str()
    ))
    .expect("push line");
    let correlation =
        DeliveryCorrelationId::try_from(push_id.as_str().to_owned()).expect("push correlation");
    assert_eq!(correlation.as_str(), push_id.as_str());
    let attempt = AttemptId::generate();
    let second = router
        .deliver(
            PreparedDeliveryRequest {
                payload: PreparedPush {
                    push_id: push_id.clone(),
                    line: line.clone(),
                    load_policy: LoadPolicy::LoadedOnly,
                },
                target: target.clone(),
                mode: MessageDelivery::Auto,
                precondition: DeliveryPrecondition::Unpinned,
                correlation,
                attempt: attempt.clone(),
            },
            &second_evidence,
        )
        .await
        .expect("second delivery")
        .into_receipt();

    assert!(matches!(first.outcome, DeliveryOutcome::Started));
    assert!(matches!(second.outcome, DeliveryOutcome::Queued));
    assert_eq!(
        second.reachability,
        Some(collaboration_protocol::SessionReachability::ProviderAcp)
    );
    assert!(matches!(
        second.client,
        Some(DeliveryClientReceipt::ProviderAcp { operation_id })
            if operation_id == OperationId::try_from(attempt.as_str().to_owned()).expect("attempt operation id")
    ));
    assert!(
        matches!(second_evidence.0.lock().await.as_slice(), [RouteEffectEvidence::ProviderAcp(effect)]
        if effect.submission == SubmissionEffect::RouterQueued)
    );
    let queued = route.queue_list(&target);
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].input_id.as_str(), push_id.as_str());
    first_event
        .write_all(b"x")
        .await
        .expect("release first prompt");
    let (mut second_event, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("second event deadline")
        .expect("second event");
    let mut second_bytes = Vec::new();
    second_event
        .read_to_end(&mut second_bytes)
        .await
        .expect("second bytes");
    assert_eq!(first_bytes, *b"first");
    assert_eq!(second_bytes, b"second");
    let captured_prompt: Value = serde_json::from_slice(
        &std::fs::read(&captured_prepared_prompt).expect("captured prepared prompt"),
    )
    .expect("prepared prompt JSON");
    assert_eq!(
        captured_prompt["params"]["sessionId"].as_str(),
        Some("fixture-session")
    );
    let prompt_blocks = captured_prompt["params"]["prompt"]
        .as_array()
        .expect("ACP prompt content blocks");
    assert_eq!(prompt_blocks.len(), 1);
    assert_eq!(prompt_blocks[0]["text"].as_str(), Some(line.as_str()));
    route.shutdown_queue().await;
    supervisor.shutdown().await.expect("supervisor shutdown");
}

#[tokio::test]
async fn router_queued_reconciliation_uses_only_the_operation_store() {
    let root = tempfile::tempdir().expect("provider root");
    let target = target();
    let binding = provider_binding(&target);
    let store = Arc::new(tokio::sync::Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("store"),
    ));
    let supervisor = Arc::new(
        ExternalProviderSupervisor::new(Vec::new(), Arc::clone(&store)).expect("supervisor"),
    );
    let route = ProviderAcpDeliveryRoute::new(
        target.endpoint.service_id.clone(),
        std::iter::once(target.endpoint.clone()).collect(),
        EndpointDirectory::new(target.endpoint.service_id.clone()),
        supervisor,
        Arc::clone(&store),
        Arc::new(NoLivePeer),
    );
    let attempt = AttemptId::generate();
    let operation_id =
        agent_automation::OperationId::try_from(attempt.as_str().to_owned()).expect("operation ID");
    let evidence = RouteEffectEvidence::ProviderAcp(ProviderAcpEffectEvidence {
        target: target.clone(),
        generation: binding.generation.clone(),
        binding: ProviderBindingReference::try_from(String::from(binding.binding_id.clone()))
            .expect("binding reference"),
        attempt_id: attempt,
        submission: SubmissionEffect::RouterQueued,
        settlement: ProviderSettlementEffect::NotObserved,
    });
    let context = || AttemptReconciliationContext {
        target: target.clone(),
        prepared_push_id: PushId::try_from("018f47d2-24d5-7a68-b9ec-6f759c394599".to_owned())
            .expect("UUIDv7 push id"),
        mode: MessageDelivery::Queue,
        recorded: evidence.clone(),
    };

    let absent = route
        .reconcile_attempt(context())
        .await
        .expect("absent reconciliation");
    assert!(matches!(absent, AttemptReconciliation::KnownNotSubmitted));
    store
        .lock()
        .await
        .admit(ProviderOperationAdmission {
            operation_id: operation_id.clone(),
            operation_kind: ProviderOperationKind::ConversationPrompt,
            binding: collaboration_protocol::ConversationBindingIdentity::ExternalProvider {
                binding,
            },
            admitted_at_ms: 1,
        })
        .await
        .expect("operation admission");
    store
        .lock()
        .await
        .record_target(&operation_id, &target, 2)
        .await
        .expect("target");
    store
        .lock()
        .await
        .mark_may_have_dispatched(&operation_id, 3)
        .await
        .expect("dispatch marker");
    store
        .lock()
        .await
        .record_terminal(
            &operation_id,
            ProviderOperationEffect::Applied,
            ProviderReconciliationState::Confirmed,
            None,
            4,
        )
        .await
        .expect("operation settlement");

    let present = route
        .reconcile_attempt(context())
        .await
        .expect("settled reconciliation");
    assert!(matches!(present, AttemptReconciliation::Accepted(receipt)
        if matches!(receipt.outcome, DeliveryOutcome::Queued)));
}

#[tokio::test]
async fn provider_load_auth_rejection_is_typed_without_session_new() {
    let root = tempfile::tempdir().expect("provider root");
    let load_marker = root.path().join("load-method.txt");
    let target = target();
    let binding = provider_binding(&target);
    let runtime = ExternalProviderRuntime::initialize(refusing_load_fixture(&load_marker, -32000))
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
    let route = Arc::new(ProviderAcpDeliveryRoute::new(
        target.endpoint.service_id.clone(),
        std::iter::once(target.endpoint.clone()).collect(),
        available_directory(&target, &binding),
        Arc::clone(&supervisor),
        store,
        Arc::new(NoLivePeer),
    ));
    assert_eq!(
        route.presence(&target).await.expect("provider presence"),
        RoutePresence::Wakeable
    );

    let stale_router =
        SessionDeliveryRouter::new(vec![Arc::new(StaleRunningClaimRoute(Arc::clone(&route)))]);
    let loaded_only_push_id = PushId::try_from("018f47d2-24d5-7a68-b9ec-6f759c394588".to_owned())
        .expect("UUIDv7 loaded-only push id");
    let loaded_only_line = MessageText::try_from(format!(
        "🧵 Main · \"held batch\" · router://00000000-0000-4000-8000-000000000001/push/{}",
        loaded_only_push_id.as_str()
    ))
    .expect("loaded-only push line");
    let loaded_only_correlation =
        DeliveryCorrelationId::try_from(loaded_only_push_id.as_str().to_owned())
            .expect("loaded-only push correlation");
    assert_eq!(
        loaded_only_correlation.as_str(),
        loaded_only_push_id.as_str()
    );
    let loaded_only_request = PreparedDeliveryRequest {
        payload: PreparedPush {
            push_id: loaded_only_push_id.clone(),
            line: loaded_only_line,
            load_policy: LoadPolicy::LoadedOnly,
        },
        target: target.clone(),
        mode: MessageDelivery::Auto,
        precondition: DeliveryPrecondition::Unpinned,
        correlation: loaded_only_correlation,
        attempt: AttemptId::generate(),
    };
    let loaded_only_evidence = RecordedEvidence(tokio::sync::Mutex::new(Vec::new()));
    let loaded_only_receipt = stale_router
        .deliver(loaded_only_request, &loaded_only_evidence)
        .await
        .expect("loaded-only stale-claim refusal")
        .into_receipt();
    assert!(matches!(
        loaded_only_receipt.outcome,
        DeliveryOutcome::NotSubmitted {
            retryable: true,
            ref reason,
        } if reason == NOT_LOADED_REASON
    ));
    assert!(!load_marker.exists());
    assert!(matches!(
        loaded_only_evidence.0.lock().await.as_slice(),
        [RouteEffectEvidence::ProviderAcp(before), RouteEffectEvidence::ProviderAcp(after)]
            if before.submission == SubmissionEffect::Dispatching
                && after.submission == SubmissionEffect::NotDispatched
    ));

    let evidence = RecordedEvidence(tokio::sync::Mutex::new(Vec::new()));

    let receipt = route
        .deliver(request(target.clone(), "hello"), &evidence)
        .await
        .expect("load refusal")
        .into_receipt();

    let outcome = serde_json::to_value(&receipt.outcome).expect("receipt encoding");
    assert_eq!(outcome["kind"], "rejected");
    assert_eq!(outcome["reason"], "providerRejected");
    assert_eq!(outcome["clientCode"], -32000);
    assert!(!outcome.to_string().contains("must not escape"));
    assert_eq!(
        std::fs::read_to_string(load_marker).expect("method").trim(),
        "session/load"
    );
    assert!(matches!(evidence.0.lock().await.as_slice(),
        [RouteEffectEvidence::ProviderAcp(before), RouteEffectEvidence::ProviderAcp(after)]
        if before.submission == SubmissionEffect::Dispatching && after.submission == SubmissionEffect::NotDispatched));
    route.shutdown_queue().await;
    supervisor.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn unadvertised_load_settles_not_submitted_without_sending_load() {
    // ACP v1 initialization.mdx:245 requires an advertisement for session/load.
    // R9 settles a cold delivery as unsupported: load without sending it.
    let root = tempfile::tempdir().expect("provider root");
    let target = target();
    let binding = provider_binding(&target);
    let runtime = ExternalProviderRuntime::initialize(no_load_fixture())
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
    );
    assert!(matches!(
        route.presence(&target).await.expect("provider presence"),
        RoutePresence::Unreachable { .. }
    ));
    let evidence = RecordedEvidence(tokio::sync::Mutex::new(Vec::new()));
    let receipt = tokio::time::timeout(
        Duration::from_secs(2),
        route.deliver(request(target, "hello"), &evidence),
    )
    .await
    .expect("delivery cannot wait on unsupported load")
    .expect("delivery receipt");
    assert_eq!(
        receipt.unsent_cause(),
        Some(collaboration_contracts::UnsentCause::Refused(
            collaboration_contracts::NativeRefusalCode::Rejection(
                DeliveryRejectionReason::UnsupportedCapability
            )
        ))
    );
    let receipt = receipt.into_receipt();
    assert!(
        matches!(
            receipt.outcome,
            DeliveryOutcome::NotSubmitted { retryable: false, ref reason }
                if reason == "unsupported: load"
        ),
        "{receipt:?}"
    );
    route.shutdown_queue().await;
    supervisor.shutdown().await.expect("supervisor shutdown");
}

#[tokio::test]
async fn coded_provider_load_errors_keep_correlations_and_do_not_expose_acp_text() {
    for (error_code, expected_kind, expected_reason, expected_explanation, code_label) in [
        (
            -32002,
            "rejected",
            Some("providerSessionNotFound"),
            "this session never started a turn and did not survive the provider restart; create a new conversation",
            "provider code",
        ),
        (
            -32600,
            "rejected",
            Some("providerRejected"),
            "provider rejected the ACP operation",
            "provider code",
        ),
        (
            -32000,
            "rejected",
            Some("providerRejected"),
            "provider rejected the ACP operation",
            "provider code",
        ),
        (
            -32601,
            "notSubmitted",
            None,
            "provider ACP method is unsupported",
            "ACP code",
        ),
        (
            -32602,
            "notSubmitted",
            None,
            "provider ACP parameters are invalid",
            "ACP code",
        ),
        (
            -32800,
            "notSubmitted",
            None,
            "provider ACP request was cancelled",
            "ACP code",
        ),
    ] {
        let root = tempfile::tempdir().expect("provider root");
        let load_marker = root.path().join("load-method.txt");
        let target = target();
        let binding = provider_binding(&target);
        let runtime =
            ExternalProviderRuntime::initialize(refusing_load_fixture(&load_marker, error_code))
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
                working_directory: ProviderWorkingDirectory::try_from("/tmp".to_owned())
                    .expect("cwd"),
                requested_policy: ProviderRequestedPolicy {
                    access: RouterAccess::WriteRestricted,
                },
                created_by: (target.clone()).into(),
                approver: (target.clone()).into(),
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
        let route = ProviderAcpDeliveryRoute::new(
            target.endpoint.service_id.clone(),
            std::iter::once(target.endpoint.clone()).collect(),
            available_directory(&target, &binding),
            Arc::clone(&supervisor),
            store,
            Arc::new(NoLivePeer),
        );
        let evidence = RecordedEvidence(tokio::sync::Mutex::new(Vec::new()));

        let receipt = route
            .deliver(request(target, "empty restart session"), &evidence)
            .await
            .expect("load refusal")
            .into_receipt();
        let outcome = serde_json::to_value(&receipt.outcome).expect("receipt encoding");

        assert_eq!(outcome["kind"], expected_kind);
        if let Some(expected_reason) = expected_reason {
            assert_eq!(outcome["reason"], expected_reason);
            assert_eq!(outcome["clientCode"], error_code);
            let detail = outcome["detail"].as_str().expect("rejection detail");
            assert!(detail.starts_with(expected_explanation), "{detail}");
            assert!(
                !detail.contains("provider fixture refusal") && !detail.contains("must not escape"),
                "ACP error data escaped: {outcome}"
            );
            assert_uuid_v7_provider_reference(detail, code_label, error_code);
        } else {
            assert_eq!(outcome["retryable"], true);
            let reason = outcome["reason"].as_str().expect("not-submitted reason");
            assert!(reason.starts_with(expected_explanation), "{reason}");
            assert!(
                !reason.contains("provider fixture refusal") && !reason.contains("must not escape"),
                "ACP error data escaped: {outcome}"
            );
            assert_uuid_v7_provider_reference(reason, code_label, error_code);
        }
        assert_eq!(
            std::fs::read_to_string(&load_marker)
                .expect("load marker")
                .trim(),
            "session/load"
        );
        assert!(matches!(evidence.0.lock().await.as_slice(),
            [RouteEffectEvidence::ProviderAcp(before), RouteEffectEvidence::ProviderAcp(after)]
            if before.submission == SubmissionEffect::Dispatching && after.submission == SubmissionEffect::NotDispatched));
        route.shutdown_queue().await;
        supervisor.shutdown().await.expect("shutdown");
    }
}

#[path = "provider_acp_delivery_route/provider_failure_tests.rs"]
mod provider_failure_tests;

#[path = "provider_acp_delivery_route/auto_direct_message_tests.rs"]
mod auto_direct_message_tests;
