//! One real Host with one ACP agent fixture, driven as a client of the
//! Router's ACP socket (`codex-acp.sock`), for the busy `session/prompt`
//! placements (D1 R12, V22b). Every wait yields to the runtime without
//! reading a clock; the nextest override for `provider_acp_busy_prompt::`
//! bounds each case.

#![expect(
    clippy::panic,
    reason = "A Host or fixture that cannot start or answer must fail its test."
)]

use codex_router_host::{
    CollaborationRuntime, CollaborationRuntimeInputs, ExternalProviderLaunchBinding,
    ExternalProviderStartup,
};
use collaboration_client::CollaborationClient;
use collaboration_contracts::SessionEventAttachment;
use collaboration_protocol::SessionRef;
use serde_json::{Value, json};
use session_event_model::{SessionEvent, StopReason, TurnOutcome};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::UnixStream;

/// The human the ACP connection acts for; the Host's owner.
const OWNER: &str = "acp-owner";

/// The agent behind the Router's ACP socket.
pub(super) enum BusyAgent<'behaviours> {
    /// `provider-steering-agent.ts send-race`, an agent that steers unless
    /// told `no-steering`, served as the Claude endpoint.
    SendRace(&'behaviours [&'behaviours str]),
    /// `cursor-settings-agent.ts reactive`, an agent that cannot steer unless
    /// told `advertise-steering`, served as the Cursor endpoint.
    Cursor(&'behaviours [&'behaviours str]),
}

impl BusyAgent<'_> {
    fn endpoint_id(&self) -> &'static str {
        match self {
            Self::SendRace(_) => "claude-local",
            Self::Cursor(_) => "cursor-local",
        }
    }

    fn binding(&self, root: &Path, receipt: &Path) -> ExternalProviderLaunchBinding {
        let node =
            node_test_fixture::node_executable().unwrap_or_else(|error| panic!("Node: {error}"));
        let (program, scenario, behaviours) = match self {
            Self::SendRace(behaviours) => ("provider-steering-agent.ts", "send-race", behaviours),
            Self::Cursor(behaviours) => ("cursor-settings-agent.ts", "reactive", behaviours),
        };
        let mut arguments = vec![
            node_test_fixture::fixture_program(program)
                .to_string_lossy()
                .into_owned(),
            scenario.to_owned(),
            receipt.display().to_string(),
        ];
        arguments.extend(behaviours.iter().map(|behaviour| (*behaviour).to_owned()));
        match self {
            Self::SendRace(_) => {
                let mut binding = ExternalProviderLaunchBinding::claude(node, arguments)
                    .unwrap_or_else(|error| panic!("Claude binding: {error}"));
                codex_router_test_support::global_claude_fixture::declare_global_claude_fixture(
                    root,
                    &mut binding.launch.environment,
                )
                .unwrap_or_else(|error| panic!("declare test-owned global Claude: {error}"));
                binding
            }
            Self::Cursor(_) => ExternalProviderLaunchBinding::cursor(node, arguments)
                .unwrap_or_else(|error| panic!("Cursor binding: {error}")),
        }
    }
}

/// One client connection to the Router's ACP socket.
struct AcpConnection {
    write: tokio::net::unix::OwnedWriteHalf,
    lines: tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
    /// Responses read while the test waited for another one.
    unclaimed: Vec<Value>,
    next_request: u64,
}

/// A Host, one ACP client connection to it, and one session that connection created.
pub(super) struct AcpBusyHost {
    root: tempfile::TempDir,
    /// Every frame the agent read, as the fixture recorded it.
    pub(super) receipt: PathBuf,
    runtime: CollaborationRuntime,
    acp: AcpConnection,
    session_id: String,
    session: SessionRef,
}

/// The frames the agent read with `method`, and with prompt text `text` when one is named.
pub(super) fn agent_frames(receipt: &Path, method: &str, text: Option<&str>) -> usize {
    std::fs::read(receipt)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<Value>>(&bytes).ok())
        .unwrap_or_default()
        .iter()
        .filter(|frame| frame.get("method").and_then(Value::as_str) == Some(method))
        .filter(|frame| {
            text.is_none_or(|text| {
                frame
                    .pointer("/params/prompt/0/text")
                    .and_then(Value::as_str)
                    == Some(text)
            })
        })
        .count()
}

/// Yields until the agent has read `count` frames with `method`.
pub(super) async fn until_agent_read(receipt: &Path, method: &str, count: usize) {
    while agent_frames(receipt, method, None) < count {
        tokio::task::yield_now().await;
    }
}

impl AcpConnection {
    async fn connect(socket: &Path) -> Self {
        let stream = UnixStream::connect(socket)
            .await
            .unwrap_or_else(|error| panic!("ACP socket: {error}"));
        let (read, write) = stream.into_split();
        Self {
            write,
            lines: BufReader::new(read).lines(),
            unclaimed: Vec::new(),
            next_request: 100,
        }
    }

    async fn send(&mut self, frame: &Value) {
        self.write
            .write_all(format!("{frame}\n").as_bytes())
            .await
            .unwrap_or_else(|error| panic!("ACP request: {error}"));
    }

    async fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_request += 1;
        let id = self.next_request;
        self.send(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await;
        self.response(id).await
    }

    async fn response(&mut self, id: u64) -> Value {
        if let Some(index) = self
            .unclaimed
            .iter()
            .position(|frame| frame.get("id") == Some(&json!(id)))
        {
            return self.unclaimed.remove(index);
        }
        loop {
            let frame = self.next_response().await;
            if frame.get("id") == Some(&json!(id)) {
                return frame;
            }
            self.unclaimed.push(frame);
        }
    }

    async fn next_response(&mut self) -> Value {
        loop {
            let line = self
                .lines
                .next_line()
                .await
                .unwrap_or_else(|error| panic!("ACP socket read: {error}"))
                .unwrap_or_else(|| panic!("ACP socket closed"));
            let frame: Value = serde_json::from_str(&line)
                .unwrap_or_else(|error| panic!("ACP frame {line}: {error}"));
            if frame.get("method").is_none()
                && (frame.get("result").is_some() || frame.get("error").is_some())
            {
                return frame;
            }
        }
    }
}

impl AcpBusyHost {
    pub(super) async fn start(agent: &BusyAgent<'_>) -> Self {
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("runtime root: {error}"));
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
            .unwrap_or_else(|error| panic!("private runtime root: {error}"));
        let receipt = root.path().join("wire.json");
        let binding = agent.binding(root.path(), &receipt);
        let owner = message_board::HumanId::try_from(OWNER.to_owned())
            .unwrap_or_else(|error| panic!("owner: {error}"));
        let runtime = Box::pin(
            CollaborationRuntime::runtime_test_start_with_provider_clock(
                CollaborationRuntimeInputs {
                    owner_human_id: Some(owner),
                    directory: root.path().to_owned(),
                    codex_home: root.path().to_owned(),
                    backend_socket: root.path().join("backend.sock"),
                    mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
                    native_schema: None,
                    peer_registry_directory: None,
                    remote_control_server_name: None,
                },
                vec![ExternalProviderStartup::Launch(binding)],
                std::sync::Arc::new(router_clock::ManualClock::new(chrono::DateTime::UNIX_EPOCH)),
            ),
        )
        .await
        .unwrap_or_else(|error| panic!("Host starts the provider face: {error}"));
        let client = CollaborationClient::connect(root.path(), "acp-busy-prompt", "1")
            .await
            .unwrap_or_else(|error| panic!("collaboration API client: {error}"));
        let inventory = client
            .list_endpoints()
            .await
            .unwrap_or_else(|error| panic!("endpoint inventory: {error}"));
        assert!(
            inventory.endpoints.iter().any(|endpoint| {
                String::from(endpoint.endpoint.endpoint_id.clone()) == agent.endpoint_id()
                    && matches!(
                        endpoint.availability,
                        collaboration_protocol::EndpointAvailability::Available { .. }
                    )
            }),
            "provider endpoint unavailable: {inventory:?}"
        );
        drop(client);
        let mut acp = AcpConnection::connect(&root.path().join("codex-acp.sock")).await;
        let initialized = acp
            .request(
                "initialize",
                json!({"protocolVersion":1,"clientCapabilities":{},"_meta":{
                    "router":{"actor":{"kind":"human","humanId":OWNER}},
                    "sessionProfile":{"version":1,"elements":[]}
                }}),
            )
            .await;
        assert!(
            initialized.get("error").is_none(),
            "initialize: {initialized}"
        );
        let created = acp
            .request(
                "session/new",
                json!({"cwd":root.path(),"mcpServers":[],
                    "_meta":{"router":{"endpoint":agent.endpoint_id()}}}),
            )
            .await;
        let (Some(session_id), Some(session)) = (
            created.pointer("/result/sessionId").and_then(Value::as_str),
            created.pointer("/result/_meta/router/sessionRef"),
        ) else {
            panic!("session/new creates a session: {created}");
        };
        let session_id = session_id.to_owned();
        let session = serde_json::from_value(session.clone())
            .unwrap_or_else(|error| panic!("session reference {session}: {error}"));
        Self {
            root,
            receipt,
            runtime,
            acp,
            session_id,
            session,
        }
    }

    /// Sends `session/prompt` with request ID `id` and one text block.
    pub(super) async fn send_prompt(&mut self, id: u64, text: &str) {
        let frame = json!({"jsonrpc":"2.0","id":id,"method":"session/prompt","params":{
            "sessionId":self.session_id,"prompt":[{"type":"text","text":text}]
        }});
        self.acp.send(&frame).await;
    }

    /// Sends a request with a fresh ID and returns its response.
    pub(super) async fn request(&mut self, method: &str, params: Value) -> Value {
        self.acp.request(method, params).await
    }

    /// The response to request `id`; other responses read meanwhile wait for
    /// their own callers, and notifications are skipped.
    pub(super) async fn response(&mut self, id: u64) -> Value {
        self.acp.response(id).await
    }

    /// Whether a response to `id` has been read, without waiting for one.
    pub(super) fn has_answered(&self, id: u64) -> bool {
        self.acp
            .unclaimed
            .iter()
            .any(|frame| frame.get("id") == Some(&json!(id)))
    }

    /// The session's Router line, from `_session/queue/list`.
    pub(super) async fn queue_list(&mut self) -> Vec<Value> {
        let params = json!({"sessionId":self.session_id});
        let listed = self.request("_session/queue/list", params).await;
        listed
            .pointer("/result/items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_else(|| panic!("queue list: {listed}"))
    }

    /// Yields until the session's line holds `count` inputs, or until prompt
    /// `id` has been answered instead, and returns the line as last listed.
    pub(super) async fn until_queued_or_answered(&mut self, count: usize, id: u64) -> Vec<Value> {
        loop {
            let items = self.queue_list().await;
            if items.len() >= count || self.has_answered(id) {
                return items;
            }
            tokio::task::yield_now().await;
        }
    }

    /// Cancels the waiting input `input_id` with `_session/queue/cancel`.
    pub(super) async fn queue_cancel(&mut self, input_id: &str) -> Value {
        let params = json!({"sessionId":self.session_id,"inputId":input_id});
        self.request("_session/queue/cancel", params).await
    }

    /// Creates the file `<receipt>.<name>` the fixture waits for.
    pub(super) fn release(&self, name: &str) {
        node_test_fixture::release_fixture(&self.receipt, &format!(".{name}"))
            .unwrap_or_else(|error| panic!("release {name}: {error}"));
    }

    pub(super) async fn attach_events(&self) -> SessionEventAttachment {
        let target = serde_json::from_value(json!(&self.session))
            .unwrap_or_else(|error| panic!("hub session: {error}"));
        self.runtime
            .attach_provider_events_for_test(target)
            .await
            .unwrap_or_else(|error| panic!("attach to provider events: {error}"))
    }

    /// Snapshot after the ACP reply has witnessed terminal publication.
    pub(super) async fn hub_events(&self) -> Vec<Value> {
        self.attach_events()
            .await
            .snapshot
            .into_iter()
            .map(|record| json!({"sequence":record.sequence, "event":record.event}))
            .collect()
    }

    /// Asks the Host to close the session through the collaboration API.
    pub(super) async fn close(
        &self,
    ) -> Result<
        collaboration_protocol::ConversationOperationSubmission,
        collaboration_client::ClientError,
    > {
        let owner: collaboration_protocol::ProviderIdentity =
            serde_json::from_value(json!({"kind":"human","humanId":OWNER}))
                .unwrap_or_else(|error| panic!("owner identity: {error}"));
        let client = CollaborationClient::connect(self.root.path(), "acp-busy-prompt", "1")
            .await
            .unwrap_or_else(|error| panic!("collaboration API client: {error}"));
        client
            .close_provider_conversation(collaboration_protocol::ConversationCloseRequest {
                operation_id: collaboration_protocol::OperationId::generate(),
                target: self.session.clone(),
                generation: None,
                requested_by: owner.clone(),
                approver: owner,
            })
            .await
    }

    /// Stops the Host; its result is asserted last, after the case's own oracle.
    pub(super) async fn shutdown(self) -> std::io::Result<()> {
        let Self { runtime, acp, .. } = self;
        drop(acp);
        runtime.shutdown().await
    }
}

/// The turns on the Host's hub for one session, from its observed history.
#[derive(Debug, Default)]
pub(super) struct HubTurns {
    /// `(turn ID, input ID)` per started turn, in order.
    pub(super) started: Vec<(String, String)>,
    /// `(turn ID, outcome)` per ended turn, in order.
    pub(super) ended: Vec<(String, TurnOutcome)>,
    /// `(input ID, turn ID)` per input a running turn accepted (a steer).
    pub(super) accepted: Vec<(String, String)>,
}

impl HubTurns {
    /// Reads every `{"sequence","event"}` the provider observation returned.
    pub(super) fn from_observed(events: &[Value]) -> Self {
        let mut turns = Self::default();
        for observed in events {
            let Some(event) = observed
                .get("event")
                .and_then(|event| serde_json::from_value::<SessionEvent>(event.clone()).ok())
            else {
                continue;
            };
            match event {
                SessionEvent::TurnStarted { turn_id, input_id } => {
                    turns.started.push((turn_id, input_id.as_str().to_owned()));
                }
                SessionEvent::TurnEnded { turn_id, outcome } => {
                    turns.ended.push((turn_id, outcome))
                }
                SessionEvent::InputAccepted { input_id, turn_id } => {
                    turns.accepted.push((input_id.as_str().to_owned(), turn_id));
                }
                _ => {}
            }
        }
        turns
    }

    /// The turn that started with `input_id`.
    pub(super) fn started_by(&self, input_id: &str) -> Option<&str> {
        self.started
            .iter()
            .find(|(_, started)| started == input_id)
            .map(|(turn_id, _)| turn_id.as_str())
    }

    /// The stop reason `turn_id` ended with, when it ended with one.
    pub(super) fn stop_reason_of(&self, turn_id: &str) -> Option<&StopReason> {
        self.ended
            .iter()
            .find(|(ended, _)| ended == turn_id)
            .and_then(|(_, outcome)| match outcome {
                TurnOutcome::Ended { stop_reason, .. } => Some(stop_reason),
                TurnOutcome::Lost { .. } => None,
            })
    }
}
