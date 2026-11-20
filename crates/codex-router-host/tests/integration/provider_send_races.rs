//! Send races through the real Host (D1 spec R12, R13, R15; V24, V25 and the
//! Host part of V28). The fixture changes the turn at a forced point; every
//! send still ends steered, queued or prompted under its one operation ID,
//! with one submission at the agent, no active-turn Busy and no cancel. A
//! reused request ID names its waiting or steered operation, or is refused.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "A Host or fixture that cannot start or answer must fail its test."
)]

use codex_router_host::{
    CollaborationRuntime, CollaborationRuntimeInputs, ExternalProviderBinding,
    ExternalProviderLaunchBinding, ExternalProviderRuntime, ExternalProviderStartup,
    ExternalProviderSupervisor, ProviderAcpDeliveryRoute,
};
use collaboration_client::{
    ClientError, CollaborationAccess, CollaborationClient, ConversationActor, ConversationClient,
    ConversationClientError, ConversationOperationResult, ConversationPromptInput,
    ConversationSettlementDetail,
};
use collaboration_contracts::ProviderConversationBackend as _;
use collaboration_protocol::ConversationAdmissionState as Admission;
use collaboration_protocol::MessageDelivery;
use collaboration_protocol::{
    ConversationCancelRequest, ConversationCreateRequest, ConversationOperationFailure,
    ConversationOperationFailureKind, ConversationOperationQueueState,
    ConversationOperationSettlement, ConversationOperationShowRequest,
    ConversationOperationSnapshot, ConversationOperationSubmission,
    ConversationOperationWaitOutput, ConversationOperationWaitRequest, ConversationPromptRequest,
    EndpointRef, OperationId, PositiveSeconds, ProviderBindingIdentity, ProviderIdentity,
    ProviderOperationEffect, ProviderOperationStage, ProviderRequestedPolicy, ProviderSettingName,
    ProviderSettingsResult, ProviderSettingsSetRequest, ProviderWorkingDirectory, RouterAccess,
    SessionId, SessionRef,
};
use collaboration_runtime::ProviderOperationStore;
use serde_json::Value;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::provider_acp_message_fifo_support::{
    NoLivePeer, available_directory, provider_binding, target,
};
use crate::provider_conversation_submission::submit_provider_create;
use crate::provider_fixture_launch;

mod direct_supervisor;
use direct_supervisor::DirectSupervisor;

/// How long a prompt call waits for its settlement before answering with what
/// it knows: a product argument. The test runner bounds each case
/// (`.config/nextest.toml`); test code never waits on a clock.
const SETTLE_WAIT: Duration = Duration::from_secs(20);

/// The message each test's running turn starts with.
const FIRST: &str = "first";
/// The message sent while that turn runs: the send under test.
const SECOND: &str = "second";

type PromptAnswer = Result<ConversationOperationResult, ConversationClientError>;

/// The frames the send-race fixture read with `method`, and with `value` at
/// `pointer` when one is named.
fn matching(receipt: &Path, method: &str, field: Option<(&str, &str)>) -> usize {
    fn text<'frame>(frame: &'frame Value, pointer: &str) -> Option<&'frame str> {
        frame.pointer(pointer).and_then(Value::as_str)
    }
    std::fs::read(receipt)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<Value>>(&bytes).ok())
        .unwrap_or_default()
        .iter()
        .filter(|frame| text(frame, "/method") == Some(method))
        .filter(|frame| field.is_none_or(|(pointer, value)| text(frame, pointer) == Some(value)))
        .count()
}

fn recorded(receipt: &Path, method: &str) -> usize {
    matching(receipt, method, None)
}

/// The frames with `method` whose prompt text is `text`.
fn carrying(receipt: &Path, method: &str, text: &str) -> usize {
    matching(receipt, method, Some(("/params/prompt/0/text", text)))
}

/// Every submission of `text`: prompts and steers.
fn submissions(receipt: &Path, text: &str) -> usize {
    carrying(receipt, "session/prompt", text) + carrying(receipt, "_session/steering", text)
}

/// Yields until `condition` holds; the runner bounds the case.
async fn until(mut condition: impl FnMut() -> bool) {
    while !condition() {
        tokio::task::yield_now().await;
    }
}

/// Lets the fixture answer what it holds behind `<receipt>.<name>`.
fn release(receipt: &Path, name: &str) {
    node_test_fixture::release_fixture(receipt, &format!(".{name}")).expect("release file");
}

fn human(id: &str) -> ProviderIdentity {
    serde_json::from_value(serde_json::json!({"kind":"human","humanId":id})).expect("human")
}

fn send_race_arguments(receipt: &Path, behaviours: &[&str]) -> Vec<String> {
    let mut arguments = vec!["send-race".to_owned(), receipt.display().to_string()];
    arguments.extend(behaviours.iter().map(|behaviour| (*behaviour).to_owned()));
    arguments
}

/// How one send ended, as its caller saw it.
#[derive(Debug, Eq, PartialEq)]
enum SendEnd {
    Prompted,
    Steered,
    Other(String),
}

fn send_end(answer: &PromptAnswer, operation_id: &OperationId) -> SendEnd {
    match answer {
        Ok(ConversationOperationResult::Completed {
            operation_id: Some(settled),
            settlement,
            ..
        }) if settled == operation_id => match settlement.detail {
            ConversationSettlementDetail::ProviderPrompt { .. } => SendEnd::Prompted,
            ConversationSettlementDetail::ProviderSteered { .. } => SendEnd::Steered,
            ref other => SendEnd::Other(format!("{other:?}")),
        },
        other => SendEnd::Other(format!("{other:?}")),
    }
}

fn router_queued(snapshot: Option<&ConversationOperationSnapshot>) -> bool {
    snapshot.is_some_and(|snapshot| {
        snapshot.queue_state == Some(ConversationOperationQueueState::RouterQueued)
    })
}

/// The delivery this suite's sends ask for: an agent that can't steer is
/// sent an explicit queue (D1 R12: auto would say cannot steer); any other
/// agent gets auto.
fn delivery_for(behaviours: &[&str]) -> MessageDelivery {
    if behaviours.contains(&"no-steering") {
        MessageDelivery::Queue
    } else {
        MessageDelivery::Auto
    }
}

/// A real Host whose one provider is the send-race fixture, driven through
/// the real client, API and supervisor.
struct ComposedHost {
    root: tempfile::TempDir,
    receipt: PathBuf,
    runtime: CollaborationRuntime,
    client: CollaborationClient,
    endpoint: EndpointRef,
    delivery: MessageDelivery,
}

impl ComposedHost {
    async fn start(behaviours: &[&str]) -> Self {
        let root = tempfile::tempdir().expect("runtime root");
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
            .expect("private runtime root");
        let receipt = root.path().join("wire.json");
        let mut arguments = vec![
            node_test_fixture::fixture_program("provider-steering-agent.ts")
                .to_string_lossy()
                .into_owned(),
        ];
        arguments.extend(send_race_arguments(&receipt, behaviours));
        let mut binding = ExternalProviderLaunchBinding::claude(
            node_test_fixture::node_executable().expect("Node"),
            arguments,
        )
        .expect("Claude binding");
        codex_router_test_support::global_claude_fixture::declare_global_claude_fixture(
            root.path(),
            &mut binding.launch.environment,
        )
        .expect("declare test-owned global Claude executable");
        let runtime = Box::pin(CollaborationRuntime::start_with_external_providers(
            CollaborationRuntimeInputs {
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
        let client = CollaborationClient::connect(root.path(), "send-races", "1")
            .await
            .expect("collaboration API client");
        let endpoint = client
            .list_endpoints()
            .await
            .expect("endpoint inventory")
            .endpoints
            .into_iter()
            .find(|endpoint| String::from(endpoint.endpoint.endpoint_id.clone()) == "claude-local")
            .expect("provider endpoint")
            .endpoint;
        Self {
            root,
            receipt,
            runtime,
            client,
            endpoint,
            delivery: delivery_for(behaviours),
        }
    }

    async fn create(&self, approver: &ProviderIdentity) -> SessionRef {
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
        match self.settled(&operation_id).await {
            Ok(ConversationOperationSettlement::Created { target, .. }) => target,
            other => panic!("expected a created session: {other:?}"),
        }
    }

    /// One API prompt as `approver` on its own task, answered after at most
    /// `SETTLE_WAIT`.
    fn spawn_prompt(
        &self,
        target: &SessionRef,
        approver: &ProviderIdentity,
        operation_id: &OperationId,
        text: &str,
    ) -> JoinHandle<PromptAnswer> {
        let directory = self.client.directory().to_owned();
        let input = ConversationPromptInput {
            delivery: self.delivery,
            operation_id: Some(operation_id.clone()),
            target: target.clone(),
            working_directory: None,
            requested_by: ConversationActor::Typed(approver.to_board_identity().expect("actor")),
            approver: Some(approver.clone()),
            message: serde_json::from_value(serde_json::json!({"kind":"humanUser","text":text}))
                .expect("prompt content"),
            effort: None,
            generation: None,
        };
        let endpoint = target.endpoint.clone();
        tokio::spawn(async move {
            ConversationClient::connect(&CollaborationAccess::api(&directory), &endpoint)
                .await?
                .prompt(input, SETTLE_WAIT, CancellationToken::new())
                .await
        })
    }

    /// The operation's snapshot, once the Host admitted it.
    async fn show(&self, operation_id: &OperationId) -> Option<ConversationOperationSnapshot> {
        self.client
            .show_provider_conversation_operation(ConversationOperationShowRequest {
                operation_id: operation_id.clone(),
            })
            .await
            .ok()
    }

    /// Waits until the operation waits in its session's line, or its caller
    /// was answered; says whether it was seen waiting.
    async fn until_queued_or_answered(
        &self,
        operation_id: &OperationId,
        answer: &JoinHandle<PromptAnswer>,
    ) -> bool {
        loop {
            if router_queued(self.show(operation_id).await.as_ref()) {
                return true;
            }
            if answer.is_finished() {
                return false;
            }
            tokio::task::yield_now().await;
        }
    }

    /// The operation's settlement, once it settles.
    async fn settled(
        &self,
        operation_id: &OperationId,
    ) -> Result<ConversationOperationSettlement, ClientError> {
        let request = ConversationOperationWaitRequest {
            operation_id: operation_id.clone(),
            timeout_seconds: PositiveSeconds::try_from(10).expect("timeout"),
        };
        loop {
            let waited = self
                .client
                .wait_for_provider_conversation_operation(request.clone())
                .await?;
            if let ConversationOperationWaitOutput::Available { settlement } = waited.output {
                return Ok(settlement);
            }
        }
    }

    /// Changes the session's effort to high on its own task.
    fn spawn_effort_change(
        &self,
        target: &SessionRef,
        actor: &ProviderIdentity,
    ) -> JoinHandle<Result<ProviderSettingsResult, ClientError>> {
        let directory = self.client.directory().to_owned();
        let request = ProviderSettingsSetRequest {
            target: target.clone(),
            actor: actor.clone(),
            setting: ProviderSettingName::Effort,
            value: "high".to_owned(),
        };
        tokio::spawn(async move {
            CollaborationClient::connect(&directory, "send-races-settings", "1")
                .await?
                .set_provider_conversation_setting(request)
                .await
        })
    }

    async fn shutdown(self) -> StoppedHost {
        let Self { root, runtime, .. } = self;
        let stopped = runtime
            .shutdown()
            .await
            .map_err(|error| format!("{error:?}"));
        StoppedHost { root, stopped }
    }
}

/// A stopped Host's folder, kept so its receipt holds every frame the agent
/// read, and how its shutdown went.
struct StoppedHost {
    root: tempfile::TempDir,
    stopped: Result<(), String>,
}

impl StoppedHost {
    fn receipt(&self) -> PathBuf {
        self.root.path().join("wire.json")
    }

    fn assert_stopped(&self) {
        assert!(self.stopped.is_ok(), "Host shutdown: {:?}", self.stopped);
    }
}

/// The send-race fixture's `index`th session (from 1).
fn fixture_session(index: usize) -> SessionRef {
    let mut session = target();
    if index > 1 {
        session.session_id =
            SessionId::try_from(format!("fixture-session-{index}")).expect("session");
    }
    session
}

const REUSED_ID: &str = "request ID already used for a different request";

type Submitted = Result<ConversationOperationSubmission, ConversationOperationFailure>;

fn admitted(submitted: &Submitted, operation_id: &OperationId, admission: Admission) -> bool {
    matches!(submitted, Ok(submission)
        if submission.admission == admission && &submission.operation.operation_id == operation_id)
}

fn refused_as_reused(submitted: &Submitted) -> bool {
    matches!(submitted, Err(failure)
        if failure.kind == ConversationOperationFailureKind::InvalidRequest
            && String::from(failure.message.clone()) == REUSED_ID)
}

fn prompt_completed(settlement: &ConversationOperationSettlement) -> bool {
    matches!(
        settlement,
        ConversationOperationSettlement::PromptCompleted { .. }
    )
}

/// Oracle: D1 spec R12 and V24 — "a race gets exactly one re-decision": when
/// a turn starts between a send's idle read and its dispatch, that send is
/// re-decided once and steered into the turn under its one operation ID,
/// never answered busy; the agent sees one submission per send and no
/// session/cancel. The fixture holds an effort change, so both sends read
/// the session idle before either dispatches: one starts the turn, the other
/// meets it at dispatch. One thread runs the Host, so a send the Host has
/// admitted has already asked for the session's activity.
#[tokio::test(flavor = "current_thread")]
async fn turn_starts_between_idle_read_and_dispatch() {
    // Arrange: an idle session whose agent holds an effort change.
    let host = ComposedHost::start(&["hold-effort-change"]).await;
    let owner = human("race-owner");
    let target = host.create(&owner).await;
    let setting = host.spawn_effort_change(&target, &owner);
    let effort = Some(("/params/configId", "effort"));
    until(|| matching(&host.receipt, "session/set_config_option", effort) == 1).await;
    let (first, second) = (OperationId::generate(), OperationId::generate());
    let first_answer = host.spawn_prompt(&target, &owner, &first, FIRST);
    let second_answer = host.spawn_prompt(&target, &owner, &second, SECOND);
    while host.show(&first).await.is_none() || host.show(&second).await.is_none() {
        tokio::task::yield_now().await;
    }

    // Act: both sends read the session idle; the change ends and both dispatch.
    release(&host.receipt, "setting-release");
    until(|| first_answer.is_finished() || second_answer.is_finished()).await;
    let prompts_while_running = recorded(&host.receipt, "session/prompt");
    release(&host.receipt, "release");
    let setting = setting.await.expect("setting task");
    let first_answer = first_answer.await.expect("first send task");
    let second_answer = second_answer.await.expect("second send task");
    let kept = host.shutdown().await;
    let receipt = kept.receipt();

    // Assert
    assert!(setting.is_ok(), "the held change applies: {setting:?}");
    let mut ends = [
        send_end(&first_answer, &first),
        send_end(&second_answer, &second),
    ];
    ends.sort_by_key(|end| format!("{end:?}"));
    assert_eq!(
        ends,
        [SendEnd::Prompted, SendEnd::Steered],
        "one send starts the turn, the other is steered into it, never busy"
    );
    assert_eq!(prompts_while_running, 1, "one turn started");
    assert_eq!(recorded(&receipt, "session/prompt"), 1, "one prompt");
    assert_eq!(recorded(&receipt, "_session/steering"), 1, "one steer");
    assert_eq!(submissions(&receipt, FIRST), 1, "one submission each");
    assert_eq!(submissions(&receipt, SECOND), 1, "one submission each");
    assert_eq!(recorded(&receipt, "session/cancel"), 0, "no cancel");
    kept.assert_stopped();
}

/// Oracle: D1 spec R12 and V24 — a steer that finds the turn already ended
/// (promptRequired) is re-decided once: the message is prompted, or queued
/// behind the ending turn, under its one operation ID, never answered busy,
/// with exactly one prompt at the agent and no session/cancel. "An unknown
/// steer outcome stays unknown and is never re-sent as a prompt": a steer
/// answered with an outcome nobody knows settles with an unknown effect, and
/// the agent, idle by then, receives no prompt for it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn turn_ends_before_steer_gives_prompt_required() {
    // Arrange: running turns whose agent ends them as the first steer arrives.
    let required = ComposedHost::start(&["first-steer-ends-turn"]).await;
    let unknown = ComposedHost::start(&["first-steer-unknown"]).await;
    let owner = human("race-owner");
    let (on_required, on_unknown) = (required.create(&owner).await, unknown.create(&owner).await);
    let (required_first, unknown_first) = (OperationId::generate(), OperationId::generate());
    let required_running = required.spawn_prompt(&on_required, &owner, &required_first, FIRST);
    let unknown_running = unknown.spawn_prompt(&on_unknown, &owner, &unknown_first, FIRST);
    until(|| recorded(&required.receipt, "session/prompt") == 1).await;
    until(|| recorded(&unknown.receipt, "session/prompt") == 1).await;

    // Act
    let (required_second, unknown_second) = (OperationId::generate(), OperationId::generate());
    let required_answer = required.spawn_prompt(&on_required, &owner, &required_second, SECOND);
    let required_answer = required_answer.await.expect("second send task");
    let unknown_answer = unknown.spawn_prompt(&on_unknown, &owner, &unknown_second, SECOND);
    let unknown_answer = unknown_answer.await.expect("second send task");
    let unknown_snapshot = unknown.show(&unknown_second).await;
    let required_running = required_running.await.expect("first send task");
    let unknown_running = unknown_running.await.expect("first send task");
    let (required_kept, unknown_kept) = (required.shutdown().await, unknown.shutdown().await);
    let required_receipt = required_kept.receipt();
    let unknown_receipt = unknown_kept.receipt();

    // Assert: promptRequired re-decides once, and the message is prompted.
    assert_eq!(
        send_end(&required_running, &required_first),
        SendEnd::Prompted
    );
    assert_eq!(
        send_end(&required_answer, &required_second),
        SendEnd::Prompted,
        "the ended turn's steer is prompted, never busy"
    );
    assert_eq!(carrying(&required_receipt, "_session/steering", SECOND), 1);
    assert_eq!(
        carrying(&required_receipt, "session/prompt", SECOND),
        1,
        "one prompt"
    );
    assert_eq!(
        recorded(&required_receipt, "session/cancel"),
        0,
        "no cancel"
    );
    // Assert: an unknown steer outcome stays unknown and is not re-sent.
    assert_eq!(
        send_end(&unknown_running, &unknown_first),
        SendEnd::Prompted
    );
    let unknown_end = send_end(&unknown_answer, &unknown_second);
    assert!(
        !matches!(unknown_end, SendEnd::Prompted | SendEnd::Steered),
        "an unknown steer is not reported as sent: {unknown_end:?}"
    );
    assert!(
        unknown_snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.stage == ProviderOperationStage::Terminal
                && snapshot.effect == ProviderOperationEffect::Unknown
        }),
        "the steer's outcome stays unknown: {unknown_snapshot:?}"
    );
    assert_eq!(carrying(&unknown_receipt, "_session/steering", SECOND), 1);
    let resent = carrying(&unknown_receipt, "session/prompt", SECOND);
    assert_eq!(resent, 0, "an unknown steer is never re-sent as a prompt");
    assert_eq!(recorded(&unknown_receipt, "session/cancel"), 0, "no cancel");
    required_kept.assert_stopped();
    unknown_kept.assert_stopped();
}

/// Oracle: D1 spec R12 and V24 — "a race gets exactly one re-decision, a
/// second contention queues": the steer finds no turn (promptRequired), the
/// one re-decision prompts, and that prompt meets a running turn again; the
/// message is queued under its one operation ID, never busy and never
/// re-decided again, then sent exactly once when the turn ends, with no
/// session/cancel. The fixture answers the steer promptRequired while its
/// turn's own end is still held, so a turn is running at the recheck.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn second_turn_starts_after_the_recheck() {
    // Arrange: a running turn whose agent answers the first steer promptRequired.
    let host = ComposedHost::start(&["first-steer-before-turn-end"]).await;
    let owner = human("race-owner");
    let target = host.create(&owner).await;
    let first = OperationId::generate();
    let running = host.spawn_prompt(&target, &owner, &first, FIRST);
    until(|| recorded(&host.receipt, "session/prompt") == 1).await;

    // Act
    let second = OperationId::generate();
    let answer = host.spawn_prompt(&target, &owner, &second, SECOND);
    let queued = host.until_queued_or_answered(&second, &answer).await;
    let waiting = host.show(&second).await;
    let steers_while_waiting = carrying(&host.receipt, "_session/steering", SECOND);
    let prompts_while_waiting = carrying(&host.receipt, "session/prompt", SECOND);
    release(&host.receipt, "release");
    let answer = answer.await.expect("second send task");
    let settled = [host.settled(&first).await, host.settled(&second).await];
    let running = running.await.expect("first send task");
    let kept = host.shutdown().await;
    let receipt = kept.receipt();

    // Assert
    assert!(
        queued,
        "the second contention queues: {waiting:?} {answer:?}"
    );
    assert_eq!(steers_while_waiting, 1, "one steer, no second re-decision");
    assert_eq!(prompts_while_waiting, 0, "nothing sent while it waits");
    assert!(
        answer.is_ok() && running.is_ok(),
        "never busy: {answer:?} {running:?}"
    );
    assert!(
        settled
            .iter()
            .all(|settled| settled.as_ref().is_ok_and(prompt_completed)),
        "both are prompted, under their own IDs: {settled:?}"
    );
    assert_eq!(carrying(&receipt, "_session/steering", SECOND), 1);
    assert_eq!(carrying(&receipt, "session/prompt", SECOND), 1, "sent once");
    assert_eq!(recorded(&receipt, "session/cancel"), 0, "no cancel");
    kept.assert_stopped();
}

/// Oracle: D1 spec R12 and R13, V25 — "no send interrupts a turn": a send to
/// a busy session that is steered, and one that is queued, leave the running
/// turn to end on its own; the agent records zero session/cancel.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_send_interrupts_a_turn() {
    // Arrange: a running turn on an agent that steers, and one on an agent that can't.
    let steering = ComposedHost::start(&[]).await;
    let queueing = ComposedHost::start(&["no-steering"]).await;
    let owner = human("race-owner");
    let (on_steering, on_queueing) = (steering.create(&owner).await, queueing.create(&owner).await);
    let (steer_first, queue_first) = (OperationId::generate(), OperationId::generate());
    let steer_running = steering.spawn_prompt(&on_steering, &owner, &steer_first, FIRST);
    let queue_running = queueing.spawn_prompt(&on_queueing, &owner, &queue_first, FIRST);
    until(|| recorded(&steering.receipt, "session/prompt") == 1).await;
    until(|| recorded(&queueing.receipt, "session/prompt") == 1).await;

    // Act
    let (steer_second, queue_second) = (OperationId::generate(), OperationId::generate());
    let steered = steering.spawn_prompt(&on_steering, &owner, &steer_second, SECOND);
    let steered = steered.await.expect("steered send task");
    let queue_answer = queueing.spawn_prompt(&on_queueing, &owner, &queue_second, SECOND);
    let queued = queueing
        .until_queued_or_answered(&queue_second, &queue_answer)
        .await;
    release(&steering.receipt, "release");
    release(&queueing.receipt, "release");
    let queue_answer = queue_answer.await.expect("queued send task");
    let steer_running = steer_running.await.expect("running turn task");
    let queue_running = queue_running.await.expect("running turn task");
    let (steering_kept, queueing_kept) = (steering.shutdown().await, queueing.shutdown().await);
    let steering_receipt = steering_kept.receipt();
    let queueing_receipt = queueing_kept.receipt();

    // Assert
    let cancels = [
        recorded(&steering_receipt, "session/cancel"),
        recorded(&queueing_receipt, "session/cancel"),
    ];
    assert_eq!(
        cancels,
        [0, 0],
        "neither a steer nor a queue cancels the turn"
    );
    assert_eq!(send_end(&steered, &steer_second), SendEnd::Steered);
    assert!(queued, "the send to the agent that can't steer queues");
    assert_eq!(send_end(&queue_answer, &queue_second), SendEnd::Prompted);
    let running_ends = [
        send_end(&steer_running, &steer_first),
        send_end(&queue_running, &queue_first),
    ];
    assert_eq!(
        running_ends,
        [SendEnd::Prompted, SendEnd::Prompted],
        "each ends on its own"
    );
    steering_kept.assert_stopped();
    queueing_kept.assert_stopped();
}

/// Oracle: D1 spec R15 and V28 (Host part) — two requests with one operation
/// ID that name different sessions, or different kinds, can both pass the
/// service's precheck; exactly one is admitted and the other is refused
/// invalidRequest "request ID already used for a different request". The
/// sessions pair is sent at once; the kinds pair (a prompt, then a cancel
/// under its ID) in the order that leaves the cancel second.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_same_id_misses_have_one_winner() {
    // Arrange: two sessions on an agent that can't steer; session one runs a turn.
    let host = DirectSupervisor::start(&["no-steering"], 2).await;
    let (one, two) = (fixture_session(1), fixture_session(2));
    let running = OperationId::generate();
    host.prompt(&one, &running, FIRST)
        .await
        .expect("the running turn");
    until(|| recorded(&host.receipt, "session/prompt") == 1).await;
    let (across_sessions, across_kinds) = (OperationId::generate(), OperationId::generate());
    let prechecks = [
        &across_sessions,
        &across_sessions,
        &across_kinds,
        &across_kinds,
    ];
    let prechecks_missed = host.prechecks_miss(&prechecks).await;

    // Act
    let (to_one, to_two) = tokio::join!(
        host.prompt(&one, &across_sessions, "to-one"),
        host.prompt(&two, &across_sessions, "to-two"),
    );
    let kind_prompt = host.prompt(&one, &across_kinds, "kind-prompt").await;
    let kind_cancel = host
        .supervisor
        .cancel(ConversationCancelRequest {
            operation_id: across_kinds.clone(),
            target_operation_id: running.clone(),
            target: one.clone(),
            generation: Some(host.binding.generation.clone()),
            requested_by: host.owner.clone(),
            approver: host.owner.clone(),
        })
        .await;
    let cancels_before_release = recorded(&host.receipt, "session/cancel");
    release(&host.receipt, "release");
    for operation_id in [&running, &across_sessions, &across_kinds] {
        host.settled(operation_id).await;
    }
    let receipt = host.receipt.clone();
    let _kept = host.shutdown().await;

    // Assert: different sessions, one admitted.
    assert!(prechecks_missed, "every request passes the precheck");
    let one_won = admitted(&to_one, &across_sessions, Admission::Admitted);
    let two_won = admitted(&to_two, &across_sessions, Admission::Admitted);
    assert!(
        (one_won && refused_as_reused(&to_two)) || (two_won && refused_as_reused(&to_one)),
        "exactly one session's request is admitted: {to_one:?} {to_two:?}"
    );
    let reached = carrying(&receipt, "session/prompt", "to-one")
        + carrying(&receipt, "session/prompt", "to-two");
    assert_eq!(reached, 1, "only the admitted request reaches the agent");
    // Assert: different kinds, one admitted.
    let prompt_won = admitted(&kind_prompt, &across_kinds, Admission::Admitted);
    assert!(prompt_won, "{kind_prompt:?}");
    assert!(
        refused_as_reused(&kind_cancel),
        "a cancel under a prompt's ID is refused, not given the prompt: {kind_cancel:?}"
    );
    assert_eq!(
        cancels_before_release, 0,
        "the refused cancel sends nothing"
    );
}

/// Oracle: D1 spec R15 and V28 (Host part) — the same request ID sent again
/// while its prompt waits in the session's line, one after the other or at
/// once, returns that waiting operation (Existing) instead of a second one;
/// when the turn ends the agent receives each message exactly once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn same_request_id_while_queued_returns_the_waiting_operation() {
    // Arrange: a running turn on an agent that can't steer.
    let host = DirectSupervisor::start(&["no-steering"], 1).await;
    let one = fixture_session(1);
    let running = OperationId::generate();
    host.prompt(&one, &running, FIRST)
        .await
        .expect("the running turn");
    until(|| recorded(&host.receipt, "session/prompt") == 1).await;
    let (sequential, concurrent) = (OperationId::generate(), OperationId::generate());
    let prechecks = [&sequential, &sequential, &concurrent, &concurrent];
    let prechecks_missed = host.prechecks_miss(&prechecks).await;

    // Act
    let queued = host.prompt(&one, &sequential, "queued-once").await;
    let again = host.prompt(&one, &sequential, "queued-once").await;
    let (left, right) = tokio::join!(
        host.prompt(&one, &concurrent, "queued-at-once"),
        host.prompt(&one, &concurrent, "queued-at-once"),
    );
    let waiting = [host.show(&sequential).await, host.show(&concurrent).await];
    let prompts_while_waiting = recorded(&host.receipt, "session/prompt");
    release(&host.receipt, "release");
    let mut settled = Vec::new();
    for operation_id in [&running, &sequential, &concurrent] {
        settled.push(host.settled(operation_id).await);
    }
    let receipt = host.receipt.clone();
    let _kept = host.shutdown().await;

    // Assert
    assert!(prechecks_missed, "every request passes the precheck");
    assert!(
        admitted(&queued, &sequential, Admission::Admitted),
        "{queued:?}"
    );
    let still_waiting = again
        .as_ref()
        .is_ok_and(|again| again.operation.stage != ProviderOperationStage::Terminal);
    assert!(
        admitted(&again, &sequential, Admission::Existing) && still_waiting,
        "the retry names the waiting operation: {again:?}"
    );
    let named = |winner: &Submitted, other: &Submitted| {
        admitted(winner, &concurrent, Admission::Admitted)
            && admitted(other, &concurrent, Admission::Existing)
    };
    assert!(
        named(&left, &right) || named(&right, &left),
        "one is admitted, the other names it: {left:?} {right:?}"
    );
    assert!(
        waiting
            .iter()
            .all(|snapshot| router_queued(snapshot.as_ref())),
        "both wait in the session's line: {waiting:?}"
    );
    assert_eq!(prompts_while_waiting, 1, "nothing sent while the turn runs");
    assert!(settled.iter().all(prompt_completed), "{settled:?}");
    assert_eq!(
        carrying(&receipt, "session/prompt", "queued-once"),
        1,
        "one dispatch"
    );
    assert_eq!(
        carrying(&receipt, "session/prompt", "queued-at-once"),
        1,
        "one dispatch"
    );
    assert_eq!(recorded(&receipt, "session/prompt"), 3, "no other prompt");
}

/// Oracle: D1 spec R15 and V28 (Host part) — the same request ID sent again
/// after its message was steered into the running turn returns that steered
/// operation (Existing) and injects nothing: the agent sees one steer and no
/// prompt for it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn same_request_id_after_steer_injects_nothing() {
    // Arrange: a running turn on an agent that steers.
    let host = DirectSupervisor::start(&[], 1).await;
    let one = fixture_session(1);
    let running = OperationId::generate();
    host.prompt(&one, &running, FIRST)
        .await
        .expect("the running turn");
    until(|| recorded(&host.receipt, "session/prompt") == 1).await;
    let steered = OperationId::generate();
    let prechecks_missed = host.prechecks_miss(&[&steered, &steered]).await;

    // Act
    let first = host.prompt(&one, &steered, SECOND).await;
    let settlement = host.settled(&steered).await;
    let again = host.prompt(&one, &steered, SECOND).await;
    release(&host.receipt, "release");
    let running_settlement = host.settled(&running).await;
    let receipt = host.receipt.clone();
    let _kept = host.shutdown().await;

    // Assert
    assert!(prechecks_missed, "both requests pass the precheck");
    assert!(admitted(&first, &steered, Admission::Admitted), "{first:?}");
    let was_steered = matches!(settlement, ConversationOperationSettlement::Steered { .. });
    assert!(was_steered, "{settlement:?}");
    assert!(
        admitted(&again, &steered, Admission::Existing),
        "the retry names the steered operation: {again:?}"
    );
    assert!(
        prompt_completed(&running_settlement),
        "{running_settlement:?}"
    );
    assert_eq!(
        carrying(&receipt, "_session/steering", SECOND),
        1,
        "one steer"
    );
    assert_eq!(
        carrying(&receipt, "session/prompt", SECOND),
        0,
        "no prompt for it"
    );
}
