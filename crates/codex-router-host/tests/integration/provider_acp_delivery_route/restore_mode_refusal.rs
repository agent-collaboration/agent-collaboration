//! A delivery to an already-loaded session carries its restore-mode refusal.

use super::{NoLivePeer, RecordedEvidence, available_directory, provider_binding, request, target};
use crate::provider_fixture_launch;
use codex_router_host::{
    ExternalProviderBinding, ExternalProviderRuntime, ExternalProviderSupervisor,
    ProviderAcpDeliveryRoute, ProviderPromptContentsError, ProviderQueueAdmissionError,
};
use collaboration_contracts::{ProviderConversationBackend, SessionDeliveryRoute};
use collaboration_protocol::{
    DeliveryOutcome, DeliveryRejectionReason, OperationId, ProviderRequestedPolicy,
    ProviderWorkingDirectory, RouterAccess,
};
use collaboration_runtime::{ProviderOperationStore, ProviderSessionRecord};
use serde_json::Value;
use std::sync::Arc;

#[tokio::test]
async fn an_already_loaded_restore_mismatch_names_both_modes_at_delivery() {
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
            ],
        ),
        Arc::new(acp_provider_adapters::ClaudeAcpBehavior::new(
            acp_provider_adapters::ClaudeSkillSource::Annotated,
        )),
    )
    .await
    .expect("fixture provider");
    let provider_id = runtime
        .create_session(root.path().to_owned())
        .await
        .expect("fixture session");
    runtime
        .close_session(provider_id.clone())
        .await
        .expect("closed before restore");
    let mut target = target();
    target.endpoint.endpoint_id =
        collaboration_protocol::EndpointId::try_from("claude-local".to_owned())
            .expect("Claude endpoint");
    target.session_id =
        collaboration_protocol::SessionId::try_from(provider_id).expect("fixture ID");
    let mut binding = provider_binding(&target);
    binding.runtime.provider = collaboration_protocol::ProviderKind::ClaudeCode;
    let store = Arc::new(tokio::sync::Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("store"),
    ));
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
        store,
        Arc::new(NoLivePeer),
        Arc::new(router_clock::SystemRuntimeClock::new()),
    );

    let evidence = RecordedEvidence(tokio::sync::Mutex::new(Vec::new()));
    let delivery = route
        .deliver(request(target.clone(), "held message"), &evidence)
        .await
        .expect("delivery refused")
        .into_receipt();
    let content = route
        .prompt_contents(
            target.clone(),
            target.clone().into(),
            session_event_model::InputId::generate(),
            session_event_model::ProviderSubmission::plain(vec![
                session_event_model::PromptContent::text("held prompt".into()).expect("text"),
            ]),
        )
        .await;
    route.shutdown_queue().await;
    supervisor.shutdown().await.expect("supervisor shutdown");

    let DeliveryOutcome::Rejected(rejection) = delivery.outcome else {
        panic!("mismatch did not refuse delivery: {delivery:?}");
    };
    assert_eq!(
        rejection.reason,
        DeliveryRejectionReason::SettingsUnresolved
    );
    let detail = rejection.detail.expect("cell's refusal answer");
    assert!(
        detail.contains("`acceptEdits`")
            && detail.contains("`auto`")
            && detail.contains("set a mode this agent offers")
            && detail.contains("close the session"),
        "{detail}"
    );
    assert!(!detail.contains("accept current values"), "{detail}");
    assert!(
        matches!(content, Err(ProviderPromptContentsError::Admission(ProviderQueueAdmissionError::SettingsUnresolved { ref answer })) if answer == &detail),
        "{content:?}"
    );
    assert!(evidence.0.lock().await.is_empty());
    let agent: Value = serde_json::from_slice(&std::fs::read(wire_receipt).expect("agent receipt"))
        .expect("agent receipt JSON");
    assert_eq!(agent["prompts"], serde_json::json!([]));
}
