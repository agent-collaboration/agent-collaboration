use super::session_fixture;
use board_sync_protocol::{BoardNotice, BoardReadPage};
use collaboration_central_core::{CoreHost, FleetKind, ObjectAddress, ObjectRuntime, PlatformIo};
use collaboration_central_tokio::TokioStorageHost;
use durable_steps::{
    AtomicStore, CallFailure, NotSentReason, StoreError, storage::TransactionScope,
};
use fleet_admission::{FleetOperation, FleetRequest, FleetRequestResult};
use fleet_sync_protocol::ReceiverId;
use fleet_sync_tokio_runner::{ObjectKey, ObjectStore, TokioObject};
use message_board::*;
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
};
#[path = "fault_store.rs"]
mod fault_store;
pub use fault_store::{FaultHost, FaultStore};
pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub type Native = TokioObject<CoreHost<Io>>;
pub const SERVICE: &str = "550e8400-e29b-41d4-a716-446655440497";
pub const WORKSPACE: &str = "019f0000-0000-7000-8000-000000004970";
pub fn installation() -> TestResult<ServiceId> {
    Ok(ServiceId::try_from(SERVICE.to_owned())?)
}

#[derive(Clone)]
pub struct Io {
    pub delivery: Arc<session_fixture::Fixture>,
    pub lose_before_delivery: Arc<AtomicBool>,
    pub lose_after_acknowledgement: Arc<AtomicBool>,
    time: Arc<AtomicI64>,
    calls: Arc<Mutex<Vec<Value>>>,
    wakes: Arc<Mutex<Vec<SyncTime>>>,
}
impl Io {
    pub fn new(delivery: Arc<session_fixture::Fixture>) -> Self {
        Self {
            delivery,
            lose_before_delivery: Arc::default(),
            lose_after_acknowledgement: Arc::default(),
            time: Arc::new(AtomicI64::new(1_792_281_600_000)),
            calls: Arc::default(),
            wakes: Arc::default(),
        }
    }
    pub fn advance(&self) {
        self.time.fetch_add(60_000, Ordering::SeqCst);
    }
    pub fn age_retention(&self) {
        self.time
            .fetch_add(31 * 24 * 60 * 60 * 1000, Ordering::SeqCst);
    }
    pub fn calls(&self) -> TestResult<Vec<Value>> {
        Ok(self
            .calls
            .lock()
            .map_err(|_| "call mutex poisoned")?
            .clone())
    }
    pub fn wakes(&self) -> TestResult<Vec<SyncTime>> {
        Ok(self
            .wakes
            .lock()
            .map_err(|_| "wake mutex poisoned")?
            .clone())
    }
}
impl Clock for Io {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.time.load(Ordering::SeqCst))
    }
}
impl IdSource for Io {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::now_v7()
    }
}
impl WakeScheduler for Io {
    async fn wake_at(&self, at: SyncTime) -> Result<(), WakeFailure> {
        self.wakes
            .lock()
            .map_err(|_| WakeFailure::Unavailable {
                message: "wake mutex poisoned".into(),
            })?
            .push(at);
        Ok(())
    }
}
impl PlatformIo for Io {
    async fn object_call(
        &self,
        destination: ObjectAddress,
        bytes: Vec<u8>,
    ) -> Result<Vec<u8>, CallFailure> {
        let call: Value = serde_json::from_slice(&bytes).map_err(|_| CallFailure::AnswerLost)?;
        if destination
            != ObjectAddress::Connection(installation().map_err(|_| CallFailure::AnswerLost)?)
            || call.get("kind").and_then(Value::as_str) != Some("central.machine-notice")
            || call.get("source") != Some(&json!({"kind":"fleet"}))
        {
            return Err(CallFailure::NotSent {
                reason: NotSentReason::Overloaded,
            });
        }
        self.calls
            .lock()
            .map_err(|_| CallFailure::AnswerLost)?
            .push(call);
        if self.lose_before_delivery.swap(false, Ordering::SeqCst) {
            return Err(CallFailure::NotSent {
                reason: NotSentReason::Unreachable { last_seen: None },
            });
        }
        let answer = self
            .delivery
            .runtime
            .delivery()
            .map_err(|_| CallFailure::AnswerLost)?
            .on_call(bytes)
            .await;
        if answer.is_ok()
            && self
                .lose_after_acknowledgement
                .swap(false, Ordering::SeqCst)
        {
            return Err(CallFailure::AnswerLost);
        }
        answer
    }
}
pub struct Fixture {
    pub objects: ObjectStore<CoreHost<Io>>,
    pub object: Arc<Native>,
    pub host: Arc<FaultHost>,
    pub runtime: ObjectRuntime<FleetKind, FaultHost, Io>,
    pub io: Io,
}
impl Fixture {
    pub async fn open(root: &Path, io: Io) -> TestResult<Self> {
        let receiver = ReceiverId::new(durable_steps::Uuid::now_v7())?;
        let objects = ObjectStore::open(root).await?;
        let object = objects
            .object(
                ObjectKey::new("fleet")?,
                Default::default(),
                Arc::new(CoreHost::for_store(io.clone(), receiver)),
            )
            .await?;
        let host = Arc::new(FaultHost {
            native: Arc::new(TokioStorageHost::new(object.clone())),
            store: Arc::new(FaultStore {
                object: object.clone(),
                armed: Arc::default(),
                injected: Arc::default(),
            }),
        });
        let runtime = ObjectRuntime::fleet(host.clone(), io.clone(), receiver)?;
        runtime.initialize().await?;
        Ok(Self {
            objects,
            object,
            host,
            runtime,
            io,
        })
    }
    pub async fn feed(&self, after: u64) -> TestResult<BoardReadPage> {
        self.feed_from(SERVICE, after).await
    }
    pub async fn feed_from(&self, installation: &str, after: u64) -> TestResult<BoardReadPage> {
        let bytes=self.runtime.on_call(serde_json::to_vec(&json!({
            "source":{"kind":"connection","value":installation},"sourceFace":"session","authority":{"kind":"object"},
            "destination":{"kind":"fleet"},"destinationFace":"fleet", "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"read_feed"},
            "kind":"central.fleet-read","target":null,"request":{"kind":"feed","value":{"after":after}}
        }))?).await.map_err(|error|format!("feed read failed: {error:?}"))?;
        Ok(serde_json::from_slice(&bytes)?)
    }
    pub async fn decide(&self, request: &FleetRequest) -> TestResult<FleetRequestResult> {
        let bytes = self
            .runtime
            .on_call(mutation(request)?)
            .await
            .map_err(|error| format!("mutation failed: {error:?}"))?;
        Ok(serde_json::from_slice(&bytes)?)
    }
    pub async fn deliver(&self) -> TestResult {
        let admitted = self.runtime.on_alarm().await?;
        assert_eq!(admitted.retried.len(), 1);
        self.io.delivery.runtime.on_alarm().await?;
        self.io.advance();
        assert_eq!(self.runtime.on_alarm().await?.completed.len(), 1);
        Ok(())
    }
    pub async fn counts(&self) -> TestResult<(i64, i64)> {
        Ok(self.object.transaction(|scope|Box::pin(async move {
            let row=query::<sqlx_turso::Turso>("SELECT (SELECT count(*) FROM workspaces) AS workspaces,(SELECT count(*) FROM fleet_feed) AS feed").fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            Ok((row.try_get("workspaces").map_err(StoreError::backend)?,row.try_get("feed").map_err(StoreError::backend)?))
        })).await?)
    }
    pub async fn reader_rows(&self) -> TestResult<Vec<(String, String, i64)>> {
        Ok(self.object.transaction(|scope|Box::pin(async move {
            query::<sqlx_turso::Turso>("SELECT reader_id,installation,last_read_position FROM feed_readers ORDER BY installation").fetch_all(scope.executor()).await.map_err(StoreError::backend)?.into_iter().map(|row|Ok((row.try_get("reader_id").map_err(StoreError::backend)?,row.try_get("installation").map_err(StoreError::backend)?,row.try_get("last_read_position").map_err(StoreError::backend)?))).collect()
        })).await?)
    }
}
pub fn offered(operation: FleetOperation) -> TestResult<FleetRequest> {
    Ok(FleetRequest {
        request_id: RequestId::try_from(durable_steps::Uuid::now_v7().to_string())?,
        workspace: WorkspaceId::try_from(WORKSPACE.to_owned())?,
        actor: RecordedActor::Human {
            human_id: HumanId::try_from("notice owner".to_owned())?,
        },
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation,
    })
}
pub fn workspace_create() -> TestResult<FleetRequest> {
    offered(FleetOperation::WorkspaceCreate {
        name: ResourceName::try_from("Outbox workspace".to_owned())?,
        description: Description::try_from("Real Fleet callback".to_owned())?,
    })
}
pub fn workspace_update() -> TestResult<FleetRequest> {
    offered(FleetOperation::WorkspaceUpdate {
        name: ResourceName::try_from("Updated outbox workspace".to_owned())?,
        description: Description::try_from("A later committed prefix".to_owned())?,
    })
}
pub fn mutation(request: &FleetRequest) -> TestResult<Vec<u8>> {
    Ok(serde_json::to_vec(&json!({
        "source":{"kind":"connection","value":SERVICE},"sourceFace":"session","authority":{"kind":"object"},
        "destination":{"kind":"fleet"},"destinationFace":"fleet","identity":{"operation":durable_steps::Uuid::now_v7(),"step":"mutate"},
        "kind":"central.fleet-metadata","target":null,"request":{"request":request,"origin":{"installation":SERVICE,"machine":"notice owner"}}
    }))?)
}
pub fn assert_accepted(answer: &FleetRequestResult) -> TestResult {
    if !matches!(
        answer,
        FleetRequestResult::Decided(RequestDecision::Accepted {
            change: ChangeOutcome::Changed,
            ..
        })
    ) {
        return Err(format!("fixture not accepted: {answer:?}").into());
    }
    Ok(())
}
pub async fn notice_rows(fixture: &session_fixture::Fixture) -> TestResult<i64> {
    Ok(i64::try_from(notice_kinds(fixture).await?.len())?)
}
pub async fn notice_kinds(fixture: &session_fixture::Fixture) -> TestResult<Vec<BoardNotice>> {
    Ok(fixture
        .host
        .store
        .object
        .transaction(|scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>("SELECT kind FROM machine_notices ORDER BY position")
                    .fetch_all(scope.executor())
                    .await
                    .map_err(StoreError::backend)?
                    .into_iter()
                    .map(|row| {
                        let payload: String = row.try_get("kind").map_err(StoreError::backend)?;
                        serde_json::from_str(&payload).map_err(StoreError::backend)
                    })
                    .collect()
            })
        })
        .await?)
}
