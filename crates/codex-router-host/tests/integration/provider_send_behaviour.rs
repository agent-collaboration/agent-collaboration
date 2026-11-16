//! One send behaviour through the real client → API → ConversationOperations
//! → supervisor path (D1 spec R12, V22): a prompt to a busy session steers
//! when the agent can steer and queues when it can't; it never answers busy.

#![expect(
    clippy::expect_used,
    reason = "A Host or fixture that cannot start or answer must fail its test."
)]

use codex_router_host::{
    CollaborationRuntime, CollaborationRuntimeInputs, ExternalProviderLaunchBinding,
    ExternalProviderStartup,
};
use collaboration_client::{
    CollaborationAccess, CollaborationClient, ConversationActor, ConversationClient,
    ConversationOperationResult, ConversationPromptInput, ConversationSettlementDetail,
};
use collaboration_protocol::{
    ConversationCreateRequest, ConversationOperationSettlement, ConversationOperationWaitOutput,
    ConversationOperationWaitRequest, EndpointRef, OperationId, PositiveSeconds, ProviderIdentity,
    ProviderRequestedPolicy, ProviderWorkingDirectory, RouterAccess, SessionRef,
};
use serde_json::Value;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::provider_conversation_submission::submit_provider_create;

/// How long a prompt call waits before answering with what it knows.
const SHORT_WAIT: Duration = Duration::from_secs(1);

struct ComposedHost {
    root: tempfile::TempDir,
    runtime: CollaborationRuntime,
    client: CollaborationClient,
    endpoint: EndpointRef,
}

impl ComposedHost {
    async fn start(binding: ExternalProviderLaunchBinding, endpoint_id: &str) -> Self {
        let root = tempfile::tempdir().expect("runtime root");
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
            .expect("private runtime root");
        Self::start_in(root, binding, endpoint_id).await
    }

    async fn start_in(
        root: tempfile::TempDir,
        mut binding: ExternalProviderLaunchBinding,
        endpoint_id: &str,
    ) -> Self {
        codex_router_test_support::global_claude_fixture::declare_global_claude_fixture(
            root.path(),
            &mut binding.launch.environment,
        )
        .expect("declare test-owned global Claude executable");
        let runtime = Box::pin(CollaborationRuntime::start_with_external_providers(
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
        ))
        .await
        .expect("collaboration runtime");
        let client = CollaborationClient::connect(root.path(), "send-behaviour", "1")
            .await
            .expect("collaboration API client");
        let endpoint = client
            .list_endpoints()
            .await
            .expect("endpoint inventory")
            .endpoints
            .into_iter()
            .find(|endpoint| String::from(endpoint.endpoint.endpoint_id.clone()) == endpoint_id)
            .expect("provider endpoint")
            .endpoint;
        Self {
            root,
            runtime,
            client,
            endpoint,
        }
    }

    fn human(id: &str) -> ProviderIdentity {
        serde_json::from_value(serde_json::json!({"kind":"human","humanId":id})).expect("human")
    }

    async fn create(&self, approver: &ProviderIdentity) -> SessionRef {
        let operation_id = OperationId::generate();
        submit_provider_create(
            &self.client,
            ConversationCreateRequest {
                settings: Some(collaboration_protocol::ProviderRequestedSettings {
                    mode: Some("auto".into()),
                    ..collaboration_protocol::ProviderRequestedSettings::default()
                }),
                operation_id: operation_id.clone(),
                endpoint: self.endpoint.clone(),
                generation: None,
                working_directory: ProviderWorkingDirectory::try_from(
                    self.root.path().display().to_string(),
                )
                .expect("working directory"),
                created_by: approver.clone(),
                approver: approver.clone(),
                requested_policy: ProviderRequestedPolicy {
                    access: RouterAccess::WriteRestricted,
                },
            },
        )
        .await
        .expect("create admitted");
        let waited = self
            .client
            .wait_for_provider_conversation_operation(ConversationOperationWaitRequest {
                operation_id,
                timeout_seconds: PositiveSeconds::try_from(5).expect("timeout"),
            })
            .await
            .expect("create settles");
        match waited.output {
            ConversationOperationWaitOutput::Available {
                settlement: ConversationOperationSettlement::Created { target, .. },
            } => target,
            other => panic!("expected a created session: {other:?}"),
        }
    }

    /// One API prompt as `approver`, answered after at most `wait`.
    async fn prompt(
        &self,
        target: &SessionRef,
        approver: &ProviderIdentity,
        operation_id: OperationId,
        text: &str,
        wait: Duration,
    ) -> ConversationOperationResult {
        let conversation = ConversationClient::connect(
            &CollaborationAccess::api(self.client.directory()),
            &target.endpoint,
        )
        .await
        .expect("conversation client");
        let actor = ConversationActor::Typed(approver.to_board_identity().expect("actor"));
        let input = ConversationPromptInput {
            operation_id: Some(operation_id),
            target: target.clone(),
            working_directory: None,
            requested_by: actor,
            approver: Some(approver.clone()),
            message: serde_json::from_value(serde_json::json!({"kind":"humanUser","text":text}))
                .expect("prompt content"),
            effort: None,
            generation: None,
        };
        conversation
            .prompt(input, wait, CancellationToken::new())
            .await
            .expect("prompt answered")
    }
}

/// Every ACP request the fixture recorded with `method`.
fn recorded(receipt: &Path, method: &str) -> usize {
    std::fs::read(receipt)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<Value>>(&bytes).ok())
        .map_or(0, |requests| {
            requests
                .iter()
                .filter(|request| request["method"] == method)
                .count()
        })
}

/// Waits, bounded and without sleeping, until the fixture recorded `count`.
async fn until_recorded(receipt: &Path, method: &str, count: usize) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while recorded(receipt, method) < count {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("fixture records the request");
}

fn cursor_reactive(receipt: &Path, behaviours: &[&str]) -> ExternalProviderLaunchBinding {
    let mut arguments = vec![
        node_test_fixture::fixture_program("cursor-settings-agent.ts")
            .to_string_lossy()
            .into_owned(),
        "reactive".to_owned(),
        receipt.display().to_string(),
    ];
    arguments.extend(behaviours.iter().map(|behaviour| (*behaviour).to_owned()));
    ExternalProviderLaunchBinding::cursor(
        node_test_fixture::node_executable().expect("Node"),
        arguments,
    )
    .expect("Cursor binding")
}

/// Oracle: D1 spec R12 row "Busy, and the agent can't take input mid-turn"
/// and V22 — the second message waits in the session's line: the client says
/// queued, the agent sees no second prompt until the first turn ends, then
/// exactly one, and waiting again gives its completion.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prompt_to_a_busy_non_steering_agent_queues() {
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("wire.json");
    let host = ComposedHost::start(
        cursor_reactive(&receipt, &["hold-first-prompt"]),
        "cursor-local",
    )
    .await;
    let owner = ComposedHost::human("send-owner");
    let target = host.create(&owner).await;

    let first = OperationId::generate();
    let held = host
        .prompt(&target, &owner, first.clone(), "first", SHORT_WAIT)
        .await;
    until_recorded(&receipt, "session/prompt", 1).await;
    let second = OperationId::generate();
    let queued = host
        .prompt(&target, &owner, second.clone(), "second", SHORT_WAIT)
        .await;
    let prompts_while_held = recorded(&receipt, "session/prompt");
    std::fs::write(PathBuf::from(format!("{}.release", receipt.display())), b"")
        .expect("release the held turn");
    let completed = host
        .client
        .wait_for_provider_conversation_operation(ConversationOperationWaitRequest {
            operation_id: second.clone(),
            timeout_seconds: PositiveSeconds::try_from(10).expect("timeout"),
        })
        .await
        .expect("queued prompt settles");
    let prompts_after = recorded(&receipt, "session/prompt");
    let cancels = recorded(&receipt, "session/cancel");
    host.runtime.shutdown().await.expect("Host shutdown");

    assert!(
        matches!(held, ConversationOperationResult::Pending { .. }),
        "the first turn is held: {held:?}"
    );
    assert!(
        matches!(queued, ConversationOperationResult::Queued { ref operation_id, .. } if operation_id == &second),
        "the second message is queued, never busy: {queued:?}"
    );
    assert_eq!(
        prompts_while_held, 1,
        "no second prompt while the first turn runs"
    );
    assert!(
        matches!(
            completed.output,
            ConversationOperationWaitOutput::Available {
                settlement: ConversationOperationSettlement::PromptCompleted { .. }
            }
        ),
        "{completed:?}"
    );
    assert_eq!(
        prompts_after, 2,
        "exactly one more prompt after the turn ends"
    );
    assert_eq!(cancels, 0, "no send interrupts a turn");
}

/// Oracle: D1 spec R12 row "Busy, and the agent can take input mid-turn",
/// C1 and V22 — the message is steered into the running turn under the
/// caller's input ID; the client says steered and names the running turn's
/// approver (A), not the one this request named (B).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prompt_to_a_busy_steering_agent_steers() {
    let host = ComposedHost::start(
        ExternalProviderLaunchBinding::claude(
            node_test_fixture::node_executable().expect("Node"),
            vec![
                node_test_fixture::fixture_program("provider-steering-agent.ts")
                    .to_string_lossy()
                    .into_owned(),
                "injected".to_owned(),
            ],
        )
        .expect("Claude binding"),
        "claude-local",
    )
    .await;
    let approver_a = ComposedHost::human("approver-a");
    let approver_b = ComposedHost::human("approver-b");
    let target = host.create(&approver_a).await;

    let held = host
        .prompt(
            &target,
            &approver_a,
            OperationId::generate(),
            "first",
            SHORT_WAIT,
        )
        .await;
    let steered = host
        .prompt(
            &target,
            &approver_b,
            OperationId::generate(),
            "second",
            Duration::from_secs(10),
        )
        .await;
    host.runtime.shutdown().await.expect("Host shutdown");

    assert!(
        matches!(held, ConversationOperationResult::Pending { .. }),
        "the first turn is held: {held:?}"
    );
    match steered {
        ConversationOperationResult::Completed { settlement, .. } => match settlement.detail {
            ConversationSettlementDetail::ProviderSteered { turn_approver } => {
                assert_eq!(
                    turn_approver,
                    Some(approver_a),
                    "the running turn's approver"
                );
            }
            other => panic!("expected steered, got {other:?}"),
        },
        other => panic!("expected a steered completion, got {other:?}"),
    }
}
