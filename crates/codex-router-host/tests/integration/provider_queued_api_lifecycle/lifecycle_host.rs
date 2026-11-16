//! One real Host with one Cursor ACP fixture, driven through the real client
//! → API → supervisor path, for the queued API prompt's lifecycle (D1 R12,
//! R14). Every wait here loops on the runtime without reading a clock; the
//! nextest override for `provider_queued_api_lifecycle::` bounds each case.

#![expect(
    clippy::panic,
    reason = "A Host or fixture that cannot start or answer must fail its test."
)]

use codex_router_host::{
    CollaborationRuntime, CollaborationRuntimeInputs, ExternalProviderLaunchBinding,
    ExternalProviderStartup,
};
use collaboration_client::{
    ClientError, CollaborationAccess, CollaborationClient, ConversationActor, ConversationClient,
    ConversationClientError, ConversationOperationResult, ConversationPromptInput,
};
use collaboration_protocol::MessageDelivery;
use collaboration_protocol::{
    ConversationCloseRequest, ConversationCreateRequest, ConversationLoadRequest,
    ConversationOperationQueueState, ConversationOperationSettlement,
    ConversationOperationShowRequest, ConversationOperationSnapshot,
    ConversationOperationSubmission, ConversationOperationWaitOutput,
    ConversationOperationWaitRequest, ConversationOperationWaitResult, EndpointRef, OperationId,
    PositiveSeconds, ProviderIdentity, ProviderRequestedPolicy, ProviderRequestedSettings,
    ProviderWorkingDirectory, RouterAccess, SessionRef,
};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::UnixStream;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::provider_conversation_submission::{submit_provider_create, submit_provider_load};

/// How long one prompt call waits before answering with what it knows.
pub(super) const SHORT_WAIT: Duration = Duration::from_secs(1);
/// How long an operation wait may run on the Host before it answers pending.
pub(super) const SETTLE_SECONDS: u32 = 30;
/// Prompt calls in flight at once, well under the API's 64 request slots.
const PROMPTS_IN_FLIGHT: usize = 24;
/// The endpoint the Cursor fixture serves.
pub(super) const CURSOR_ENDPOINT: &str = "cursor-local";

pub(super) type PromptCall =
    JoinHandle<Result<ConversationOperationResult, ConversationClientError>>;

/// Where a Host keeps its state: a root this test owns, or one a parent test
/// owns and hands to a relaunched child.
pub(super) enum HostRoot {
    Owned(tempfile::TempDir),
    Borrowed(PathBuf),
}

impl HostRoot {
    pub(super) fn path(&self) -> &Path {
        match self {
            Self::Owned(root) => root.path(),
            Self::Borrowed(root) => root,
        }
    }
}

pub(super) struct LifecycleHost {
    pub(super) root: HostRoot,
    pub(super) runtime: CollaborationRuntime,
    pub(super) client: CollaborationClient,
    pub(super) endpoint: EndpointRef,
}

/// A private runtime root, as the Host requires.
pub(super) fn private_root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap_or_else(|error| panic!("runtime root: {error}"));
    if let Err(error) =
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
    {
        panic!("private runtime root: {error}");
    }
    root
}

/// The Cursor binding for the reactive fixture with `behaviours`, recording to `receipt`.
pub(super) fn cursor_reactive(
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
    let node = node_test_fixture::node_executable().unwrap_or_else(|error| panic!("Node: {error}"));
    ExternalProviderLaunchBinding::cursor(node, arguments)
        .unwrap_or_else(|error| panic!("Cursor binding: {error}"))
}

pub(super) fn human(id: &str) -> ProviderIdentity {
    serde_json::from_value(json!({"kind":"human","humanId":id}))
        .unwrap_or_else(|error| panic!("human identity: {error}"))
}

impl LifecycleHost {
    pub(super) async fn start(binding: ExternalProviderLaunchBinding) -> Self {
        Self::start_in(private_root(), binding).await
    }

    /// Starts a Host on `root`, which may hold an earlier Host's state.
    pub(super) async fn start_in(
        root: tempfile::TempDir,
        binding: ExternalProviderLaunchBinding,
    ) -> Self {
        Self::start_on(HostRoot::Owned(root), binding, CURSOR_ENDPOINT).await
    }

    /// Starts a Host on `root` serving the provider endpoint `endpoint_id`.
    pub(super) async fn start_on(
        root: HostRoot,
        mut binding: ExternalProviderLaunchBinding,
        endpoint_id: &str,
    ) -> Self {
        if let Err(error) =
            codex_router_test_support::global_claude_fixture::declare_global_claude_fixture(
                root.path(),
                &mut binding.launch.environment,
            )
        {
            panic!("declare test-owned global Claude executable: {error}");
        }
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
        .unwrap_or_else(|error| panic!("collaboration runtime: {error}"));
        let client = CollaborationClient::connect(root.path(), "queued-api-lifecycle", "1")
            .await
            .unwrap_or_else(|error| panic!("collaboration API client: {error}"));
        let endpoint = client
            .list_endpoints()
            .await
            .unwrap_or_else(|error| panic!("endpoint inventory: {error}"))
            .endpoints
            .into_iter()
            .find(|endpoint| String::from(endpoint.endpoint.endpoint_id.clone()) == endpoint_id)
            .unwrap_or_else(|| panic!("the {endpoint_id} endpoint is listed"))
            .endpoint;
        Self {
            root,
            runtime,
            client,
            endpoint,
        }
    }

    /// Stops the Host gracefully and hands back its root for a restart.
    pub(super) async fn stop(self) -> (tempfile::TempDir, std::io::Result<()>) {
        let stopped = self.runtime.shutdown().await;
        match self.root {
            HostRoot::Owned(root) => (root, stopped),
            HostRoot::Borrowed(root) => panic!(
                "a borrowed root {} is not this test's to stop and return",
                root.display()
            ),
        }
    }

    pub(super) async fn create(&self, approver: &ProviderIdentity) -> SessionRef {
        let operation_id = OperationId::generate();
        let working_directory =
            ProviderWorkingDirectory::try_from(self.root.path().display().to_string())
                .unwrap_or_else(|error| panic!("working directory: {error}"));
        if let Err(error) = submit_provider_create(
            &self.client,
            ConversationCreateRequest {
                work_roots: Vec::new(),
                settings: Some(ProviderRequestedSettings {
                    mode: Some("auto".into()),
                    ..ProviderRequestedSettings::default()
                }),
                operation_id: operation_id.clone(),
                endpoint: self.endpoint.clone(),
                generation: None,
                working_directory,
                created_by: approver.clone(),
                approver: approver.clone(),
                requested_policy: ProviderRequestedPolicy {
                    access: RouterAccess::WriteRestricted,
                },
            },
        )
        .await
        {
            panic!("create admitted: {error}");
        }
        match self.wait(&operation_id).await {
            Ok(ConversationOperationWaitResult {
                output:
                    ConversationOperationWaitOutput::Available {
                        settlement: ConversationOperationSettlement::Created { target, .. },
                    },
                ..
            }) => target,
            other => panic!("expected a created session: {other:?}"),
        }
    }

    /// Loads `target` on this Host, so a prompt sent to it would reach the agent.
    pub(super) async fn load(
        &self,
        target: &SessionRef,
        owner: &ProviderIdentity,
    ) -> Result<ConversationOperationResult, ConversationClientError> {
        let working_directory =
            ProviderWorkingDirectory::try_from(self.root.path().display().to_string())
                .unwrap_or_else(|error| panic!("working directory: {error}"));
        submit_provider_load(
            &self.client,
            ConversationLoadRequest {
                operation_id: OperationId::generate(),
                target: target.clone(),
                generation: None,
                working_directory,
                requested_by: owner.clone(),
                approver: owner.clone(),
                requested_policy: ProviderRequestedPolicy {
                    access: RouterAccess::WriteRestricted,
                },
            },
        )
        .await
    }

    /// One API prompt as `approver`, in the background, answered after `wait`.
    pub(super) fn spawn_prompt(
        &self,
        target: &SessionRef,
        approver: &ProviderIdentity,
        operation_id: OperationId,
        text: &str,
        wait: Duration,
    ) -> PromptCall {
        self.spawn_prompt_as(
            target,
            (approver, approver),
            operation_id,
            text,
            wait,
            MessageDelivery::Auto,
        )
    }

    /// One API prompt by `requester` naming `approver`, in the background,
    /// with the caller's delivery choice.
    pub(super) fn spawn_prompt_as(
        &self,
        target: &SessionRef,
        (requester, approver): (&ProviderIdentity, &ProviderIdentity),
        operation_id: OperationId,
        text: &str,
        wait: Duration,
        delivery: MessageDelivery,
    ) -> PromptCall {
        let client = self.client.clone();
        let target = target.clone();
        let requester = requester.clone();
        let approver = approver.clone();
        let text = text.to_owned();
        tokio::spawn(async move {
            let conversation = ConversationClient::connect(
                &CollaborationAccess::api(client.directory()),
                &target.endpoint,
            )
            .await?;
            let actor = ConversationActor::Typed(
                requester
                    .to_board_identity()
                    .map_err(ConversationClientError::InvalidInput)?,
            );
            let message = serde_json::from_value(json!({"kind":"humanUser","text":text}))
                .map_err(|_| ConversationClientError::InvalidInput("prompt content"))?;
            let input = ConversationPromptInput {
                delivery,
                operation_id: Some(operation_id),
                target,
                working_directory: None,
                requested_by: actor,
                approver: Some(approver),
                message,
                effort: None,
                generation: None,
            };
            conversation
                .prompt(input, wait, CancellationToken::new())
                .await
        })
    }

    /// One API prompt by `requester` naming `approver`, answered after `wait`.
    pub(super) async fn prompt_as(
        &self,
        target: &SessionRef,
        requester: &ProviderIdentity,
        approver: &ProviderIdentity,
        operation_id: OperationId,
        text: &str,
        wait: Duration,
    ) -> Result<ConversationOperationResult, ConversationClientError> {
        self.spawn_prompt_as(
            target,
            (requester, approver),
            operation_id,
            text,
            wait,
            MessageDelivery::Auto,
        )
        .await
        .unwrap_or_else(|error| panic!("prompt task: {error}"))
    }

    /// One API prompt as `approver` that asks to queue (D1 R12), answered
    /// after `wait`.
    pub(super) async fn queue(
        &self,
        target: &SessionRef,
        approver: &ProviderIdentity,
        operation_id: OperationId,
        text: &str,
        wait: Duration,
    ) -> Result<ConversationOperationResult, ConversationClientError> {
        self.spawn_prompt_as(
            target,
            (approver, approver),
            operation_id,
            text,
            wait,
            MessageDelivery::Queue,
        )
        .await
        .unwrap_or_else(|error| panic!("queue task: {error}"))
    }

    /// One API prompt as `approver`, answered after `wait`.
    pub(super) async fn prompt(
        &self,
        target: &SessionRef,
        approver: &ProviderIdentity,
        operation_id: OperationId,
        text: &str,
        wait: Duration,
    ) -> Result<ConversationOperationResult, ConversationClientError> {
        self.spawn_prompt(target, approver, operation_id, text, wait)
            .await
            .unwrap_or_else(|error| panic!("prompt task: {error}"))
    }

    /// Queues one prompt per text, in order: each is sent only once the one
    /// before it shows as waiting in the session's line. Returns their IDs.
    pub(super) async fn queue_in_order(
        &self,
        target: &SessionRef,
        approver: &ProviderIdentity,
        texts: &[String],
    ) -> Vec<OperationId> {
        self.queue_in_order_as(target, approver, approver, texts)
            .await
    }

    /// As `queue_in_order`, by `requester` naming `approver`.
    pub(super) async fn queue_in_order_as(
        &self,
        target: &SessionRef,
        requester: &ProviderIdentity,
        approver: &ProviderIdentity,
        texts: &[String],
    ) -> Vec<OperationId> {
        let mut in_flight = std::collections::VecDeque::new();
        let mut queued = Vec::with_capacity(texts.len());
        for text in texts {
            if in_flight.len() >= PROMPTS_IN_FLIGHT
                && let Some(call) = in_flight.pop_front()
            {
                expect_queued(call).await;
            }
            let operation_id = OperationId::generate();
            in_flight.push_back(self.spawn_prompt_as(
                target,
                (requester, approver),
                operation_id.clone(),
                text,
                SHORT_WAIT,
                MessageDelivery::Queue,
            ));
            self.until_router_queued(&operation_id).await;
            queued.push(operation_id);
        }
        for call in in_flight {
            expect_queued(call).await;
        }
        queued
    }

    pub(super) async fn show(
        &self,
        operation_id: &OperationId,
    ) -> Result<ConversationOperationSnapshot, ClientError> {
        self.client
            .show_provider_conversation_operation(ConversationOperationShowRequest {
                operation_id: operation_id.clone(),
            })
            .await
    }

    /// Loops until show says the operation waits in the session's line.
    pub(super) async fn until_router_queued(&self, operation_id: &OperationId) {
        loop {
            if let Ok(snapshot) = self.show(operation_id).await
                && snapshot.queue_state == Some(ConversationOperationQueueState::RouterQueued)
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    }

    /// One bounded wait on the Host: the operation's state after at most `seconds`.
    pub(super) async fn wait_once(
        &self,
        operation_id: &OperationId,
        seconds: u32,
    ) -> Result<ConversationOperationWaitResult, ClientError> {
        let timeout_seconds = PositiveSeconds::try_from(seconds)
            .unwrap_or_else(|error| panic!("wait seconds: {error}"));
        self.client
            .wait_for_provider_conversation_operation(ConversationOperationWaitRequest {
                operation_id: operation_id.clone(),
                timeout_seconds,
            })
            .await
    }

    /// Waits on the Host until the operation settles.
    pub(super) async fn wait(
        &self,
        operation_id: &OperationId,
    ) -> Result<ConversationOperationWaitResult, ClientError> {
        let timeout_seconds = PositiveSeconds::try_from(SETTLE_SECONDS)
            .unwrap_or_else(|error| panic!("wait seconds: {error}"));
        loop {
            let waited = self
                .client
                .wait_for_provider_conversation_operation(ConversationOperationWaitRequest {
                    operation_id: operation_id.clone(),
                    timeout_seconds,
                })
                .await;
            match &waited {
                Ok(result) if matches!(result.output, ConversationOperationWaitOutput::Pending) => {
                }
                _ => return waited,
            }
        }
    }

    pub(super) async fn close(
        &self,
        target: &SessionRef,
        owner: &ProviderIdentity,
        operation_id: OperationId,
    ) -> Result<ConversationOperationSubmission, ClientError> {
        self.client
            .close_provider_conversation(ConversationCloseRequest {
                operation_id,
                target: target.clone(),
                generation: None,
                requested_by: owner.clone(),
                approver: owner.clone(),
            })
            .await
    }

    /// Cancels a queued input on the Router's ACP socket (`_session/queue/cancel`).
    pub(super) async fn queue_cancel(&self, target: &SessionRef, input_id: &str) -> Value {
        let stream = UnixStream::connect(self.root.path().join("codex-acp.sock"))
            .await
            .unwrap_or_else(|error| panic!("ACP socket: {error}"));
        let (read, mut write) = stream.into_split();
        let mut lines = BufReader::new(read).lines();
        let session_ref = serde_json::to_value(target)
            .unwrap_or_else(|error| panic!("session reference: {error}"));
        let frames = [
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
                "protocolVersion":1,"clientCapabilities":{},"_meta":{
                    "router":{"actor":{"kind":"human","humanId":"queue-owner"}},
                    "sessionProfile":{"version":1,"elements":[]}
                }
            }}),
            json!({"jsonrpc":"2.0","id":2,"method":"_session/queue/cancel","params":{
                "sessionId":String::from(target.session_id.clone()),"inputId":input_id,
                "_meta":{"router":{"sessionRef":session_ref}}
            }}),
        ];
        let mut last = Value::Null;
        for frame in frames {
            if let Err(error) = write.write_all(format!("{frame}\n").as_bytes()).await {
                panic!("ACP request: {error}");
            }
            last = next_response(&mut lines).await;
        }
        last
    }
}

async fn expect_queued(call: PromptCall) {
    match call.await {
        Ok(Ok(ConversationOperationResult::Queued { .. })) => {}
        other => panic!("a prompt to a busy session is queued: {other:?}"),
    }
}

/// The next response frame on an ACP socket, skipping notifications.
async fn next_response(
    lines: &mut tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
) -> Value {
    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            other => panic!("ACP socket stays open: {other:?}"),
        };
        let frame: Value =
            serde_json::from_str(&line).unwrap_or_else(|error| panic!("ACP frame: {error}"));
        if frame.get("result").is_some() || frame.get("error").is_some() {
            return frame;
        }
    }
}

/// Every ACP request the fixture recorded.
fn recorded_requests(receipt: &Path) -> Vec<Value> {
    std::fs::read(receipt)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<Value>>(&bytes).ok())
        .unwrap_or_default()
}

/// How many requests with `method` the fixture recorded.
pub(super) fn recorded(receipt: &Path, method: &str) -> usize {
    recorded_requests(receipt)
        .iter()
        .filter(|request| request.get("method").and_then(Value::as_str) == Some(method))
        .count()
}

/// The text of every prompt the fixture received, in the order it received them.
pub(super) fn prompt_texts(receipt: &Path) -> Vec<String> {
    recorded_requests(receipt)
        .iter()
        .filter(|request| request.get("method").and_then(Value::as_str) == Some("session/prompt"))
        .map(|request| {
            request
                .pointer("/params/prompt/0/text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

/// Loops until the fixture recorded `count` requests with `method`.
pub(super) async fn until_recorded(receipt: &Path, method: &str, count: usize) {
    while recorded(receipt, method) < count {
        tokio::task::yield_now().await;
    }
}

/// Ends the fixture's held first turn.
pub(super) fn release(receipt: &Path) {
    if let Err(error) = node_test_fixture::release_fixture(receipt, ".release") {
        panic!("release the held turn: {error}");
    }
}

/// The failure an operation wait or prompt reported, as the Host shaped it.
pub(super) fn conversation_failure(
    error: &ClientError,
) -> collaboration_protocol::ConversationOperationFailure {
    error
        .conversation_failure()
        .unwrap_or_else(|| panic!("a typed conversation failure: {error:?}"))
}
