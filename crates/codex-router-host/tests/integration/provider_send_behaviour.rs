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
use std::path::Path;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::provider_conversation_submission::submit_provider_create;

/// How long a prompt call waits before answering with what it knows.
pub(crate) const SHORT_WAIT: Duration = Duration::from_secs(1);

pub(crate) struct ComposedHost {
    pub(crate) root: tempfile::TempDir,
    pub(crate) runtime: CollaborationRuntime,
    pub(crate) client: CollaborationClient,
    pub(crate) endpoint: EndpointRef,
}

impl ComposedHost {
    pub(crate) async fn start(binding: ExternalProviderLaunchBinding, endpoint_id: &str) -> Self {
        let root = tempfile::tempdir().expect("runtime root");
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
            .expect("private runtime root");
        Self::start_in(root, binding, endpoint_id).await
    }

    pub(crate) async fn start_in(
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

    pub(crate) fn human(id: &str) -> ProviderIdentity {
        serde_json::from_value(serde_json::json!({"kind":"human","humanId":id})).expect("human")
    }

    pub(crate) async fn create(&self, approver: &ProviderIdentity) -> SessionRef {
        let operation_id = OperationId::generate();
        submit_provider_create(
            &self.client,
            ConversationCreateRequest {
                work_roots: Vec::new(),
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
        self.prompt_answer(
            target,
            approver,
            operation_id,
            text,
            wait,
            MessageDelivery::Auto,
        )
        .await
        .expect("prompt answered")
    }

    /// One API prompt with the caller's delivery choice, as it answered.
    async fn prompt_answer(
        &self,
        target: &SessionRef,
        approver: &ProviderIdentity,
        operation_id: OperationId,
        text: &str,
        wait: Duration,
        delivery: MessageDelivery,
    ) -> Result<ConversationOperationResult, collaboration_client::ConversationClientError> {
        let conversation = ConversationClient::connect(
            &CollaborationAccess::api(self.client.directory()),
            &target.endpoint,
        )
        .await
        .expect("conversation client");
        let actor = ConversationActor::Typed(approver.to_board_identity().expect("actor"));
        let input = ConversationPromptInput {
            delivery,
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
    }
}

/// Every ACP request the fixture recorded with `method`.
pub(crate) fn recorded(receipt: &Path, method: &str) -> usize {
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
/// Prompts the agent read whose request carries `text`.
fn prompts_carrying(receipt: &Path, text: &str) -> usize {
    std::fs::read(receipt)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<Value>>(&bytes).ok())
        .map_or(0, |requests| {
            requests
                .iter()
                .filter(|request| {
                    request.get("method").and_then(Value::as_str) == Some("session/prompt")
                        && request.to_string().contains(text)
                })
                .count()
        })
}

pub(crate) async fn until_recorded(receipt: &Path, method: &str, count: usize) {
    while recorded(receipt, method) < count {
        tokio::task::yield_now().await;
    }
}

pub(crate) fn cursor_reactive(
    receipt: &Path,
    behaviours: &[&str],
) -> ExternalProviderLaunchBinding {
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

/// Oracle: D1 spec R12 (owner, 16 Nov: "Cannot steer, interrupt or queue")
/// and V32: to a busy agent that can't steer, a CLI prompt with no choice is
/// not sent: CannotSteer, naming queue and interrupt, and no prompt reaches
/// the agent. With queue, it waits in the line and says Queued with its
/// follow-up, then runs as its own turn. Mail to the same busy session is
/// still queued.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prompt_to_a_busy_non_steering_agent_says_cannot_steer_or_queues_when_asked() {
    // Arrange: a held turn on an agent that can't steer.
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

    // Act: auto, then queue, then mail, while the turn runs.
    let refused = host
        .prompt_answer(
            &target,
            &owner,
            OperationId::generate(),
            "refused",
            SHORT_WAIT,
            MessageDelivery::Auto,
        )
        .await;
    let prompts_after_auto = recorded(&receipt, "session/prompt");
    let second = OperationId::generate();
    let queued = host
        .prompt_answer(
            &target,
            &owner,
            second.clone(),
            "second",
            SHORT_WAIT,
            MessageDelivery::Queue,
        )
        .await;
    let mailed = host.mail(&target).await;
    let prompts_while_held = recorded(&receipt, "session/prompt");
    node_test_fixture::release_fixture(&receipt, ".release").expect("release the held turn");
    let completed = host
        .client
        .wait_for_provider_conversation_operation(ConversationOperationWaitRequest {
            operation_id: second.clone(),
            timeout_seconds: PositiveSeconds::try_from(10).expect("timeout"),
        })
        .await
        .expect("queued prompt settles");
    until_recorded(&receipt, "session/prompt", 3).await;
    let refused_prompts = prompts_carrying(&receipt, "refused");
    let cancels = recorded(&receipt, "session/cancel");
    host.runtime.shutdown().await.expect("Host shutdown");

    // Assert
    assert!(
        matches!(held, ConversationOperationResult::Pending { .. }),
        "the first turn is held: {held:?}"
    );
    let failure = match &refused {
        Err(collaboration_client::ConversationClientError::Client(error)) => error
            .conversation_failure()
            .unwrap_or_else(|| panic!("a typed refusal: {refused:?}")),
        other => panic!("auto to an agent that can't steer is refused: {other:?}"),
    };
    assert_eq!(
        failure.kind,
        collaboration_protocol::ConversationOperationFailureKind::CannotSteer
    );
    assert_eq!(
        failure.effect,
        collaboration_protocol::ProviderOperationEffect::None
    );
    let message = String::from(failure.message);
    assert!(
        message.contains("delivery queue") && message.contains("conversation cancel"),
        "it names both choices: {message}"
    );
    assert_eq!(prompts_after_auto, 1, "nothing was sent on auto");
    match &queued {
        Ok(ConversationOperationResult::Queued {
            operation_id,
            follow_up,
            ..
        }) => {
            assert_eq!(operation_id, &second);
            assert!(
                follow_up.contains("conversation operation wait")
                    && follow_up.contains("conversation cancel"),
                "the follow-up says how to wait, cancel or interrupt: {follow_up}"
            );
        }
        other => panic!("queue waits in the line and says queued: {other:?}"),
    }
    assert_eq!(
        mailed.outcome,
        DeliveryOutcome::Queued,
        "mail is still queued: {mailed:?}"
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
        "the queued prompt runs as its own turn: {completed:?}"
    );
    assert_eq!(
        refused_prompts, 0,
        "the refused prompt never reaches the agent"
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
    CannotSteer,
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
            let answer = host
                .prompt_answer(
                    &target,
                    &owner,
                    OperationId::generate(),
                    "second",
                    Duration::from_secs(10),
                    MessageDelivery::Auto,
                )
                .await;
            let result = match answer {
                Ok(result) => result,
                Err(collaboration_client::ConversationClientError::Client(ref error))
                    if error.conversation_failure().is_some_and(|failure| {
                        failure.kind
                            == collaboration_protocol::ConversationOperationFailureKind::CannotSteer
                    }) =>
                {
                    return finish_case(host, &receipt, door, state, SendOutcome::CannotSteer)
                        .await;
                }
                Err(error) => panic!("{door:?} {state:?}: the CLI failed {error:?}"),
            };
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
    finish_case(host, &receipt, door, state, outcome).await
}

/// Lets a held turn end (a queued message then runs once after it), and
/// checks no send interrupted the turn.
async fn finish_case(
    host: ComposedHost,
    receipt: &Path,
    door: Door,
    state: SessionStateAtSend,
    outcome: SendOutcome,
) -> SendOutcome {
    if matches!(state, SessionStateAtSend::BusyNonSteering) {
        node_test_fixture::release_fixture(receipt, ".release").expect("release the held turn");
        if outcome == SendOutcome::Queued {
            until_recorded(receipt, "session/prompt", 2).await;
        }
    }
    let cancels = recorded(receipt, "session/cancel");
    host.runtime.shutdown().await.expect("Host shutdown");
    assert_eq!(cancels, 0, "{door:?} {state:?}: no send interrupts a turn");
    outcome
}

/// Oracle: D1 spec R12 "one send behaviour" and V23, with R12's 16 Nov
/// change: for idle and busy-steerable sessions, an Auto mail delivery and a
/// CLI prompt end prompted or steered the same way. For a busy agent that
/// can't steer, mail still queues and the CLI with no choice says cannot
/// steer. Mail keeps its Router ACP evidence, and neither deadlocks or
/// interrupts the turn.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mail_and_cli_outcomes_for_each_session_state() {
    let cases = [
        (
            SessionStateAtSend::Idle,
            (SendOutcome::Prompted, SendOutcome::Prompted),
        ),
        (
            SessionStateAtSend::BusySteering,
            (SendOutcome::Steered, SendOutcome::Steered),
        ),
        (
            SessionStateAtSend::BusyNonSteering,
            (SendOutcome::Queued, SendOutcome::CannotSteer),
        ),
    ];
    for (state, expected) in cases {
        let mail = outcome_through(Door::Mail, state).await;
        let cli = outcome_through(Door::Cli, state).await;
        assert_eq!((mail, cli), expected, "{state:?}");
    }
}

/// Oracle: D1 spec R12, "a caller that explicitly asks to steer an agent that
/// can't steer is refused with 'steer unsupported', unchanged from main", and
/// message send's rule mirrored for an idle session (no running turn): both
/// explicit steers are refused, nothing reaches the agent, and no turn is
/// interrupted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_explicit_steer_without_a_turn_to_join_is_refused() {
    // Arrange: an idle steering agent, and a held turn on one that can't steer.
    let steering = ComposedHost::start(claude_steering(), "claude-local").await;
    let steering_owner = ComposedHost::human("steer-owner");
    let idle = steering.create(&steering_owner).await;
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("wire.json");
    let non_steering = ComposedHost::start(
        cursor_reactive(&receipt, &["hold-first-prompt"]),
        "cursor-local",
    )
    .await;
    let owner = ComposedHost::human("steer-owner");
    let busy = non_steering.create(&owner).await;
    let _held = non_steering
        .prompt(&busy, &owner, OperationId::generate(), "first", SHORT_WAIT)
        .await;
    until_recorded(&receipt, "session/prompt", 1).await;

    // Act
    let to_idle = steering
        .prompt_answer(
            &idle,
            &steering_owner,
            OperationId::generate(),
            "steer-idle",
            SHORT_WAIT,
            MessageDelivery::Steer,
        )
        .await;
    let to_non_steering = non_steering
        .prompt_answer(
            &busy,
            &owner,
            OperationId::generate(),
            "steer-busy",
            SHORT_WAIT,
            MessageDelivery::Steer,
        )
        .await;
    let prompts = recorded(&receipt, "session/prompt");
    let cancels = recorded(&receipt, "session/cancel");
    steering.runtime.shutdown().await.expect("Host shutdown");
    non_steering
        .runtime
        .shutdown()
        .await
        .expect("Host shutdown");

    // Assert
    let refusal = |answer: &Result<
        ConversationOperationResult,
        collaboration_client::ConversationClientError,
    >| {
        match answer {
            Err(collaboration_client::ConversationClientError::Client(error)) => error
                .conversation_failure()
                .map(|failure| (failure.kind, String::from(failure.message))),
            _ => None,
        }
    };
    assert_eq!(
        refusal(&to_idle),
        Some((
            collaboration_protocol::ConversationOperationFailureKind::InvalidRequest,
            "no running turn to steer".to_owned()
        )),
        "{to_idle:?}"
    );
    assert_eq!(
        refusal(&to_non_steering),
        Some((
            collaboration_protocol::ConversationOperationFailureKind::UnsupportedCapability,
            "steer unsupported by provider".to_owned()
        )),
        "{to_non_steering:?}"
    );
    assert_eq!(prompts, 1, "neither steer reached the agent");
    assert_eq!(cancels, 0, "no turn is interrupted");
}
