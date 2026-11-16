//! V29: the Host keeps an idle Claude session's cache warm on its one injected clock.
//! A real Claude-profile ACP fixture ends a turn; the Host's ManualClock advances; the
//! fixture sees one warm prompt per 55 minutes and none once the Host has shut down.
use codex_router_host::{
    CacheWarmSetting, CollaborationRuntime, CollaborationRuntimeInputs,
    ExternalProviderLaunchBinding, ExternalProviderStartup,
};
use collaboration_client::CollaborationClient;
use collaboration_protocol::{
    ChannelDescription, CodexGeneration, ConversationCreateRequest,
    ConversationOperationSettlement, ConversationOperationWaitOutput,
    ConversationOperationWaitRequest, ConversationPromptRequest, EndpointId, EndpointRef,
    MessageContent, MessageText, OperationId, PositiveSeconds, ProviderRequestedPolicy,
    ProviderWorkingDirectory, RouterAccess, SessionId, SessionRef,
};
use std::{os::unix::fs::PermissionsExt as _, path::Path, time::Duration};

use crate::provider_conversation_submission::{submit_provider_create, submit_provider_prompt};

use crate::host_clock_control;

const WARM_PROMPT: &str = "Cache warm: reply ok";
const FIFTY_FIVE_MINUTES: Duration = Duration::from_secs(55 * 60);

fn prompts_containing(log: &Path, text: &str) -> usize {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|entry| {
            entry.get("method").and_then(serde_json::Value::as_str) == Some("session/prompt")
                && entry
                    .get("params")
                    .and_then(|params| params.get("prompt"))
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|parts| {
                        parts.iter().any(|part| {
                            part.get("text").and_then(serde_json::Value::as_str) == Some(text)
                        })
                    })
        })
        .count()
}

/// The test runner bounds the case (`.config/nextest.toml`), so test code
/// never waits on Tokio's clock.
async fn wait_for_warm_prompts(log: &Path, expected: usize) {
    while prompts_containing(log, WARM_PROMPT) < expected {
        tokio::task::yield_now().await;
    }
}

/// The Host armed its warm timer: a sleep 55 minutes ahead was checked and was not due.
async fn wait_for_warm_timer_armed(runtime: &CollaborationRuntime) {
    while !runtime.runtime_test_sleep_was_checked(FIFTY_FIVE_MINUTES) {
        tokio::task::yield_now().await;
    }
}

#[expect(
    clippy::expect_used,
    reason = "A malformed fixture must fail its test."
)]
#[expect(clippy::panic, reason = "An unexpected settlement fails the test.")]
async fn create_claude_session(
    client: &CollaborationClient,
    actor: &SessionRef,
    working_directory: &Path,
) -> (SessionRef, CodexGeneration) {
    let inventory = client.list_endpoints().await.expect("endpoint inventory");
    let endpoint = inventory
        .endpoints
        .iter()
        .find(|endpoint| String::from(endpoint.endpoint.endpoint_id.clone()) == "claude-local")
        .expect("Claude endpoint");
    let binding_generation = endpoint
        .channels
        .iter()
        .find_map(|channel| match channel {
            ChannelDescription::ExternalProvider {
                binding_generation, ..
            } => Some(*binding_generation),
            _ => None,
        })
        .expect("Claude binding generation");
    let generation = CodexGeneration {
        service_epoch: inventory.service_epoch.clone(),
        generation: binding_generation,
    };
    let operation_id = OperationId::generate();
    submit_provider_create(
        client,
        ConversationCreateRequest {
            settings: Some(collaboration_protocol::ProviderRequestedSettings {
                mode: Some("auto".into()),
                ..collaboration_protocol::ProviderRequestedSettings::default()
            }),
            operation_id: operation_id.clone(),
            endpoint: endpoint.endpoint.clone(),
            generation: Some(generation.clone()),
            working_directory: ProviderWorkingDirectory::try_from(
                working_directory.display().to_string(),
            )
            .expect("working directory"),
            created_by: actor.clone().into(),
            approver: actor.clone().into(),
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
        },
    )
    .await
    .expect("create admitted");
    let created = client
        .wait_for_provider_conversation_operation(ConversationOperationWaitRequest {
            operation_id,
            timeout_seconds: PositiveSeconds::try_from(3).expect("timeout"),
        })
        .await
        .expect("create settled");
    let ConversationOperationWaitOutput::Available {
        settlement: ConversationOperationSettlement::Created { target, .. },
    } = created.output
    else {
        panic!("the Claude fixture did not create its session: {created:?}")
    };
    (target, generation)
}

#[tokio::test]
async fn cache_warm_runs_through_host_composition() {
    let root = tempfile::Builder::new()
        .prefix("cache-warm-")
        .tempdir_in("/tmp")
        .expect("runtime root");
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private runtime root");
    let provider = root.path().join("claude-provider.ts");
    node_test_fixture::write_node_fixture(
        &provider,
        "session-message-route-agent.ts",
        &serde_json::json!(null),
    )
    .expect("provider fixture");
    std::fs::set_permissions(&provider, std::fs::Permissions::from_mode(0o700))
        .expect("provider executable");
    let loads = root.path().join("claude-loads.log");
    let prompts = root.path().join("claude-prompts.log");
    let clock_listener =
        host_clock_control::prepare(root.path()).expect("Host clock control input");
    let mut binding = ExternalProviderLaunchBinding::claude(
        provider,
        vec![
            "cache-warm-session".to_owned(),
            loads.display().to_string(),
            prompts.display().to_string(),
        ],
    )
    .expect("Claude binding")
    .with_cache_warm(CacheWarmSetting::default());
    codex_router_test_support::global_claude_fixture::declare_global_claude_fixture(
        root.path(),
        &mut binding.launch.environment,
    )
    .expect("declare test-owned global Claude executable");
    let runtime = CollaborationRuntime::start_with_external_providers(
        CollaborationRuntimeInputs {
            board_session: None,
            directory: root.path().to_owned(),
            codex_home: root.path().to_owned(),
            backend_socket: root.path().join("backend.sock"),
            mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
            native_schema: None,
            peer_registry_directory: None,
            remote_control_server_name: None,
            owner_human_id: None,
        },
        vec![ExternalProviderStartup::Launch(binding)],
    )
    .await
    .expect("Host starts through startup composition");
    let mut clock = host_clock_control::HostClockControl::connected(clock_listener)
        .await
        .expect("Host clock control");
    let client = CollaborationClient::connect(root.path(), "cache-warm", "1")
        .await
        .expect("collaboration API client");
    let actor = SessionRef {
        endpoint: EndpointRef {
            service_id: runtime.service_id().clone(),
            endpoint_id: EndpointId::try_from("codex-local".to_owned()).expect("actor endpoint"),
        },
        session_id: SessionId::try_from("cache-warm-caller".to_owned()).expect("actor session"),
    };
    let (target, generation) = create_claude_session(&client, &actor, root.path()).await;

    // A real turn: the owner's prompt, answered end_turn by the fixture.
    let prompt_operation = OperationId::generate();
    submit_provider_prompt(
        &client,
        ConversationPromptRequest {
            input_id: None,
            operation_id: prompt_operation.clone(),
            target,
            generation: Some(generation),
            requested_by: actor.clone().into(),
            approver: actor.clone().into(),
            prompt: MessageContent::Agent {
                sender: actor,
                text: MessageText::try_from("real work".to_owned()).expect("prompt"),
            },
        },
    )
    .await
    .expect("real prompt admitted");
    client
        .wait_for_provider_conversation_operation(ConversationOperationWaitRequest {
            operation_id: prompt_operation,
            timeout_seconds: PositiveSeconds::try_from(3).expect("timeout"),
        })
        .await
        .expect("real prompt settled");
    wait_for_warm_timer_armed(&runtime).await;
    assert_eq!(prompts_containing(&prompts, WARM_PROMPT), 0);

    clock.advance(FIFTY_FIVE_MINUTES).await.expect("55 minutes");
    wait_for_warm_prompts(&prompts, 1).await;
    wait_for_warm_timer_armed(&runtime).await;

    clock
        .advance(FIFTY_FIVE_MINUTES)
        .await
        .expect("55 more minutes");
    wait_for_warm_prompts(&prompts, 2).await;
    wait_for_warm_timer_armed(&runtime).await;

    runtime.shutdown().await.expect("Host shutdown");
    assert_eq!(
        prompts_containing(&prompts, WARM_PROMPT),
        2,
        "two warm prompts, and none once the Host has shut down"
    );
}
