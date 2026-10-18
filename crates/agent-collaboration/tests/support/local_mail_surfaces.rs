//! V8: real CLI and MCP transport share Local custody. External carrier I/O is the named stand-in.
use automation_storage::AutomationStore;
use collaboration_mcp::test_support::ServedCollaborationApi;
use collaboration_protocol::{
    DeliveryClientReceipt, DeliveryOutcome, DeliveryReceipt, SessionReachability, SessionRef,
};
use collaboration_service::{
    AttemptEvidenceSink, AttemptReconciliation, AttemptReconciliationContext, BoardAvailability,
    CollaborationApplication, DeliveryFuture, MachineIdentity, ServiceIdentity,
    SessionMessageDelivery, SubscriptionDeliveryService, SubscriptionDeliveryServiceProps,
    SystemSubscriptionClock, TargetPresence, TargetPresenceProbe,
};
use serde_json::{Value, json};
use std::{
    os::unix::fs::PermissionsExt,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Mutex;
pub(super) type TestResult<TValue = ()> = Result<TValue, Box<dyn std::error::Error + Send + Sync>>;
pub(super) const SERVICE: &str = "00000000-0000-4000-8000-000000000001";
#[derive(Default)]
pub(super) struct CarrierStandIn {
    pub(super) injections: AtomicUsize,
}
impl SessionMessageDelivery for CarrierStandIn {
    fn deliver<'a>(
        &'a self,
        _request: collaboration_service::layer_zero::DeliveryRequest,
        _evidence: &'a dyn AttemptEvidenceSink,
    ) -> DeliveryFuture<'a, DeliveryReceipt> {
        self.injections.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Ok(DeliveryReceipt {
                outcome: DeliveryOutcome::PeerMessageWritten,
                reachability: Some(SessionReachability::ClaudeCodePeer),
                client: Some(DeliveryClientReceipt::ClaudeCodePeer),
            })
        })
    }
    fn reconcile_attempt(
        &self,
        _: AttemptReconciliationContext,
    ) -> DeliveryFuture<'_, AttemptReconciliation> {
        Box::pin(async { Ok(AttemptReconciliation::StillUnknown) })
    }
}
impl TargetPresenceProbe for CarrierStandIn {
    fn presence(&self, _: &SessionRef) -> DeliveryFuture<'_, TargetPresence> {
        Box::pin(async { Ok(TargetPresence::Running) })
    }
}
pub(super) struct Surfaces {
    pub(super) root: tempfile::TempDir,
    pub(super) api: ServedCollaborationApi,
    pub(super) delivery: SubscriptionDeliveryService,
    pub(super) store: Arc<Mutex<AutomationStore>>,
    pub(super) carrier: Arc<CarrierStandIn>,
}
impl Surfaces {
    pub(super) async fn start() -> TestResult<Self> {
        let root = tempfile::Builder::new()
            .prefix("m2c-mail-surface-")
            .tempdir()?;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
        let store = Arc::new(Mutex::new(
            AutomationStore::open(&root.path().join("automation.sqlite")).await?,
        ));
        let carrier = Arc::new(CarrierStandIn::default());
        let machine_identity =
            MachineIdentity::new(SERVICE.to_owned().try_into()?, Some("M2c fixture"))?;
        let delivery = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
            board_availability: BoardAvailability::Unavailable,
            push_store: store.clone(),
            delivery: carrier.clone(),
            presence: carrier.clone(),
            machine_identity,
            clock: Arc::new(SystemSubscriptionClock),
        });
        delivery.start().await?;
        let identity = ServiceIdentity::new(SERVICE, SERVICE)?
            .with_automation_store(store.clone())
            .with_subscription_delivery_service(delivery.clone(), carrier.clone());
        let api =
            ServedCollaborationApi::start(root.path(), CollaborationApplication::new(identity))
                .await?;
        Ok(Self {
            root,
            api,
            delivery,
            store,
            carrier,
        })
    }
    pub(super) fn session(id: &str) -> Value {
        json!({"endpoint":{"serviceId":SERVICE,"endpointId":"claude-local"},"sessionId":id})
    }
    pub(super) fn input(id: Option<&str>, text: &str) -> Value {
        let mut value = json!({"target":Self::session("fixture-target"),"message":{"kind":"humanUser","text":text},"delivery":"auto"});
        if let Some(id) = id
            && let Some(fields) = value.as_object_mut()
        {
            fields.insert("itemId".into(), json!(id));
        }
        value
    }
    pub(super) async fn cli(&self, text: &str, flags: &[&str]) -> TestResult<(i32, Value, String)> {
        let target = Self::session("fixture-target").to_string();
        let output = tokio::time::timeout(
            Duration::from_secs(10),
            tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
                .kill_on_drop(true)
                .env("CODEX_THREAD_ID", "fixture-sender")
                .env_remove("CLAUDE_CODE_SESSION_ID")
                .env_remove("CURSOR_CONVERSATION_ID")
                .args([
                    "message",
                    "send",
                    "--to",
                    &target,
                    "--text",
                    text,
                    "--json",
                    "--service-directory",
                ])
                .arg(self.root.path())
                .args(flags)
                .output(),
        )
        .await??;
        let stderr = String::from_utf8(output.stderr)?;
        let wire = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("CLI JSON: {error}; stderr={stderr}"))?;
        Ok((output.status.code().unwrap_or(-1), wire, stderr))
    }
    pub(super) async fn stop(self) -> TestResult {
        self.api.stop().await?;
        self.delivery.shutdown().await;
        drop(self.delivery);
        Arc::try_unwrap(self.store)
            .map_err(|_| "delivery store still shared after shutdown")?
            .into_inner()
            .close()
            .await?;
        Ok(())
    }
}
