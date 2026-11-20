//! R32 and M7 through the ACP route, real Host command port, and scripted ACP harness.

use super::*;
use codex_acp_adapter::{
    AcpConnectionContext, AcpOutputSender, AcpRouterChannels, AcpSessionRoute as _, QueuedAcpFrame,
    bounded_acp_output,
};
use collaboration_runtime::ProviderAcpSessionRoute;
use router_clock::{ManualClock, RuntimeClock as _};
use serde_json::{Value, json};
use tokio::{sync::mpsc, task::JoinHandle};
use tokio_util::sync::CancellationToken;

struct FailureRoute {
    root: tempfile::TempDir,
    receipt: PathBuf,
    session: BoardSessionRef,
    clock: Arc<ManualClock>,
    supervisor: Arc<ExternalProviderSupervisor>,
    frames: AcpOutputSender,
    replies: mpsc::Receiver<QueuedAcpFrame>,
    closed: CancellationToken,
    route: JoinHandle<std::io::Result<()>>,
}

impl FailureRoute {
    async fn start(
        behavior: Arc<dyn acp_client_runtime::ProviderBehavior>,
        fixture_case: &str,
    ) -> Self {
        let root = tempfile::tempdir().expect("fixture root");
        let receipt = root.path().join("wire.json");
        let clock = Arc::new(ManualClock::new(chrono::DateTime::UNIX_EPOCH));
        let store = Arc::new(Mutex::new(
            ProviderOperationStore::open(&root.path().join("operations.sqlite"))
                .await
                .expect("fixture store"),
        ));
        let hub = Arc::new(ProviderSessionEventHub::new(store.clone()));
        let target = protocol_target("saved-session");
        let binding = provider_binding(&target);
        let actor = ProviderIdentity::Session(target.clone())
            .to_board_identity()
            .expect("board actor");
        let Identity::Session { session } = actor.clone() else {
            panic!("fixture must use a session actor");
        };
        store
            .lock()
            .await
            .record_session(&ProviderSessionRecord {
                target: target.clone(),
                working_directory: ProviderWorkingDirectory::try_from(
                    root.path().display().to_string(),
                )
                .expect("fixture cwd"),
                requested_policy: ProviderRequestedPolicy {
                    access: RouterAccess::WriteRestricted,
                },
                created_by: target.clone().into(),
                approver: target.clone().into(),
                updated_at_ms: 1,
                resume: Default::default(),
            })
            .await
            .expect("saved session");
        let launch = crate::external_provider_runtime::acp_scripted_fixture::launch_fixture_agent(
            "permission-mode-agent.ts",
            vec![
                receipt.display().to_string(),
                "cursor".into(),
                "no-directories".into(),
                "advertise-restore".into(),
                fixture_case.into(),
            ],
        );
        let runtime = ExternalProviderRuntime::initialize_with_mcp_http_and_hub(
            launch,
            acp_client_runtime::ProviderModelPicker::Standard,
            "fixture",
            "http://127.0.0.1:1/mcp",
            hub.clone(),
            session.endpoint.clone(),
            behavior,
            clock.clone(),
        )
        .await
        .expect("fixture runtime");
        let supervisor = Arc::new(
            ExternalProviderSupervisor::new_with_hub(
                vec![ExternalProviderBinding {
                    identity: binding.clone(),
                    runtime,
                }],
                store.clone(),
                Some(hub.clone()),
                None,
            )
            .expect("supervisor"),
        );
        let delivery = Arc::new(ProviderAcpDeliveryRoute::new(
            target.endpoint.service_id.clone(),
            std::iter::once(target.endpoint).collect(),
            endpoint_directory(&binding),
            supervisor.clone(),
            store.clone(),
            Arc::new(NoLivePeer),
            clock.clone(),
        ));
        let port = Arc::new(HostSessionCommandPort::new(
            supervisor.clone(),
            delivery,
            hub.clone(),
            store,
            clock.clone(),
        ));
        let closed = CancellationToken::new();
        let (frames, input) = bounded_acp_output(closed.clone());
        let (output, replies) = bounded_acp_output(closed.clone());
        let route = Box::new(ProviderAcpSessionRoute::new(
            session.endpoint.clone(),
            port,
            hub,
        ));
        let route = tokio::spawn(route.run(
            AcpRouterChannels {
                input,
                output,
                closed: closed.clone(),
            },
            AcpConnectionContext {
                actor: Some(actor),
                client_profile: None,
                client_supports_elicitation_form: false,
            },
        ));
        Self {
            root,
            receipt,
            session,
            clock,
            supervisor,
            frames,
            replies,
            closed,
            route,
        }
    }

    async fn load(&self) {
        self.send(1, "session/load", json!({
            "sessionId":self.session.session_id.as_str(), "cwd":self.root.path(), "mcpServers":[]
        })).await;
    }

    async fn send_prompt(&self, id: u64, method: &str) {
        self.send(id, method, json!({
            "sessionId":self.session.session_id.as_str(), "prompt":[{"type":"text","text":"fixture input"}]
        })).await;
    }

    async fn send(&self, id: u64, method: &str, params: Value) {
        self.frames
            .send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await
            .expect("send ACP request");
    }

    async fn reply(&mut self, id: u64) -> Value {
        loop {
            let frame = self.replies.recv().await.expect("ACP reply");
            if frame.get("id").is_some() && frame.get("method").is_none() {
                assert_eq!(frame["id"], id, "{}", *frame);
                return (*frame).clone();
            }
        }
    }

    async fn wire_request(&self, field: &str) -> Value {
        loop {
            if let Ok(bytes) = tokio::fs::read(&self.receipt).await
                && let Ok(receipt) = serde_json::from_slice::<Value>(&bytes)
                && !receipt[field][0].is_null()
            {
                return receipt[field][0].clone();
            }
            tokio::task::yield_now().await;
        }
    }

    async fn stop(self) {
        self.closed.cancel();
        self.route
            .await
            .expect("route task")
            .expect("route shutdown");
        self.supervisor.shutdown().await.expect("fixture shutdown");
    }
}

#[tokio::test]
async fn acp_error_carries_cursor_lost_session_guidance() {
    let mut route = FailureRoute::start(
        Arc::new(acp_provider_adapters::CursorAcpBehavior),
        "restore-session-not-found",
    )
    .await;

    route.load().await;
    let reply = route.reply(1).await;
    let wire_call = route.wire_request("restores").await;
    route.stop().await;

    assert_eq!(wire_call["method"], "session/load");
    assert_eq!(wire_call["sessionId"], "saved-session");
    let error = &reply["error"];
    assert_eq!(error["code"], -32002, "{reply}");
    let reference = error["message"].as_str().and_then(|message| {
        message.strip_prefix("the provider no longer knows this session, so start a new session with the same setup (provider code -32602; reference ")
            .and_then(|message| message.strip_suffix(')'))
    }).unwrap_or_else(|| panic!("expected lost-session guidance and reference, got {reply}"));
    assert_eq!(
        uuid::Uuid::parse_str(reference)
            .expect("reference")
            .get_version_num(),
        7
    );
    assert_eq!(error["data"]["kind"], "providerSessionNotFound", "{reply}");
}

#[tokio::test]
async fn acp_error_carries_unanswered_restore_guidance() {
    let mut route = FailureRoute::start(
        Arc::new(acp_provider_adapters::StandardAcpBehavior),
        "silent-first-restore",
    )
    .await;

    route.load().await;
    assert_eq!(
        route.wire_request("restores").await["method"],
        "session/load"
    );
    let budget = Duration::from_secs(30);
    route
        .clock
        .not_due_check_now(route.clock.monotonic_now().after(budget))
        .await;
    route
        .clock
        .advance(budget)
        .expect("expire the wire restore");
    let reply = route.reply(1).await;
    route.stop().await;

    let error = &reply["error"];
    assert_eq!(error["code"], -32000, "{reply}");
    assert_eq!(
        error["message"], "the provider did not answer the restore in time, so retry this session",
        "{reply}"
    );
    assert_eq!(error["data"]["kind"], "outcomeUnknown", "{reply}");
}

#[tokio::test]
async fn acp_error_carries_explicit_steer_refusal_guidance() {
    let mut route = FailureRoute::start(
        Arc::new(acp_provider_adapters::StandardAcpBehavior),
        "silent-prompts",
    )
    .await;
    route.load().await;
    let restored = route.reply(1).await;
    assert_eq!(
        restored["result"]["sessionId"], "saved-session",
        "{restored}"
    );
    route.send_prompt(2, "session/prompt").await;
    assert_eq!(route.wire_request("prompts").await, "saved-session");

    route.send_prompt(3, "_session/steering").await;
    let reply = route.reply(3).await;
    route.stop().await;

    let error = &reply["error"];
    assert_eq!(error["code"], -32601, "{reply}");
    assert_eq!(
        error["message"], "a turn is running; queue the message or cancel the turn",
        "{reply}"
    );
    assert_eq!(error["data"]["kind"], "cannotSteer", "{reply}");
}
