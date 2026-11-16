//! One send behaviour through the real client → API → ConversationOperations
//! → supervisor path (D1 spec R12, V22): a prompt to a busy session steers
//! when the agent can steer and queues when it can't; it never answers busy.

#![expect(
    clippy::expect_used,
    clippy::panic,
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
    ConversationOperationWaitRequest, DeliveryClientReceipt, DeliveryOutcome, EndpointRef,
    MessageDelivery, MessageSendResult, MessageText, OperationId, PositiveSeconds,
    ProviderIdentity, ProviderRequestedPolicy, ProviderWorkingDirectory, RouterAccess,
    SessionReachability, SessionRef,
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

    /// One Auto mail to `target` from a human, as `message send` sends it.
    async fn mail(&self, target: &SessionRef) -> collaboration_protocol::DeliveryReceipt {
        let sent = self
            .client
            .send_message(collaboration_client::MessageSendRequest {
                item_id: None,
                target: target.clone(),
                message: collaboration_client::PublicMessageContent::HumanUser {
                    text: MessageText::try_from("second".to_owned()).expect("mail text"),
                },
                delivery: MessageDelivery::Auto,
                generation_guard: None,
            })
            .await
            .expect("mail receipt");
        match sent {
            MessageSendResult::Local(sent) => sent.receipt,
            MessageSendResult::Queued { .. } => panic!("a local session took sender custody"),
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

/// Waits until the fixture recorded `count`; the test runner bounds the case
/// (`.config/nextest.toml`), so test code never waits on Tokio's clock.
async fn until_recorded(receipt: &Path, method: &str, count: usize) {
    while recorded(receipt, method) < count {
        tokio::task::yield_now().await;
    }
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

/// How a send to a session ended, in the same words for every door (R12).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SendOutcome {
    Prompted,
    Steered,
    Queued,
}

/// The state the session is in when the second message arrives.
#[derive(Clone, Copy, Debug)]
enum SessionStateAtSend {
    Idle,
    BusySteering,
    BusyNonSteering,
}

/// The door the second message comes through.
#[derive(Clone, Copy, Debug)]
enum Door {
    Mail,
    Cli,
}

fn claude_steering() -> ExternalProviderLaunchBinding {
    ExternalProviderLaunchBinding::claude(
        node_test_fixture::node_executable().expect("Node"),
        vec![
            node_test_fixture::fixture_program("provider-steering-agent.ts")
                .to_string_lossy()
                .into_owned(),
            "injected".to_owned(),
        ],
    )
    .expect("Claude binding")
}

/// What one door's message did to a session in `state`, on its own Host:
/// the outcome, plus the evidence each door keeps (mail: its Router ACP
/// receipt naming the operation; a queued message: it runs once after the
/// turn ends), and no session/cancel at the agent.
async fn outcome_through(door: Door, state: SessionStateAtSend) -> SendOutcome {
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("wire.json");
    let (binding, endpoint) = match state {
        SessionStateAtSend::Idle => (cursor_reactive(&receipt, &[]), "cursor-local"),
        SessionStateAtSend::BusyNonSteering => (
            cursor_reactive(&receipt, &["hold-first-prompt"]),
            "cursor-local",
        ),
        SessionStateAtSend::BusySteering => (claude_steering(), "claude-local"),
    };
    let host = ComposedHost::start(binding, endpoint).await;
    let owner = ComposedHost::human("same-outcome-owner");
    let target = host.create(&owner).await;
    if !matches!(state, SessionStateAtSend::Idle) {
        let held = host
            .prompt(
                &target,
                &owner,
                OperationId::generate(),
                "first",
                SHORT_WAIT,
            )
            .await;
        assert!(
            matches!(held, ConversationOperationResult::Pending { .. }),
            "{door:?} {state:?}: the first turn is held: {held:?}"
        );
    }
    if matches!(state, SessionStateAtSend::BusyNonSteering) {
        until_recorded(&receipt, "session/prompt", 1).await;
    }
    let outcome = match door {
        Door::Mail => {
            let receipt = host.mail(&target).await;
            assert_eq!(
                receipt.reachability,
                Some(SessionReachability::ProviderAcp),
                "{door:?} {state:?}: mail keeps its evidence: {receipt:?}"
            );
            assert!(
                matches!(
                    receipt.client,
                    Some(DeliveryClientReceipt::ProviderAcp { .. })
                ),
                "{door:?} {state:?}: mail names its Router operation: {receipt:?}"
            );
            match receipt.outcome {
                DeliveryOutcome::Started => SendOutcome::Prompted,
                DeliveryOutcome::Steered => SendOutcome::Steered,
                DeliveryOutcome::Queued => SendOutcome::Queued,
                other => panic!("{door:?} {state:?}: mail ended {other:?}"),
            }
        }
        Door::Cli => {
            let result = host
                .prompt(
                    &target,
                    &owner,
                    OperationId::generate(),
                    "second",
                    Duration::from_secs(10),
                )
                .await;
            match result {
                ConversationOperationResult::Queued { .. } => SendOutcome::Queued,
                ConversationOperationResult::Completed { ref settlement, .. } => {
                    match settlement.detail {
                        ConversationSettlementDetail::ProviderSteered { .. } => {
                            SendOutcome::Steered
                        }
                        ConversationSettlementDetail::ProviderPrompt { .. } => {
                            SendOutcome::Prompted
                        }
                        _ => panic!("{door:?} {state:?}: the CLI ended {result:?}"),
                    }
                }
                other => panic!("{door:?} {state:?}: the CLI ended {other:?}"),
            }
        }
    };
    if matches!(state, SessionStateAtSend::BusyNonSteering) {
        std::fs::write(PathBuf::from(format!("{}.release", receipt.display())), b"")
            .expect("release the held turn");
        // The queued message runs once, after the held turn ends.
        until_recorded(&receipt, "session/prompt", 2).await;
    }
    let cancels = recorded(&receipt, "session/cancel");
    host.runtime.shutdown().await.expect("Host shutdown");
    assert_eq!(cancels, 0, "{door:?} {state:?}: no send interrupts a turn");
    outcome
}

/// Oracle: D1 spec R12 "one send behaviour" and V23 — for idle,
/// busy-steerable and busy-non-steerable sessions, an Auto mail delivery and
/// a CLI prompt both end prompted, steered or queued the same way; mail keeps
/// its Router ACP evidence, and neither deadlocks or interrupts the turn.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mail_and_cli_reach_the_same_outcome_for_the_same_session_state() {
    let cases = [
        (SessionStateAtSend::Idle, SendOutcome::Prompted),
        (SessionStateAtSend::BusySteering, SendOutcome::Steered),
        (SessionStateAtSend::BusyNonSteering, SendOutcome::Queued),
    ];
    for (state, expected) in cases {
        let mail = outcome_through(Door::Mail, state).await;
        let cli = outcome_through(Door::Cli, state).await;
        assert_eq!((mail, cli), (expected, expected), "{state:?}");
    }
}
