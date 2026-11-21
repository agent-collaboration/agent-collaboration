//! An already-loaded provider can receive messages after a mode difference.

use super::{NoLivePeer, RecordedEvidence, available_directory, provider_binding, request, target};
use crate::provider_fixture_launch;
use codex_router_host::{
    ExternalProviderBinding, ExternalProviderRuntime, ExternalProviderSupervisor,
    ProviderAcpDeliveryRoute,
};
use collaboration_contracts::{ProviderConversationBackend, SessionDeliveryRoute};
use collaboration_protocol::{
    DeliveryOutcome, OperationId, ProviderRequestedPolicy, ProviderWorkingDirectory, RouterAccess,
};
use collaboration_runtime::{ProviderOperationStore, ProviderSessionRecord};
use serde_json::Value;
use std::sync::Arc;

#[tokio::test]
async fn an_already_loaded_session_accepts_delivery_despite_a_mode_difference() {
    let root = tempfile::tempdir().expect("provider root");
    let wire_receipt = root.path().join("agent.json");
    let runtime = ExternalProviderRuntime::initialize(
        provider_fixture_launch::launch(
            "permission-mode-agent.ts",
            vec![
                wire_receipt.display().to_string(),
                "claude-without-auto-or-accept-edits".into(),
                "no-directories".into(),
                "advertise-close".into(),
                "advertise-restore".into(),
                "restored-in-auto".into(),
                "plain-prompts".into(),
            ],
        ),
        Arc::new(acp_provider_adapters::ClaudeAcpBehavior::new(
            acp_provider_adapters::ClaudeSkillSource::Annotated,
        )),
    )
    .await
    .expect("fixture provider");
    let mut target = target();
    target.endpoint.endpoint_id =
        collaboration_protocol::EndpointId::try_from("claude-local".to_owned())
            .expect("Claude endpoint");
    let mut binding = provider_binding(&target);
    binding.runtime.provider = collaboration_protocol::ProviderKind::ClaudeCode;
    let store = Arc::new(tokio::sync::Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("store"),
    ));
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
    let provider_id = supervisor
        .open_provider_fixture(&target.endpoint, root.path().to_owned())
        .await
        .expect("fixture session");
    target.session_id =
        collaboration_protocol::SessionId::try_from(provider_id).expect("fixture ID");
    let working_directory =
        ProviderWorkingDirectory::try_from(root.path().display().to_string()).expect("cwd");
    let policy = ProviderRequestedPolicy {
        access: RouterAccess::WriteRestricted,
    };
    store
        .lock()
        .await
        .record_session(&ProviderSessionRecord {
            target: target.clone(),
            working_directory: working_directory.clone(),
            requested_policy: policy.clone(),
            created_by: target.clone().into(),
            approver: target.clone().into(),
            updated_at_ms: 1,
            resume: collaboration_runtime::ProviderSessionResumeInputs {
                requested_mode: Some("acceptEdits".into()),
                ..Default::default()
            },
        })
        .await
        .expect("recorded mode");
    let close = OperationId::generate();
    supervisor
        .close(collaboration_protocol::ConversationCloseRequest {
            operation_id: close.clone(),
            target: target.clone(),
            generation: Some(binding.generation.clone()),
            requested_by: target.clone().into(),
            approver: target.clone().into(),
        })
        .await
        .expect("close admitted");
    supervisor
        .wait(collaboration_protocol::ConversationOperationWaitRequest {
            operation_id: close,
            timeout_seconds: collaboration_protocol::PositiveSeconds::try_from(2)
                .expect("wait budget"),
        })
        .await
        .expect("closed before restore");
    let operation_id = OperationId::generate();
    supervisor
        .load(collaboration_protocol::ConversationLoadRequest {
            operation_id: operation_id.clone(),
            target: target.clone(),
            generation: Some(binding.generation.clone()),
            working_directory,
            requested_by: target.clone().into(),
            approver: target.clone().into(),
            requested_policy: policy,
        })
        .await
        .expect("restore submitted");
    let loaded = supervisor
        .wait(collaboration_protocol::ConversationOperationWaitRequest {
            operation_id,
            timeout_seconds: collaboration_protocol::PositiveSeconds::try_from(2)
                .expect("wait budget"),
        })
        .await
        .expect("restore settled");
    assert!(matches!(
        loaded.output,
        collaboration_protocol::ConversationOperationWaitOutput::Available {
            settlement: collaboration_protocol::ConversationOperationSettlement::Loaded { .. },
        }
    ));
    let route = ProviderAcpDeliveryRoute::new(
        target.endpoint.service_id.clone(),
        std::iter::once(target.endpoint.clone()).collect(),
        available_directory(&target, &binding),
        Arc::clone(&supervisor),
        Arc::clone(&store),
        Arc::new(NoLivePeer),
        Arc::new(router_clock::SystemRuntimeClock::new()),
    );

    let evidence = RecordedEvidence(tokio::sync::Mutex::new(Vec::new()));
    let request = request(target.clone(), "next message");
    let operation_id =
        OperationId::try_from(request.attempt.as_str().to_owned()).expect("delivery operation");
    let delivery = route
        .deliver(request, &evidence)
        .await
        .expect("delivery result")
        .into_receipt();
    let wait_for_prompt = |operation_id| {
        supervisor.wait(collaboration_protocol::ConversationOperationWaitRequest {
            operation_id,
            timeout_seconds: collaboration_protocol::PositiveSeconds::try_from(2)
                .expect("wait budget"),
        })
    };
    assert!(
        matches!(
            delivery.outcome,
            DeliveryOutcome::Started | DeliveryOutcome::Queued
        ),
        "{delivery:?}"
    );
    let delivered = wait_for_prompt(operation_id)
        .await
        .expect("delivery settles");
    let content = route
        .prompt_contents(
            target.clone(),
            target.clone().into(),
            session_event_model::InputId::generate(),
            session_event_model::ProviderSubmission::plain(vec![
                session_event_model::PromptContent::text("next content command".into())
                    .expect("text"),
            ]),
        )
        .await
        .expect("content admitted");
    let prompted = wait_for_prompt(content.operation.operation_id)
        .await
        .expect("content settles");
    for completed in [delivered, prompted] {
        assert!(
            matches!(
                completed.output,
                collaboration_protocol::ConversationOperationWaitOutput::Available {
                    settlement:
                        collaboration_protocol::ConversationOperationSettlement::PromptCompleted { .. },
                }
            ),
            "{completed:?}"
        );
    }
    let record = store
        .lock()
        .await
        .session_record(&target)
        .await
        .expect("read record")
        .expect("saved record");
    assert_eq!(record.resume.requested_mode.as_deref(), Some("acceptEdits"));
    route.shutdown_queue().await;
    supervisor.shutdown().await.expect("supervisor shutdown");
    let agent: Value = serde_json::from_slice(&std::fs::read(wire_receipt).expect("agent receipt"))
        .expect("agent receipt JSON");
    assert_eq!(
        agent["prompts"],
        serde_json::json!([target.session_id, target.session_id])
    );
}
