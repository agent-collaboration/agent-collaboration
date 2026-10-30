//! Host content admission preserves actor authority and ACP block order.

#[path = "support/provider_fixture_launch.rs"]
mod provider_fixture_launch;
use codex_router_host::{
    ExternalProviderBinding, ExternalProviderLaunch, ExternalProviderRuntime,
    ExternalProviderSupervisor, ProviderAcpDeliveryRoute, ProviderPromptContentsError,
    ProviderQueueAdmissionError,
};
use collaboration_protocol::{
    ProviderRequestedPolicy, ProviderWorkingDirectory, RouterAccess, SessionId,
};
use collaboration_runtime::ProviderOperationStore;
use collaboration_runtime::ProviderSessionRecord;
use session_event_model::{InputId, PromptContent};
use std::{path::PathBuf, sync::Arc};

#[path = "support/provider_acp_message_fifo_support.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
mod support;
use support::{NoLivePeer, available_directory, provider_binding, target};

fn two_block_fixture() -> ExternalProviderLaunch {
    let script = node_test_fixture::fixture_program("provider-content-command-agent.ts");
    ExternalProviderLaunch {
        persistence_target: acp_client_runtime::ProviderPersistenceTarget::Unspecified,
        executable: PathBuf::from("node"),
        arguments: vec![script.to_string_lossy().into_owned(), "content".to_owned()],
        environment: Vec::new(),
    }
}

#[tokio::test]
async fn non_creator_content_prompt_is_accepted_after_capability_validation() {
    let root = tempfile::tempdir().expect("provider root");
    let target = target();
    let binding = provider_binding(&target);
    let runtime = ExternalProviderRuntime::initialize(
        two_block_fixture(),
        std::sync::Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
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
    let route = ProviderAcpDeliveryRoute::new(
        target.endpoint.service_id.clone(),
        std::iter::once(target.endpoint.clone()).collect(),
        available_directory(&target, &binding),
        Arc::clone(&supervisor),
        store,
        Arc::new(NoLivePeer),
    );
    let contents = || {
        vec![
            PromptContent::text("Read".into()).expect("text"),
            PromptContent::resource_link(
                "https://example.test/context".into(),
                "context".into(),
                None,
            )
            .expect("resource link"),
        ]
    };
    let mut other_actor = target.clone();
    other_actor.session_id = SessionId::try_from("other-session".to_owned()).expect("actor ID");
    let unsupported = route
        .prompt_contents(
            target.clone(),
            other_actor.clone().into(),
            InputId::generate(),
            session_event_model::ProviderSubmission::plain(vec![
                PromptContent::image("image/png".into(), "aGVsbG8=".into(), None).expect("image"),
            ]),
        )
        .await;
    assert!(matches!(
        unsupported,
        Err(ProviderPromptContentsError::Admission(
            ProviderQueueAdmissionError::UnsupportedContent {
                content_type: "image"
            }
        ))
    ));
    let submitted = route
        .prompt_contents(
            target.clone(),
            other_actor.into(),
            InputId::generate(),
            session_event_model::ProviderSubmission::plain(contents()),
        )
        .await
        .expect("two blocks admitted");
    let settled = collaboration_contracts::ProviderConversationBackend::wait(
        supervisor.as_ref(),
        collaboration_protocol::ConversationOperationWaitRequest {
            operation_id: submitted.operation.operation_id,
            timeout_seconds: collaboration_protocol::PositiveSeconds::try_from(2)
                .expect("wait seconds"),
        },
    )
    .await
    .expect("two-block prompt settles");
    assert!(matches!(
        settled.output,
        collaboration_protocol::ConversationOperationWaitOutput::Available { .. }
    ));
    route.shutdown_queue().await;
    supervisor.shutdown().await.expect("supervisor shutdown");
}

fn cancellable_prompt_fixture(dispatch_log: &std::path::Path) -> ExternalProviderLaunch {
    let script = node_test_fixture::fixture_program("provider-content-command-agent.ts");
    ExternalProviderLaunch {
        persistence_target: acp_client_runtime::ProviderPersistenceTarget::Unspecified,
        executable: PathBuf::from("node"),
        arguments: vec![
            script.to_string_lossy().into_owned(),
            "cancel".to_owned(),
            dispatch_log.to_string_lossy().into_owned(),
        ],
        environment: Vec::new(),
    }
}

#[tokio::test]
async fn active_control_turn_cancel_uses_operation_bound_path_and_settles_after_agent_stop() {
    let root = tempfile::tempdir().expect("provider root");
    let dispatch_log = root.path().join("cancel.log");
    let target = target();
    let binding = provider_binding(&target);
    let runtime = ExternalProviderRuntime::initialize(
        cancellable_prompt_fixture(&dispatch_log),
        std::sync::Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
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
    let route = ProviderAcpDeliveryRoute::new(
        target.endpoint.service_id.clone(),
        std::iter::once(target.endpoint.clone()).collect(),
        available_directory(&target, &binding),
        Arc::clone(&supervisor),
        store,
        Arc::new(NoLivePeer),
    );
    let idle_cancel = route
        .cancel_active_turn(target.clone(), target.clone().into())
        .await;
    assert!(matches!(
        idle_cancel,
        Err(codex_router_host::ProviderCancelActiveTurnError::NoActiveTurn)
    ));
    let prompt = collaboration_contracts::ProviderConversationBackend::prompt(
        supervisor.as_ref(),
        collaboration_protocol::ConversationPromptRequest {
            input_id: None,
            operation_id: collaboration_protocol::OperationId::generate(),
            target: target.clone(),
            generation: Some(binding.generation),
            requested_by: target.clone().into(),
            approver: target.clone().into(),
            prompt: collaboration_protocol::MessageContent::HumanUser {
                text: collaboration_protocol::MessageText::try_from("hold".to_owned())
                    .expect("prompt text"),
            },
        },
    )
    .await
    .expect("Control prompt admitted");
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if std::fs::read_to_string(&dispatch_log).is_ok_and(|text| text.contains("prompt\n")) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("agent receives Control prompt");
    let mut other_actor = target.clone();
    other_actor.session_id = SessionId::try_from("other-session".to_owned()).expect("actor ID");
    let cancel = route
        .cancel_active_turn(target.clone(), other_actor.into())
        .await
        .expect("active turn cancel admitted");
    let cancelled = collaboration_contracts::ProviderConversationBackend::wait(
        supervisor.as_ref(),
        collaboration_protocol::ConversationOperationWaitRequest {
            operation_id: cancel.operation.operation_id,
            timeout_seconds: collaboration_protocol::PositiveSeconds::try_from(2)
                .expect("wait seconds"),
        },
    )
    .await
    .expect("cancel operation settles");
    assert!(matches!(
        cancelled.output,
        collaboration_protocol::ConversationOperationWaitOutput::Available {
            settlement: collaboration_protocol::ConversationOperationSettlement::CancelRequested { .. }
        }
    ));
    let turn = collaboration_contracts::ProviderConversationBackend::wait(
        supervisor.as_ref(),
        collaboration_protocol::ConversationOperationWaitRequest {
            operation_id: prompt.operation.operation_id,
            timeout_seconds: collaboration_protocol::PositiveSeconds::try_from(2)
                .expect("wait seconds"),
        },
    )
    .await
    .expect("cancelled turn settles");
    assert!(matches!(
        turn.output,
        collaboration_protocol::ConversationOperationWaitOutput::Available {
            settlement: collaboration_protocol::ConversationOperationSettlement::PromptCompleted {
                stop_reason: collaboration_protocol::ProviderPromptStopReason::Cancelled,
                ..
            }
        }
    ));
    assert_eq!(
        std::fs::read_to_string(dispatch_log).expect("dispatch log"),
        "prompt\ncancel\n"
    );
    assert!(matches!(
        route
            .cancel_active_turn(target.clone(), target.into())
            .await,
        Err(codex_router_host::ProviderCancelActiveTurnError::NoActiveTurn)
    ));
    route.shutdown_queue().await;
    supervisor.shutdown().await.expect("supervisor shutdown");
}
