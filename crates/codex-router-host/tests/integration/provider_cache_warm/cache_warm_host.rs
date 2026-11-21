//! A real Host started through startup composition with its one ManualClock,
//! whose Claude endpoint runs the send-race fixture (`provider-steering-agent.ts`)
//! with cache warm on at its default (55 minutes, 8 hours). The fixture records
//! every frame it reads, so a test counts warm prompts per session, steers,
//! loads and cancels from what the agent actually received.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "A Host or fixture that cannot start or answer must fail its test."
)]

use codex_router_host::{
    CacheWarmSetting, CollaborationRuntime, CollaborationRuntimeInputs,
    ExternalProviderLaunchBinding, ExternalProviderStartup,
};
use collaboration_client::{
    ClientError, CollaborationAccess, CollaborationClient, ConversationActor, ConversationClient,
    ConversationClientError, ConversationOperationResult, ConversationPromptInput,
    ConversationSettlementDetail,
};
use collaboration_protocol::{
    ConversationCreateRequest, ConversationLoadRequest, ConversationOperationSettlement,
    ConversationOperationShowRequest, ConversationOperationSnapshot,
    ConversationOperationWaitOutput, ConversationOperationWaitRequest, EndpointRef, OperationId,
    PositiveSeconds, ProviderIdentity, ProviderRequestedPolicy, ProviderSettingName,
    ProviderSettingsResult, ProviderSettingsSetRequest, ProviderWorkingDirectory, RouterAccess,
    SessionId, SessionRef,
};
use serde_json::Value;
use sqlx::{Connection as _, SqliteConnection, sqlite::SqliteConnectOptions};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::provider_conversation_submission::{submit_provider_create, submit_provider_load};
use router_clock::{ManualClock, RuntimeClock as _};
use std::sync::Arc;

/// The text of every warm prompt the Host sends.
pub(super) const WARM_PROMPT: &str = "Cache warm: reply ok";
/// The default interval between warm prompts.
pub(super) const FIFTY_FIVE_MINUTES: Duration = Duration::from_secs(55 * 60);
/// One minute: staggers turn ends so each session's timer has its own deadline.
pub(super) const ONE_MINUTE: Duration = Duration::from_secs(60);

/// How long a prompt call waits for its settlement before answering with what
/// it knows: a product argument. The test runner bounds each case
/// (`.config/nextest.toml`); test code never waits on a clock.
const SETTLE_WAIT: Duration = Duration::from_secs(20);

pub(super) type PromptAnswer = Result<ConversationOperationResult, ConversationClientError>;

/// How one prompt call ended, as its caller saw it.
#[derive(Debug, Eq, PartialEq)]
pub(super) enum SendEnd {
    Prompted,
    Steered,
    Other(String),
}

pub(super) fn send_end(answer: &PromptAnswer) -> SendEnd {
    match answer {
        Ok(ConversationOperationResult::Completed { settlement, .. }) => match settlement.detail {
            ConversationSettlementDetail::ProviderPrompt { .. } => SendEnd::Prompted,
            ConversationSettlementDetail::ProviderSteered { .. } => SendEnd::Steered,
            ref other => SendEnd::Other(format!("{other:?}")),
        },
        other => SendEnd::Other(format!("{other:?}")),
    }
}

/// Every frame the fixture read, in order; empty while the file is being written.
fn frames(receipt: &Path) -> Vec<Value> {
    std::fs::read(receipt)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<Value>>(&bytes).ok())
        .unwrap_or_default()
}

fn method_is(frame: &Value, method: &str) -> bool {
    frame.get("method").and_then(Value::as_str) == Some(method)
}

fn session_is(frame: &Value, session: &SessionRef) -> bool {
    frame.pointer("/params/sessionId").and_then(Value::as_str)
        == Some(String::from(session.session_id.clone()).as_str())
}

/// Whether any prompt text block of the frame is `text`.
fn carries_text(frame: &Value, text: &str) -> bool {
    frame
        .pointer("/params/prompt")
        .and_then(Value::as_array)
        .is_some_and(|parts| {
            parts
                .iter()
                .any(|part| part.get("text").and_then(Value::as_str) == Some(text))
        })
}

/// The receipt positions of the frames `matches` selects.
fn positions(receipt: &Path, matches: impl Fn(&Value) -> bool) -> Vec<usize> {
    frames(receipt)
        .iter()
        .enumerate()
        .filter(|(_, frame)| matches(frame))
        .map(|(index, _)| index)
        .collect()
}

/// The receipt positions of the warm prompts the agent received for `session`.
pub(super) fn warm_prompt_positions(receipt: &Path, session: &SessionRef) -> Vec<usize> {
    positions(receipt, |frame| {
        method_is(frame, "session/prompt")
            && session_is(frame, session)
            && carries_text(frame, WARM_PROMPT)
    })
}

pub(super) fn warm_prompts_to(receipt: &Path, session: &SessionRef) -> usize {
    warm_prompt_positions(receipt, session).len()
}

/// Every warm prompt the agent received, to any session.
pub(super) fn warm_prompts(receipt: &Path) -> usize {
    positions(receipt, |frame| {
        method_is(frame, "session/prompt") && carries_text(frame, WARM_PROMPT)
    })
    .len()
}

/// The receipt positions of the `method` frames for `session`.
pub(super) fn method_positions(receipt: &Path, method: &str, session: &SessionRef) -> Vec<usize> {
    positions(receipt, |frame| {
        method_is(frame, method) && session_is(frame, session)
    })
}

/// Every frame with `method`, to any session.
pub(super) fn recorded(receipt: &Path, method: &str) -> usize {
    positions(receipt, |frame| method_is(frame, method)).len()
}

/// The `method` frames whose prompt text is `text`.
pub(super) fn carrying(receipt: &Path, method: &str, text: &str) -> usize {
    positions(receipt, |frame| {
        method_is(frame, method) && carries_text(frame, text)
    })
    .len()
}

/// Yields until `condition` holds; the runner bounds the case.
pub(super) async fn until(mut condition: impl FnMut() -> bool) {
    while !condition() {
        tokio::task::yield_now().await;
    }
}

pub(super) fn human(id: &str) -> ProviderIdentity {
    serde_json::from_value(serde_json::json!({"kind":"human","humanId":id})).expect("human")
}

/// A real Host whose Claude endpoint is the send-race fixture with cache warm on.
pub(super) struct CacheWarmHost {
    root: tempfile::TempDir,
    pub(super) receipt: PathBuf,
    runtime: CollaborationRuntime,
    clock: Arc<ManualClock>,
    client: CollaborationClient,
    endpoint: EndpointRef,
    owner: ProviderIdentity,
}

impl CacheWarmHost {
    /// Starts the Host through startup composition; `behaviours` are the
    /// fixture's send-race behaviours.
    pub(super) async fn start(behaviours: &[&str]) -> Self {
        // A short root under /tmp keeps the Host's socket paths inside the Unix limit.
        let root = tempfile::Builder::new()
            .prefix("cache-warm-")
            .tempdir_in("/tmp")
            .expect("runtime root");
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
            .expect("private runtime root");
        let receipt = root.path().join("wire.json");
        let mut arguments = vec![
            node_test_fixture::fixture_program("provider-steering-agent.ts")
                .to_string_lossy()
                .into_owned(),
            "send-race".to_owned(),
            receipt.display().to_string(),
        ];
        arguments.extend(behaviours.iter().map(|behaviour| (*behaviour).to_owned()));
        let clock = Arc::new(ManualClock::new(chrono::DateTime::UNIX_EPOCH));
        let mut binding = ExternalProviderLaunchBinding::claude(
            node_test_fixture::node_executable().expect("Node"),
            arguments,
        )
        .expect("Claude binding")
        .with_cache_warm(CacheWarmSetting::default());
        codex_router_test_support::global_claude_fixture::declare_global_claude_fixture(
            root.path(),
            &mut binding.launch.environment,
        )
        .expect("declare test-owned global Claude executable");
        let runtime = Box::pin(
            CollaborationRuntime::runtime_test_start_with_provider_clock(
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
                clock.clone(),
            ),
        )
        .await
        .expect("Host starts through startup composition");
        let client = CollaborationClient::connect(root.path(), "cache-warm", "1")
            .await
            .expect("collaboration API client");
        let endpoint = client
            .list_endpoints()
            .await
            .expect("endpoint inventory")
            .endpoints
            .into_iter()
            .find(|endpoint| String::from(endpoint.endpoint.endpoint_id.clone()) == "claude-local")
            .expect("Claude endpoint")
            .endpoint;
        Self {
            root,
            receipt,
            runtime,
            clock,
            client,
            endpoint,
            owner: human("cache-warm-owner"),
        }
    }

    /// Lets the fixture end what it holds behind `<receipt>.<name>`.
    pub(super) fn release(&self, name: &str) {
        node_test_fixture::release_fixture(&self.receipt, &format!(".{name}"))
            .expect("release file");
    }

    /// With hold-each-prompt: lets prompts 1 to `last` end at once, except `held`.
    pub(super) fn release_prompts_except(&self, held: &[usize], last: usize) {
        for prompt in (1..=last).filter(|prompt| !held.contains(prompt)) {
            self.release(&format!("release-{prompt}"));
        }
    }

    pub(super) fn advance(&self, by: Duration) {
        self.clock.advance(by).expect("Host clock step");
    }

    /// A session's warm timer was armed at the current instant: a sleep 55
    /// minutes ahead was checked and was not due.
    pub(super) async fn until_warm_timer_armed(&self) {
        // Says what a timed-out case was waiting for.
        eprintln!("waiting for a warm timer armed at the current instant");
        let deadline = self
            .clock
            .monotonic_now()
            .checked_add(FIFTY_FIVE_MINUTES)
            .expect("cache warm deadline");
        self.clock.not_due_check_now(deadline).await;
    }

    pub(super) async fn until_warm_prompts_to(&self, session: &SessionRef, expected: usize) {
        eprintln!(
            "waiting for warm prompt {expected} to {}",
            String::from(session.session_id.clone())
        );
        until(|| warm_prompts_to(&self.receipt, session) >= expected).await;
    }

    /// The supervisor records the held session's warm attempt even when ACP
    /// refuses it before the wire. Its terminal row is the dispatch barrier.
    pub(super) async fn until_terminal_prompts_to(&self, session: &SessionRef, expected: i64) {
        let session_id = String::from(session.session_id.clone());
        let mut store = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(self.root.path().join("provider-operations.sqlite"))
                .read_only(true),
        )
        .await
        .expect("open operation receipts");
        loop {
            let count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM provider_operations WHERE target_session_id = ? AND operation_kind = 'conversationPrompt' AND stage = 'terminal'",
            )
            .bind(&session_id)
            .fetch_one(&mut store)
            .await
            .expect("count terminal prompt operations");
            if count >= expected {
                return;
            }
            tokio::task::yield_now().await;
        }
    }

    pub(super) fn working_directory(&self) -> ProviderWorkingDirectory {
        ProviderWorkingDirectory::try_from(self.root.path().display().to_string())
            .expect("working directory")
    }

    /// Creates a Claude session through the API, after the Host started.
    pub(super) async fn create(&self) -> SessionRef {
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
                working_directory: self.working_directory(),
                created_by: self.owner.clone(),
                approver: self.owner.clone(),
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

    /// Loads `session_id`, a session the agent already has, through the API.
    pub(super) async fn load(&self, session_id: &str) -> (SessionRef, PromptAnswer) {
        let target = SessionRef {
            endpoint: self.endpoint.clone(),
            session_id: SessionId::try_from(session_id.to_owned()).expect("session ID"),
        };
        let answer = submit_provider_load(
            &self.client,
            ConversationLoadRequest {
                operation_id: OperationId::generate(),
                target: target.clone(),
                generation: None,
                working_directory: self.working_directory(),
                requested_by: self.owner.clone(),
                approver: self.owner.clone(),
                requested_policy: ProviderRequestedPolicy {
                    access: RouterAccess::WriteRestricted,
                },
            },
        )
        .await;
        (target, answer)
    }

    /// One API prompt from the owner, on its own task.
    pub(super) fn spawn_prompt(
        &self,
        target: &SessionRef,
        operation_id: &OperationId,
        text: &str,
    ) -> JoinHandle<PromptAnswer> {
        let directory = self.client.directory().to_owned();
        let input = ConversationPromptInput {
            delivery: collaboration_protocol::MessageDelivery::Auto,
            operation_id: Some(operation_id.clone()),
            target: target.clone(),
            working_directory: None,
            requested_by: ConversationActor::Typed(
                self.owner.to_board_identity().expect("owner actor"),
            ),
            approver: Some(self.owner.clone()),
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

    /// One API prompt from the owner, answered once it settles or is steered.
    pub(super) async fn prompt(&self, target: &SessionRef, text: &str) -> PromptAnswer {
        self.spawn_prompt(target, &OperationId::generate(), text)
            .await
            .expect("prompt task")
    }

    /// The operation's snapshot, once the Host admitted it.
    pub(super) async fn show(
        &self,
        operation_id: &OperationId,
    ) -> Option<ConversationOperationSnapshot> {
        self.client
            .show_provider_conversation_operation(ConversationOperationShowRequest {
                operation_id: operation_id.clone(),
            })
            .await
            .ok()
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

    /// The owner changes the session's effort.
    pub(super) async fn set_effort(
        &self,
        target: &SessionRef,
        value: &str,
    ) -> Result<ProviderSettingsResult, ClientError> {
        self.client
            .set_provider_conversation_setting(ProviderSettingsSetRequest {
                target: target.clone(),
                actor: self.owner.clone(),
                setting: ProviderSettingName::Effort,
                value: value.to_owned(),
            })
            .await
    }

    /// Shuts the Host down, keeping its folder (and the fixture's receipt) and
    /// the clock control the test held.
    pub(super) async fn shutdown(self) -> StoppedCacheWarmHost {
        let Self {
            root,
            receipt,
            runtime,
            clock,
            ..
        } = self;
        let stopped = runtime
            .shutdown()
            .await
            .map_err(|error| format!("{error:?}"));
        StoppedCacheWarmHost {
            _root: root,
            receipt,
            clock,
            stopped,
        }
    }
}

/// A stopped Host's folder, its receipt, the clock control the test held, and
/// how its shutdown went.
pub(super) struct StoppedCacheWarmHost {
    _root: tempfile::TempDir,
    pub(super) receipt: PathBuf,
    clock: Arc<ManualClock>,
    stopped: Result<(), String>,
}

impl StoppedCacheWarmHost {
    pub(super) fn assert_stopped(&self) {
        assert!(self.stopped.is_ok(), "Host shutdown: {:?}", self.stopped);
    }

    pub(super) fn advance_after_stop(&self, by: Duration) {
        assert_eq!(
            Arc::strong_count(&self.clock),
            1,
            "the stopped Host released its clock"
        );
        self.clock.advance(by).expect("advance after Host shutdown");
    }
}
