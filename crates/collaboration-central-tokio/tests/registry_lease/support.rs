//! Shared real Registry/Fleet storage; each case selects its explicit transport fault.

#[path = "../fleet_storage/support.rs"]
mod fleet_fixture;
pub(super) use fleet_fixture::Fixture as FleetFixture;

use collaboration_central_core::{
    CoreHost, ObjectAddress, ObjectRuntime, PlatformIo, RegistryKind,
};
use collaboration_central_tokio::TokioStorageHost;
use durable_steps::{BoxFuture, CallFailure};
use fleet_sync_lease::{LeasePolicy, RegistryConfig};
use fleet_sync_protocol::ReceiverId;
use fleet_sync_tokio_runner::{ObjectKey, ObjectStore};
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use serde_json::Value;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    time::Duration,
};

pub(super) type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub(super) type RegistryStore = fleet_sync_tokio_runner::TokioObject<CoreHost<RoutingIo>>;
pub(super) type RegistryRuntime =
    ObjectRuntime<RegistryKind, TokioStorageHost<CoreHost<RoutingIo>>, RoutingIo>;

pub(super) struct RegistryFixture {
    pub root: tempfile::TempDir,
    pub fleet: Arc<FleetFixture>,
    pub io: RoutingIo,
    pub config: RegistryConfig,
    pub objects: ObjectStore<CoreHost<RoutingIo>>,
    pub registry: Arc<RegistryStore>,
    pub runtime: RegistryRuntime,
}
impl RegistryFixture {
    pub async fn open(lose_first_grant_answer: bool) -> TestResult<Self> {
        let root = tempfile::tempdir()?;
        let fleet = Arc::new(FleetFixture::open(root.path()).await?);
        fleet.import(&fleet_fixture::bundle()?).await?;
        let io = RoutingIo {
            fleet: fleet.clone(),
            notice_delivery: Default::default(),
            calls: Default::default(),
            wakes: Default::default(),
            now: Arc::new(AtomicI64::new(1_792_281_600_000)),
            lose_grant_answer: Arc::new(AtomicBool::new(lose_first_grant_answer)),
            pause_grant: Arc::new(AtomicBool::new(false)),
            grant_entered: Arc::new(tokio::sync::Notify::new()),
            grant_release: Arc::new(tokio::sync::Notify::new()),
            lose_publication_answer: Arc::new(AtomicBool::new(false)),
            pause_publication: Arc::new(AtomicBool::new(false)),
            publication_entered: Arc::new(tokio::sync::Notify::new()),
            publication_release: Arc::new(tokio::sync::Notify::new()),
        };
        let config = RegistryConfig {
            lease: LeasePolicy {
                lease_duration: Duration::from_secs(60),
                renew_interval: Duration::from_secs(20),
            },
            authoritative_receiver: ReceiverId::new(durable_steps::Uuid::now_v7())?,
            release_unsent_for: Duration::from_secs(10),
            move_needs_renewal_within: None,
        };
        let objects = ObjectStore::open(root.path()).await?;
        let registry = objects
            .object(
                ObjectKey::new("registry")?,
                Default::default(),
                Arc::new(CoreHost::for_store(
                    io.clone(),
                    config.authoritative_receiver,
                )),
            )
            .await?;
        let runtime = ObjectRuntime::registry(
            Arc::new(TokioStorageHost::new(registry.clone())),
            io.clone(),
            config,
            collaboration_central_tokio::DeliveryConfig {
                relay_peer: fleet_sync_protocol::PeerId::try_from(
                    "019f0000-0000-7000-8000-000000005240".to_owned(),
                )?,
            },
        )?;
        runtime.initialize().await?;
        Ok(Self {
            root,
            fleet,
            io,
            config,
            objects,
            registry,
            runtime,
        })
    }
}

#[derive(Clone)]
pub(super) struct RoutingIo {
    fleet: Arc<FleetFixture>,
    pub notice_delivery: Arc<Mutex<Option<NoticeDelivery>>>,
    pub calls: Arc<Mutex<Vec<Value>>>,
    pub wakes: Arc<Mutex<Vec<SyncTime>>>,
    pub now: Arc<AtomicI64>,
    lose_grant_answer: Arc<AtomicBool>,
    pub pause_grant: Arc<AtomicBool>,
    pub grant_entered: Arc<tokio::sync::Notify>,
    pub grant_release: Arc<tokio::sync::Notify>,
    pub lose_publication_answer: Arc<AtomicBool>,
    pub pause_publication: Arc<AtomicBool>,
    pub publication_entered: Arc<tokio::sync::Notify>,
    pub publication_release: Arc<tokio::sync::Notify>,
}
pub(super) type NoticeDelivery = Arc<
    dyn Fn(ObjectAddress, Vec<u8>) -> BoxFuture<'static, Result<Vec<u8>, CallFailure>>
        + Send
        + Sync,
>;
impl Clock for RoutingIo {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.now.load(Ordering::SeqCst))
    }
}
impl IdSource for RoutingIo {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::now_v7()
    }
}
impl WakeScheduler for RoutingIo {
    async fn wake_at(&self, at: SyncTime) -> Result<(), WakeFailure> {
        self.wakes
            .lock()
            .map_err(|_| WakeFailure::Unavailable {
                message: "wake observation poisoned".into(),
            })?
            .push(at);
        Ok(())
    }
}
impl PlatformIo for RoutingIo {
    async fn object_call(
        &self,
        destination: ObjectAddress,
        bytes: Vec<u8>,
    ) -> Result<Vec<u8>, CallFailure> {
        if destination != ObjectAddress::Fleet {
            let deliver = self
                .notice_delivery
                .lock()
                .map_err(|_| CallFailure::AnswerLost)?
                .clone();
            return match deliver {
                Some(deliver) => deliver(destination, bytes).await,
                None => Err(CallFailure::AnswerLost),
            };
        }
        let call: Value = serde_json::from_slice(&bytes).map_err(|_| CallFailure::AnswerLost)?;
        self.calls
            .lock()
            .map_err(|_| CallFailure::AnswerLost)?
            .push(call.clone());
        if call.get("kind").and_then(Value::as_str) == Some("central.claim-granted")
            && self.pause_grant.swap(false, Ordering::SeqCst)
        {
            let released = self.grant_release.notified();
            self.grant_entered.notify_one();
            released.await;
        }
        if call.get("kind").and_then(Value::as_str) == Some("central.lease-projection")
            && self.pause_publication.swap(false, Ordering::SeqCst)
        {
            let released = self.publication_release.notified();
            self.publication_entered.notify_one();
            released.await;
        }
        let answer = self.fleet.runtime.on_call(bytes).await?;
        if call.get("kind").and_then(Value::as_str) == Some("central.claim-granted")
            && self.lose_grant_answer.swap(false, Ordering::SeqCst)
        {
            return Err(CallFailure::AnswerLost);
        }
        if call.get("kind").and_then(Value::as_str) == Some("central.lease-projection")
            && self.lose_publication_answer.swap(false, Ordering::SeqCst)
        {
            return Err(CallFailure::AnswerLost);
        }
        Ok(answer)
    }
}
