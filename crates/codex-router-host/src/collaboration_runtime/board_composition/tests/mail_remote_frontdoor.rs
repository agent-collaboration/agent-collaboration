//! Actual public producer, two authenticated peers and generic relay; external carrier I/O stands in.
use super::*;
use collaboration_contracts::AttemptEvidenceSink;
use collaboration_contracts::AttemptReconciliation;
use collaboration_contracts::AttemptReconciliationContext;
use collaboration_contracts::DeliveryContractError;
use collaboration_contracts::DeliveryFuture;
use collaboration_contracts::MachineIdentity;
use collaboration_contracts::NativeDeliveryResult;
use collaboration_contracts::NativeObservedOutcome;
use collaboration_contracts::SessionMessageDelivery;
use collaboration_contracts::TargetPresence;
use collaboration_contracts::TargetPresenceProbe;
use collaboration_mcp::test_support::ServedCollaborationApi;
use collaboration_protocol::{
    DeliveryClientReceipt, MailItemId, PushId, SessionReachability, SessionRef as RouterSessionRef,
};
use collaboration_service::ApplicationFixture;

use fleet_sync_protocol::DeliveryStatus;
use runtime_primitives::SystemClock;
use serde_json::json;
use std::{os::unix::fs::PermissionsExt, sync::OnceLock};
use transport::WireEvent;

const SOURCE: &str = "00000000-0000-4000-8000-000000000001";
const RECIPIENT: &str = "00000000-0000-4000-8000-000000000002";

#[derive(Clone, Debug)]
struct CapturedDelivery {
    item: PushId,
    target: RouterSessionRef,
    line: String,
}
#[derive(Default)]
struct CarrierStandIn {
    owner: OnceLock<Weak<interaction_storage::InteractionStore>>,
    deliveries: std::sync::Mutex<Vec<CapturedDelivery>>,
}
impl TargetPresenceProbe for CarrierStandIn {
    fn presence(&self, _: &RouterSessionRef) -> DeliveryFuture<'_, TargetPresence> {
        Box::pin(async { Ok(TargetPresence::Running) })
    }
}
impl SessionMessageDelivery for CarrierStandIn {
    fn deliver<'a>(
        &'a self,
        request: collaboration_contracts::layer_zero::DeliveryRequest,
        _: &'a dyn AttemptEvidenceSink,
    ) -> DeliveryFuture<'a, NativeDeliveryResult> {
        Box::pin(async move {
            let owner = self
                .owner
                .get()
                .and_then(Weak::upgrade)
                .ok_or(DeliveryContractError::InvalidEvidence)?;
            let saved = owner
                .get_push_record(&request.payload.push_id)
                .await
                .map_err(|_| DeliveryContractError::EvidencePersistence)?;
            if saved.is_none() {
                return Err(DeliveryContractError::InvalidEvidence);
            }
            self.deliveries
                .lock()
                .map_err(|_| DeliveryContractError::InvalidEvidence)?
                .push(CapturedDelivery {
                    item: request.payload.push_id,
                    target: request.target,
                    line: request.payload.line.as_str().to_owned(),
                });
            Ok(NativeDeliveryResult::observed(
                NativeObservedOutcome::PeerMessageWritten,
                Some(SessionReachability::ClaudeCodePeer),
                Some(DeliveryClientReceipt::ClaudeCodePeer),
            ))
        })
    }
    fn reconcile_attempt(
        &self,
        _: AttemptReconciliationContext,
    ) -> DeliveryFuture<'_, AttemptReconciliation> {
        Box::pin(async { Ok(AttemptReconciliation::StillUnknown) })
    }
}

struct RecipientRuntime {
    mail: mail_owner::MailOwner,
    registry: Arc<ProjectStoreRegistry>,
    projects: Arc<ProjectWriters>,
    transport: NoticeTransport,
    ledger: Arc<NoticeLedger>,
    ready: watch::Receiver<bool>,
    stop: watch::Sender<bool>,
    shutdown: tokio_util::sync::CancellationToken,
    tasks: JoinSet<io::Result<()>>,
}
impl RecipientRuntime {
    async fn open(
        root: &Path,
        inputs: board_fleet_sync::BoardSessionInputs,
        carrier: Arc<CarrierStandIn>,
    ) -> Result<Self, TestError> {
        tokio::fs::create_dir_all(root).await?;
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
        let ledger = NoticeLedger::open(root, inputs.peer).await?;
        let transport = NoticeTransport::new(
            ReqwestSessionTransport::new(inputs.access, BOUND, BOUND)?,
            ledger.clone(),
            false,
        );
        let client = SessionClient::<_, _, BoardConsumerProtocol>::load(
            transport.clone(),
            MachineSessionStore::open(root).await?,
            &UuidV7Ids,
            SchemaSupport { max_version: 1 },
            Backoff {
                initial: Duration::from_millis(100),
                max: BOUND,
                factor: 2,
            },
        )
        .await?;
        let (requests, events, pump) =
            SessionPump::new(client, inputs.endpoint, inputs.credential, BOUND);
        let (welcomed, ready) = watch::channel(false);
        let (stop, stopped) = watch::channel(false);
        let mail = mail_owner::MailOwner::open_for_machine(
            root,
            inputs.peer,
            collaboration_runtime::MailDeliverySession {
                requests: requests.clone(),
                welcomed: ready.clone(),
                peer: inputs.peer,
                relay: inputs.relay,
            },
            MachineIdentity::new(
                RECIPIENT.to_owned().try_into()?,
                Some("actual recipient fixture"),
            )?,
            carrier.clone(),
        )
        .await?;
        carrier
            .owner
            .set(Arc::downgrade(&mail.owner))
            .map_err(|_| "carrier owner already bound")?;
        let registry = Arc::new(ProjectStoreRegistry::new(
            root,
            Some(inputs.peer),
            Arc::new(SystemClock),
        ));
        let projects = Arc::new(ProjectWriters::new(
            root,
            inputs.peer,
            &registry,
            requests.clone(),
            inputs.receiver,
            ready.clone(),
            stopped.clone(),
        ));
        let shutdown = tokio_util::sync::CancellationToken::new();
        let mut tasks = JoinSet::new();
        tasks.spawn(async move { pump.run(stopped).await.map_err(io::Error::other) });
        tasks.spawn(run_events(
            events,
            requests,
            MachineNoticeReactions::open(root).await?,
            MachineSessionEventOwners {
                projects: projects.clone(),
                admission: Weak::new(),
                fleet: Weak::new(),
                publisher: Weak::new(),
                mail: Arc::downgrade(&mail.mail),
            },
            welcomed,
            stop.subscribe(),
        ));
        let running = mail.mail.clone();
        let cancelled = shutdown.clone();
        tasks.spawn(async move { running.run(cancelled).await.map_err(io::Error::other) });
        let mut observed = ready.clone();
        tokio::time::timeout(BOUND, async {
            while !*observed.borrow() {
                observed.changed().await?;
            }
            Ok::<_, tokio::sync::watch::error::RecvError>(())
        })
        .await??;
        Ok(Self {
            mail,
            registry,
            projects,
            transport,
            ledger,
            ready,
            stop,
            shutdown,
            tasks,
        })
    }
    async fn close(mut self) -> Result<(), TestError> {
        self.stop.send_replace(true);
        self.shutdown.cancel();
        while let Some(result) = tokio::time::timeout(BOUND, self.tasks.join_next()).await? {
            result??;
        }
        self.projects.shutdown().await?;
        drop(self.projects);
        Arc::try_unwrap(self.registry)
            .map_err(|_| "recipient registry retained")?
            .close()
            .await?;
        drop(self.transport);
        Arc::try_unwrap(self.ledger)
            .map_err(|_| "recipient notice ledger retained")?
            .close()
            .await?;
        self.mail.close().await
    }
}

async fn serve(
    root: &Path,
    identity: ApplicationFixture,
) -> Result<ServedCollaborationApi, TestError> {
    tokio::fs::create_dir_all(root).await?;
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
    Ok(ServedCollaborationApi::start(root, (identity).build())?)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn public_remote_send_queued_before_welcome_reaches_distinct_recipient_through_real_relay_and_canonical_status()
-> Result<(), TestError> {
    let mut source = MailHarness::open(false).await?;
    let recipient_inputs = source.central.recipient_inputs()?;
    assert_ne!(recipient_inputs.peer, source.peer());
    let remote_peer = recipient_inputs.peer;
    let carrier = Arc::new(CarrierStandIn::default());
    let recipient = RecipientRuntime::open(
        &source.root.join("recipient-router"),
        recipient_inputs,
        carrier.clone(),
    )
    .await?;
    assert!(*recipient.ready.borrow());
    source
        .mail
        .owner
        .remember_installation_peer(&interaction_storage::InstallationPeerBinding {
            service_id: RECIPIENT.to_owned().try_into()?,
            peer_id: remote_peer.as_uuid().to_string().try_into()?,
        })
        .await?;
    let target: RouterSessionRef = serde_json::from_value(json!({
        "endpoint":{"serviceId":RECIPIENT,"endpointId":"claude-local"},"sessionId":"real-remote-recipient",
    }))?;
    let identity = ApplicationFixture::new(SOURCE, SOURCE)?
        .with_interaction_store(source.mail.owner.clone())
        .with_installation_peer(source.peer())
        .map_err(io::Error::other)?
        .with_mail_delivery_runtime(source.mail.mail.clone())
        .map_err(io::Error::other)?;
    let api = serve(&source.root.join("sender-api"), identity).await?;
    let item = MailItemId::generate();
    let input = json!({"itemId":item,"target":target,"message":{"kind":"humanUser","text":"actual remote producer and relay"},"delivery":"auto"});
    let queued = api.call("message_send", input.clone()).await?;
    assert_eq!(
        queued.pointer("/result/queued/item"),
        Some(&json!(item)),
        "{queued}"
    );
    assert!(!*source.welcomed.borrow());
    let sdk = fleet_sync_protocol::ItemId::new(uuid::Uuid::parse_str(item.as_str())?)?;
    assert!(
        !source
            .transport
            .observer
            .events()
            .iter()
            .any(|event| matches!(event, WireEvent::MailSubmit(id) if *id==sdk))
    );
    assert!(
        source
            .mail
            .owner
            .get_push_record(&item.clone().try_into()?)
            .await?
            .is_none()
    );
    source.release_welcome()?;
    source
        .wait_state(
            |state| state["items"]["active"]["cycle"] == 1,
            "actual source Welcome did not admit receive obligations",
        )
        .await?;
    source.start_mail();
    source
        .transport
        .observer
        .wait_for(
            |events| {
                events
                    .iter()
                    .any(|event| matches!(event, WireEvent::GenericSubmit(id) if *id==sdk))
            },
            "public producer never reached actual generic relay",
        )
        .await?;
    // The consumer's notification publication policy is the existing fixture
    // seam. SDK-generated mail hint crosses the recipient's real pump/Host loop.
    recipient
        .transport
        .publish(1, DeliveryStream::Items)
        .await?;
    source.transport.observer.wait_for(|events| events.iter().any(|event|
        matches!(event, WireEvent::Publication { item: id, status: DeliveryStatus::Delivered { .. }, .. } if *id==sdk)),
        "real recipient report did not publish canonical Delivered to sender").await?;
    let shown = tokio::time::timeout(BOUND, async {
        loop {
            let result = api
                .call(
                    "router_show",
                    json!({"caller":{"kind":"humanUser"},"reference":item}),
                )
                .await?;
            if result.pointer("/result/mail/status/kind") == Some(&json!("delivered")) {
                return Ok::<_, TestError>(result);
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    assert_eq!(
        shown.pointer("/result/mail/status/acceptance"),
        Some(&json!("peerMessageWritten"))
    );
    let receiving = serve(
        &source.root.join("recipient-api"),
        ApplicationFixture::new(RECIPIENT, RECIPIENT)?
            .with_interaction_store(recipient.mail.owner.clone()),
    )
    .await?;
    let received = receiving
        .call("router_show", json!({"caller":target,"reference":item}))
        .await?;
    assert_eq!(
        received.pointer("/result/record/body"),
        Some(&json!("actual remote producer and relay")),
        "{received}"
    );
    assert_eq!(
        received.pointer("/result/record/target"),
        Some(&json!(target))
    );
    assert_eq!(
        api.call("message_send", input).await?,
        queued,
        "delivered replay lost immutable queued admission answer"
    );
    let deliveries = carrier
        .deliveries
        .lock()
        .map_err(|_| "carrier observations poisoned")?
        .clone();
    assert_eq!(deliveries.len(), 1);
    let delivered = deliveries
        .first()
        .ok_or("external carrier stand-in received nothing")?;
    assert_eq!(delivered.item.as_str(), item.as_str());
    assert_eq!(delivered.target, target);
    assert!(
        delivered
            .line
            .contains(&format!("router://{RECIPIENT}/push/{}", item.as_str()))
    );
    let events = source.transport.observer.events();
    assert!(events.iter().any(
        |event| matches!(event, WireEvent::SubmissionEpoch { item:id, epoch:None } if *id==sdk)
    ));
    assert_eq!(
        source.connections(),
        2,
        "two Routers must use exactly their two existing authenticated sessions"
    );
    assert!(
        source
            .mail
            .owner
            .mail_submission_epoch(&item)
            .await?
            .is_none()
    );
    assert!(
        source
            .mail
            .owner
            .get_push_record(&item.try_into()?)
            .await?
            .is_none()
    );
    receiving.stop().await?;
    api.stop().await?;
    recipient.close().await?;
    source.close().await
}
