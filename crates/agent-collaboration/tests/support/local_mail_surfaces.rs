//! V8: real CLI and MCP transport share Local custody. External carrier I/O is the named stand-in.
use super::native_mail_owner;
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
    ) -> DeliveryFuture<'a, collaboration_service::NativeDeliveryResult> {
        self.injections.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Ok({
                let receipt = DeliveryReceipt {
                    outcome: DeliveryOutcome::PeerMessageWritten,
                    reachability: Some(SessionReachability::ClaudeCodePeer),
                    client: Some(DeliveryClientReceipt::ClaudeCodePeer),
                };
                collaboration_service::NativeDeliveryResult::observed(
                    collaboration_service::NativeObservedOutcome::PeerMessageWritten,
                    receipt.reachability,
                    receipt.client,
                )
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
    fn presence(&self, target: &SessionRef) -> DeliveryFuture<'_, TargetPresence> {
        let presence = if String::from(target.session_id.clone()) == "capacity-unloaded" {
            TargetPresence::Wakeable
        } else {
            TargetPresence::Running
        };
        Box::pin(async move { Ok(presence) })
    }
}
pub(super) struct Surfaces {
    pub(super) root: tempfile::TempDir,
    pub(super) api: ServedCollaborationApi,
    pub(super) delivery: SubscriptionDeliveryService,
    pub(super) interactions: Arc<interaction_storage::InteractionStore>,
    native: native_mail_owner::NativeMailOwner,
    pub(super) peer: fleet_sync_protocol::PeerId,
    pub(super) carrier: Arc<CarrierStandIn>,
}
impl Surfaces {
    #[allow(dead_code)] // Only the companion compaction target needs the frozen reference.
    pub(super) async fn delivery_reference(
        &self,
        item: &collaboration_protocol::MailItemId,
    ) -> TestResult<fleet_sync_protocol::DeliveryItemRef> {
        let push = item.as_str().to_owned().try_into()?;
        let record = self
            .interactions
            .get_push_record(&push)
            .await?
            .ok_or("saved native push")?;
        let item = collaboration_service::outgoing_mail_message(
            &draft_from_saved_push(record),
            &SERVICE.to_owned().try_into()?,
            self.peer,
        )?;
        Ok(fleet_sync_protocol::DeliveryItemRef {
            item: item.id(),
            digest: item.digest().clone(),
        })
    }
    pub(super) async fn start() -> TestResult<Self> {
        Self::start_with_session(None).await
    }
    pub(super) async fn start_with_session(
        session: Option<collaboration_service::MailDeliverySession>,
    ) -> TestResult<Self> {
        Self::start_with_project_admission(session, None).await
    }
    pub(super) async fn start_with_project_admission(
        session: Option<collaboration_service::MailDeliverySession>,
        admission: Option<Arc<collaboration_service::ProjectAdmission>>,
    ) -> TestResult<Self> {
        let root = tempfile::Builder::new()
            .prefix("m2c-mail-surface-")
            .tempdir()?;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
        let carrier = Arc::new(CarrierStandIn::default());
        let machine_identity =
            MachineIdentity::new(SERVICE.to_owned().try_into()?, Some("M2c fixture"))?;
        let interactions = std::sync::Arc::new(
            interaction_storage::InteractionStore::open(root.path().join("interaction.sqlite"))
                .await?,
        );
        let delivery = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
            board_availability: BoardAvailability::Unavailable,
            push_store: interactions.clone(),
            delivery: carrier.clone(),
            presence: carrier.clone(),
            machine_identity: machine_identity.clone(),
            clock: Arc::new(SystemSubscriptionClock),
        });
        delivery.start().await?;
        let mut identity = ServiceIdentity::new(SERVICE, SERVICE)?
            .with_session_delivery(carrier.clone())
            .with_subscription_delivery_service(delivery.clone(), carrier.clone());
        if let Some(admission) = admission {
            identity = identity.with_project_admission(admission);
        }
        let peer = match &session {
            Some(session) => session.peer,
            None => fleet_sync_protocol::PeerId::new(uuid::Uuid::now_v7())?,
        };
        let (identity, native) = native_mail_owner::NativeMailOwner::start_with_session(
            identity,
            peer,
            collaboration_service::MailPushDeliveryProps {
                interactions: interactions.clone(),
                actors: delivery.clone(),
                presence: carrier.clone(),
                machine: machine_identity,
                clock: Arc::new(SystemSubscriptionClock),
            },
            session,
        )?;
        let api =
            ServedCollaborationApi::start(root.path(), CollaborationApplication::new(identity))
                .await?;
        Ok(Self {
            root,
            api,
            delivery,
            interactions,
            native,
            peer,
            carrier,
        })
    }
    pub(super) fn session(id: &str) -> Value {
        json!({"endpoint":{"serviceId":SERVICE,"endpointId":"claude-local"},"sessionId":id})
    }
    #[allow(dead_code)] // Remote public-surface proofs inspect the actual SDK status owner.
    pub(super) fn mail_runtime(&self) -> Arc<collaboration_service::MailDeliveryRuntime> {
        self.native.runtime_handle()
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
    #[allow(dead_code)] // Only held-custody proof targets use this input.
    pub(super) fn held_input(id: Option<&str>, text: &str) -> Value {
        let mut value = Self::input(id, text);
        if let Some(fields) = value.as_object_mut() {
            fields.insert("target".into(), Self::session("capacity-unloaded"));
            // Queue stays loaded-only. A first unguarded Auto is allowed to wake.
            fields.insert("delivery".into(), json!("queue"));
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
        drop(self.interactions);
        self.native.close().await?;
        Ok(())
    }
}

pub(super) fn draft_from_saved_push(
    record: collaboration_protocol::PushRecord,
) -> collaboration_protocol::PushRecordDraft {
    collaboration_protocol::PushRecordDraft {
        push_id: record.push_id,
        kind: record.kind,
        origin: record.origin,
        origin_router_ref: record.origin_router_ref,
        target: record.target,
        mode: record.mode,
        guard: record.guard,
        reply_to_push_id: record.reply_to_push_id,
        header_facts: record.header_facts,
        body: record.body,
        activity: record.activity,
        created_at: record.created_at,
    }
}
