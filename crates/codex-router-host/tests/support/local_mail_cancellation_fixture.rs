//! Arrange a real pending Local control, then close its owner before the Host starts.
use codex_router_host::{
    CollaborationRuntime, CollaborationRuntimeInputs, ExternalProviderStartup,
};
use collaboration_contracts::RouterConsumerProtocol;
use collaboration_contracts::RouterDeliveryTypes;
use collaboration_mail_protocol::MailSender;
use collaboration_protocol::{
    EndpointId, EndpointRef, MailItemId, PushRecord, SessionId, SessionRef,
};
use durable_steps::{
    AtomicStore, CallFailure, CallId, NotSentReason, StoreError, turso_store::TursoScope,
};
use fleet_sync_delivery::{
    DeliveryConsumer, DeliveryError, DeliveryLimits, GateDecision, Inbox, LocalDelivery,
    LocalOutcome, delivery_rollback, minimum_settlement_allowance,
};
use fleet_sync_protocol::{
    CancelBody, DeliveryCancel, DeliveryHeader, DeliveryItem, ItemId, PeerId, Recipient,
    RecipientOutcome,
};
use fleet_sync_tokio_runner::{CallEnvelope, CallRouter, TokioHost};
use interaction_storage::{InteractionAtomicStore, InteractionStore, MessageSendAnswer};
use runtime_primitives::{Clock, SystemClock};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{
    io,
    os::unix::{fs::PermissionsExt as _, net::UnixListener},
    path::{Path, PathBuf},
    sync::Arc,
};

pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
type SetupHost = TokioHost<NoSetupCalls, SystemClock>;

struct NoSetupCalls;
impl CallRouter for NoSetupCalls {
    async fn dispatch(&self, _: CallEnvelope) -> Result<Value, CallFailure> {
        Err(CallFailure::NotSent {
            reason: NotSentReason::Overloaded,
        })
    }
}

// Only arrangement uses this consumer: the original does not exist yet, so no outcome is supplied.
struct PendingControlSetup;
impl DeliveryConsumer<TursoScope<'_, SetupHost>> for PendingControlSetup {
    type Protocol = RouterConsumerProtocol;
    async fn release_gate(
        &self,
        _: &mut TursoScope<'_, SetupHost>,
        _: &DeliveryItem<RouterDeliveryTypes>,
    ) -> Result<GateDecision, DeliveryError> {
        Ok(GateDecision::Release)
    }
    async fn deliver_locally(
        &self,
        _: &DeliveryItem<RouterDeliveryTypes>,
        _: CallId,
    ) -> LocalDelivery<RouterDeliveryTypes> {
        LocalDelivery {
            outcome: LocalOutcome::Uncertain,
            observation: collaboration_protocol::DeliveryReceipt {
                outcome: collaboration_protocol::DeliveryOutcome::Unknown,
                reachability: None,
                client: None,
            }
            .into(),
        }
    }
    fn may_cancel(&self, sender: &MailSender, item: &DeliveryHeader<RouterDeliveryTypes>) -> bool {
        sender == item.sender()
    }
}

pub struct PendingLocalCancelFixture {
    directory: tempfile::TempDir,
    registry: PathBuf,
    listener: UnixListener,
    pub target: SessionRef,
    pub item: MailItemId,
    pub cancel: ItemId,
    peer: PeerId,
}

pub struct ColdMailState {
    pub push: PushRecord,
    pub answer: MessageSendAnswer,
    pub outcome: Option<RecipientOutcome<collaboration_mail_protocol::CarrierAcceptance>>,
}

fn setup_limits() -> TestResult<DeliveryLimits> {
    Ok(DeliveryLimits::new::<RouterDeliveryTypes>(
        16 * 1024 * 1024,
        30,
        16 * 1024 * 1024,
        30,
        16 * 1024 * 1024,
        minimum_settlement_allowance::<RouterDeliveryTypes>()?,
    )?)
}

impl PendingLocalCancelFixture {
    pub async fn prepare() -> TestResult<Self> {
        let directory = tempfile::Builder::new()
            .prefix("crlc-")
            .tempdir_in("/tmp")?;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
        let service = collaboration_runtime::load_service_identity(directory.path())?;
        let peer = collaboration_runtime::load_installation_peer(directory.path())?;
        let registry = directory.path().join("peer-registry");
        std::fs::create_dir(&registry)?;
        let socket = registry.join("peer.sock");
        let listener = UnixListener::bind(&socket)?;
        listener.set_nonblocking(true)?;
        let target = SessionRef {
            endpoint: EndpointRef {
                service_id: service.clone(),
                endpoint_id: EndpointId::try_from("claude-local".to_owned())?,
            },
            session_id: SessionId::try_from("pending-cancel-recipient".to_owned())?,
        };
        let pid = std::process::id();
        std::fs::write(
            registry.join(format!("{pid}.json")),
            serde_json::to_vec(&json!({
                "pid":pid,"sessionId":target.session_id,"status":"busy","peerProtocol":1,
                "messagingSocketPath":socket,"cwd":directory.path(),"kind":"interactive","entrypoint":"cli",
                "startedAt":1000,"updatedAt":2000,
            }))?,
        )?;
        let digest = Sha256::digest(
            socket
                .to_str()
                .ok_or("peer socket is not UTF-8")?
                .as_bytes(),
        );
        let key = registry.join(format!("{pid}.{digest:x}.key"));
        std::fs::write(&key, br#"{"peerToken":"0123456789abcdef0123456789abcdef"}"#)?;
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600))?;
        let item = MailItemId::generate();
        let original = ItemId::new(uuid::Uuid::parse_str(item.as_str())?)?;
        let cancel = ItemId::new(uuid::Uuid::now_v7())?;
        let control = DeliveryCancel::<RouterDeliveryTypes>::new(
            cancel,
            MailSender::HumanUser {
                installation: String::from(service.clone()).try_into()?,
            },
            Recipient {
                peer,
                address: target.clone().try_into()?,
            },
            None,
            CancelBody { target: original },
        )?;
        let owner =
            Arc::new(InteractionStore::open(directory.path().join("interaction.sqlite")).await?);
        let host = Arc::new(TokioHost::new(Arc::new(NoSetupCalls), SystemClock));
        let now = host.now();
        let store = InteractionAtomicStore::new(owner.clone(), host);
        let limits = setup_limits()?;
        let reference = store
            .transaction(move |scope| {
                Box::pin(async move {
                    let admitted = Inbox::<_, RouterDeliveryTypes>::new(scope, peer, limits)
                        .enqueue_local_cancel(control, &PendingControlSetup, now)
                        .await
                        .map_err(delivery_rollback)?;
                    if !admitted.effects.attempts.is_empty() || !admitted.effects.reports.is_empty()
                    {
                        return Err(StoreError::backend(
                            "a pending control unexpectedly admitted work",
                        ));
                    }
                    Ok(admitted.reference)
                })
            })
            .await?;
        if reference.item != cancel {
            return Err("pending control changed identity".into());
        }
        drop(store);
        Arc::try_unwrap(owner)
            .map_err(|_| "pending control owner is retained")?
            .close()
            .await?;
        Ok(Self {
            directory,
            registry,
            listener,
            target,
            item,
            cancel,
            peer,
        })
    }

    pub fn root(&self) -> &Path {
        self.directory.path()
    }

    pub async fn start_host(&self) -> TestResult<CollaborationRuntime> {
        Ok(CollaborationRuntime::start_with_external_providers(
            CollaborationRuntimeInputs {
                board_session: None,
                directory: self.root().to_owned(),
                codex_home: self.root().to_owned(),
                backend_socket: self.root().join("absent-native.sock"),
                mcp_bind: ([127, 0, 0, 1], 0).into(),
                native_schema: None,
                peer_registry_directory: Some(self.registry.clone()),
                remote_control_server_name: None,
                owner_human_id: None,
            },
            vec![
                ExternalProviderStartup::unavailable(
                    collaboration_protocol::ProviderKind::ClaudeCode,
                    "ACP is outside this local peer proof".to_owned(),
                    "Use the real scripted peer".to_owned(),
                )?,
                ExternalProviderStartup::unavailable(
                    collaboration_protocol::ProviderKind::Cursor,
                    "ACP is outside this local peer proof".to_owned(),
                    "Use the real scripted peer".to_owned(),
                )?,
            ],
        )
        .await?)
    }

    pub fn require_no_native_input(&self) -> TestResult {
        match self.listener.accept() {
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(()),
            Err(error) => Err(error.into()),
            Ok((stream, _)) => {
                drop(stream);
                Err("a cancelled original reached the actual native peer socket".into())
            }
        }
    }

    /// Call only after the Host has shut down; no second physical owner is opened live.
    pub async fn read_after_shutdown(&self) -> TestResult<ColdMailState> {
        let owner = Arc::new(InteractionStore::open(self.root().join("interaction.sqlite")).await?);
        let push_id = self.item.clone().try_into()?;
        let push = owner
            .get_push_record(&push_id)
            .await?
            .ok_or("native push is absent")?;
        let answer = owner
            .message_send_answer(&self.item)
            .await?
            .ok_or("retained send answer is absent")?;
        let store = InteractionAtomicStore::new(
            owner.clone(),
            Arc::new(TokioHost::new(Arc::new(NoSetupCalls), SystemClock)),
        );
        let peer = self.peer;
        let item = ItemId::new(uuid::Uuid::parse_str(self.item.as_str())?)?;
        let limits = setup_limits()?;
        let outcome = store
            .transaction(move |scope| {
                Box::pin(async move {
                    Inbox::<_, RouterDeliveryTypes>::new(scope, peer, limits)
                        .local_outcome(item)
                        .await
                        .map_err(delivery_rollback)
                })
            })
            .await?;
        drop(store);
        Arc::try_unwrap(owner)
            .map_err(|_| "cold observation owner is retained")?
            .close()
            .await?;
        Ok(ColdMailState {
            push,
            answer,
            outcome,
        })
    }
}
