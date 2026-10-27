//! The mailbox's receipt and mail mutation survive reopening its owning store.
#![cfg(feature = "conformance")]

use board_sync_protocol::BoardDeliveryTypes;
use collaboration_central_core::{CoreHost, MailboxKind, ObjectAddress, ObjectRuntime, PlatformIo};
use collaboration_central_tokio::TokioStorageHost;
use durable_steps::{AtomicStore, CallFailure, CallId, OperationId, StepName, StoreError, Uuid};
use fleet_sync_delivery::{DeliveryLimits, Mailbox, minimum_settlement_allowance};
use fleet_sync_protocol::{DeliveryEnvelope, DeliveryItem, ItemId, PeerId, ReceiverId, Recipient};
use fleet_sync_tokio_runner::{ObjectKey, ObjectStore, TokioObject};
use message_board::ServiceId;
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
#[path = "native_http/support.rs"]
mod configuration;
type MailRuntime = ObjectRuntime<MailboxKind, TokioStorageHost<CoreHost<LocalIo>>, LocalIo>;
#[derive(Clone)]
struct LocalIo {
    fail_wake: Arc<std::sync::atomic::AtomicBool>,
    time: Arc<std::sync::atomic::AtomicI64>,
}
impl Default for LocalIo {
    fn default() -> Self {
        Self {
            fail_wake: Arc::default(),
            time: Arc::new(std::sync::atomic::AtomicI64::new(1_792_281_600_000)),
        }
    }
}
impl Clock for LocalIo {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.time.load(std::sync::atomic::Ordering::SeqCst))
    }
}
impl IdSource for LocalIo {
    fn new_id(&self) -> Uuid {
        Uuid::now_v7()
    }
}
impl WakeScheduler for LocalIo {
    async fn wake_at(&self, _: SyncTime) -> Result<(), WakeFailure> {
        if self.fail_wake.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(WakeFailure::Unavailable {
                message: "controlled post-commit wake loss".into(),
            });
        }
        Ok(())
    }
}
impl PlatformIo for LocalIo {
    async fn object_call(&self, _: ObjectAddress, _: Vec<u8>) -> Result<Vec<u8>, CallFailure> {
        Err(CallFailure::AnswerLost)
    }
}
fn test_limits() -> TestResult<DeliveryLimits> {
    // Explicit fixture capacity; this does not choose central's unsettled defaults.
    Ok(DeliveryLimits::new::<BoardDeliveryTypes>(
        2_000_000,
        100,
        2_000_000,
        100,
        2_000_000,
        minimum_settlement_allowance::<BoardDeliveryTypes>()?,
    )?)
}
struct Fixture {
    store: ObjectStore<CoreHost<LocalIo>>,
    object: Arc<TokioObject<CoreHost<LocalIo>>>,
    runtime: MailRuntime,
    io: LocalIo,
}
impl Fixture {
    async fn open(root: &Path, service: &ServiceId) -> TestResult<Self> {
        Self::open_named(root, "mailbox".into(), service).await
    }
    async fn open_named(root: &Path, key: String, service: &ServiceId) -> TestResult<Self> {
        Self::open_with_limits(root, key, service, test_limits()?).await
    }
    async fn open_with_limits(
        root: &Path,
        key: String,
        service: &ServiceId,
        limits: DeliveryLimits,
    ) -> TestResult<Self> {
        let io = LocalIo::default();
        let store = ObjectStore::open(root).await?;
        let receiver = ReceiverId::new(Uuid::now_v7())?;
        let object = store
            .object(
                ObjectKey::new(key)?,
                Default::default(),
                Arc::new(CoreHost::for_store(io.clone(), receiver)),
            )
            .await?;
        let runtime = ObjectRuntime::mailbox(
            Arc::new(TokioStorageHost::new(object.clone())),
            io.clone(),
            receiver,
            service.clone(),
            limits,
        )?;
        runtime.initialize().await?;
        Ok(Self {
            store,
            object,
            runtime,
            io,
        })
    }
    async fn submit(&self, peer: PeerId, service: &ServiceId, id: ItemId) -> TestResult<Value> {
        let item = DeliveryEnvelope::Item(DeliveryItem::<BoardDeliveryTypes>::new(
            id,
            serde_json::from_value(json!({"kind":"humanUser","installation":service}))?,
            Recipient {
                peer,
                address: serde_json::from_value(json!({
                    "endpoint":{"serviceId":service,"endpointId":"codex-local"},
                    "sessionId":"mailbox-close-proof"
                }))?,
            },
            None,
            serde_json::from_value(json!({
                "kind":"message","text":"mail survives custody","delivery":"auto","guard":null,"senderDisplayName":null,"replyTo":null
            }))?,
        )?);
        let limits = test_limits()?;
        let route = service.clone();
        let now = self.io.now();
        Ok(self
            .object
            .transaction(move |scope| {
                Box::pin(async move {
                    let result = Mailbox::<_, BoardDeliveryTypes>::new(scope, peer, limits)
                        .submit(peer, item, now, route, None)
                        .await
                        .map_err(StoreError::backend)?;
                    serde_json::to_value(result).map_err(StoreError::backend)
                })
            })
            .await?)
    }
}
fn close_call(service: &ServiceId, peer: PeerId, call: &CallId) -> TestResult<Vec<u8>> {
    Ok(serde_json::to_vec(&json!({
        "source":{"kind":"registry"},"sourceFace":"registry","authority":{"kind":"object"},
        "destination":{"kind":"mailbox","value":service},"destinationFace":"mailbox",
        "identity":call,"kind":"central.close-peer-mailbox","target":{"peer":peer},"request":null
    }))?)
}

#[path = "mailbox_lifecycle/readmission.rs"]
mod readmission;
fn revoked() -> Value {
    json!({"kind":"refused","value":{"reason":{"kind":"recipientRevoked"}}})
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn normal_revocation_completes_only_after_its_physical_mailbox_has_closed() -> TestResult {
    use collaboration_central_conformance::{
        ConformanceClient, JwksFixtureServer, enroll_admitted_peer, fixture_policy, owner_assertion,
    };
    use collaboration_central_core::{
        RevocationControl, RevocationControlInput, RevocationObservation,
    };
    use collaboration_central_tokio::{TokioServer, TokioSettings};

    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let service = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400ef".to_owned())?;
    let server = TokioServer::start(TokioSettings {
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: configuration::registry_config()?,
        holder_answer_timeout: std::time::Duration::from_secs(2),
        copy_ack_wait: std::time::Duration::from_secs(2),
    })
    .await?;
    let proof = async {
        let client = ConformanceClient::new(server.base_url())?;
        let admitted = enroll_admitted_peer(
            &client,
            service.as_str(),
            "019f0000-0000-7000-8000-00000000101d",
            "mailbox-close-through-registry",
            "019f0000-0000-7000-8000-0000000009ef",
        )
        .await?;
        let peer = PeerId::new(admitted.peer)?;
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(8))
            .build()?;
        let answer: Value = http
            .post(format!("{}/admin/v1/machines/revoke", server.base_url()))
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .json(&json!({"kind":"revokeMachine","value":{"installation":service}}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(answer, json!({"kind":"machineRevoked"}));
        tokio::time::timeout(std::time::Duration::from_secs(8), async {
            loop {
                let observed: RevocationObservation = http
                    .post(format!(
                        "{}/admin/v1/conformance/revocation",
                        server.base_url()
                    ))
                    .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
                    .json(&RevocationControlInput {
                        service: service.clone(),
                        control: RevocationControl::Observe,
                    })
                    .send()
                    .await?
                    .error_for_status()?
                    .json()
                    .await?;
                if matches!(
                    observed.operation.map(|operation| operation.state),
                    Some(durable_steps::OperationState::Completed { .. })
                ) {
                    break Ok::<(), Box<dyn std::error::Error + Send + Sync>>(());
                }
            }
        })
        .await??;
        let _ = admitted
            .socket
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(peer)
    }
    .await;
    let stopped = server.shutdown().await;
    let keys = jwks.shutdown().await;
    stopped?;
    keys?;
    let peer = proof?;

    // Open the actual physical mailbox after every server object has closed.
    // A missing dispatch instead creates a fresh open mailbox and fails below.
    let persisted = Fixture::open_named(
        root.path(),
        format!("mailbox-{}", service.as_str()),
        &service,
    )
    .await?;
    let actual = persisted
        .submit(peer, &service, ItemId::new(Uuid::now_v7())?)
        .await?;
    persisted.store.close().await?;
    assert_eq!(
        actual,
        revoked(),
        "completed Registry revoke left its mailbox open"
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn close_answers_only_its_saved_completion_and_keeps_unfetched_mail_refused_after_reopen()
-> TestResult {
    let root = tempfile::tempdir()?;
    let service = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400ec".to_owned())?;
    let peer = PeerId::new(Uuid::now_v7())?;
    let item = ItemId::new(Uuid::now_v7())?;
    let call = CallId {
        operation: OperationId::new(Uuid::now_v7())?,
        step: StepName::new("close_mailbox")?,
    };
    let bytes = close_call(&service, peer, &call)?;
    let first = Fixture::open(root.path(), &service).await?;

    assert!(
        matches!(
            first.runtime.on_call(bytes.clone()).await,
            Err(CallFailure::AnswerLost)
        ),
        "admission is not a saved close completion"
    );
    let before = first.submit(peer, &service, item).await?;
    assert_eq!(
        before["kind"], "accepted",
        "close ran before its alarm-owned step"
    );
    first.runtime.on_alarm().await?;
    // Observe admission immediately after Close's committed step, before another
    // alarm could repair the owed publication. This isolates the transaction invariant.
    let publication_keys = first
        .object
        .transaction(|scope| {
            Box::pin(async move {
                use durable_steps::JournalScope;
                scope
                    .running_request_keys("fleet-sync.delivery.publish-status")
                    .await
            })
        })
        .await?;
    let revoked_keys = publication_keys
        .iter()
        .map(|key| serde_json::from_str::<Value>(key.as_str()))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        revoked_keys
            .iter()
            .filter(|key| key.get("item") == Some(&json!(item))
                && key.get("version") == Some(&json!(2)))
            .count(),
        1,
        "Close committed revoked custody without admitting its newer publication"
    );
    assert_eq!(
        first
            .runtime
            .on_call(bytes.clone())
            .await
            .map_err(|error| format!("close receipt: {error:?}"))?,
        b"null"
    );
    let replayed = first.submit(peer, &service, item).await?;
    assert_eq!(replayed["kind"], "replayed");
    assert_eq!(
        replayed["value"]["status"],
        json!({"kind":"refused","value":{"reason":{"kind":"recipientRevoked"}}})
    );
    assert_eq!(
        first
            .submit(peer, &service, ItemId::new(Uuid::now_v7())?)
            .await?,
        revoked()
    );
    first.store.close().await?;
    drop(first);

    let reopened = Fixture::open(root.path(), &service).await?;
    assert_eq!(
        reopened
            .runtime
            .on_call(bytes)
            .await
            .map_err(|error| format!("reopened receipt: {error:?}"))?,
        b"null"
    );
    assert!(
        reopened.runtime.on_alarm().await?.completed.is_empty(),
        "completed close was re-executed"
    );
    assert_eq!(
        reopened
            .submit(peer, &service, ItemId::new(Uuid::now_v7())?)
            .await?,
        revoked()
    );
    assert_eq!(reopened.submit(peer, &service, item).await?, replayed);
    reopened.store.close().await?;
    Ok(())
}

#[path = "mailbox_lifecycle/submit.rs"]
mod submit;

#[path = "mailbox_lifecycle/relay.rs"]
mod relay;
#[path = "mailbox_lifecycle/retired_receipts.rs"]
mod retired_receipts;
#[path = "mailbox_lifecycle/succession.rs"]
mod succession;
