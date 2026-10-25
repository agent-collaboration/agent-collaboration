use collaboration_central_core::{
    AccessCaller, CoreHost, FleetKind, FleetResult, ObjectAddress, ObjectRuntime, OwnerIdentity,
    PlatformIo,
};
use collaboration_central_tokio::TokioStorageHost;
use durable_steps::{
    AtomicStore, CallFailure, NotSentReason, StoreError, storage::TransactionScope,
};
use fleet_admission::{FleetImportBundle, FleetImportResult};
use fleet_sync_protocol::ReceiverId;
use fleet_sync_tokio_runner::{ObjectKey, ObjectStore, TokioObject};
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use serde_json::json;
use sqlx_core::{query::query, row::Row};
use std::{path::Path, sync::Arc};

pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
#[derive(Clone, Copy)]
pub struct Io(SyncTime);
impl Default for Io {
    fn default() -> Self {
        Self(SyncTime::from_unix_millis(1_792_281_600_000))
    }
}
impl Clock for Io {
    fn now(&self) -> SyncTime {
        self.0
    }
}
impl IdSource for Io {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::now_v7()
    }
}
impl WakeScheduler for Io {
    async fn wake_at(&self, _: SyncTime) -> Result<(), WakeFailure> {
        Ok(())
    }
}
impl PlatformIo for Io {
    async fn object_call(&self, _: ObjectAddress, _: Vec<u8>) -> Result<Vec<u8>, CallFailure> {
        Err(CallFailure::NotSent {
            reason: NotSentReason::Unreachable { last_seen: None },
        })
    }
}

pub struct Fixture {
    pub receiver: ReceiverId,
    pub objects: ObjectStore<CoreHost<Io>>,
    pub object: Arc<TokioObject<CoreHost<Io>>>,
    pub(super) runtime: ObjectRuntime<FleetKind, TokioStorageHost<CoreHost<Io>>, Io>,
}
impl Fixture {
    pub async fn fleet_request(
        &self,
        request: &fleet_admission::FleetRequest,
    ) -> TestResult<fleet_admission::FleetRequestResult> {
        self.fleet_request_at(request, durable_steps::Uuid::now_v7(), "fleet_fixture")
            .await
    }
    pub async fn fleet_request_at(
        &self,
        request: &fleet_admission::FleetRequest,
        operation: durable_steps::Uuid,
        step: &str,
    ) -> TestResult<fleet_admission::FleetRequestResult> {
        self.fleet_request_from(
            request,
            operation,
            step,
            message_board::ActorOrigin {
                installation: message_board::ServiceId::try_from(
                    "550e8400-e29b-41d4-a716-4466554400a9".to_owned(),
                )?,
                machine: message_board::MachineLabel::try_from("live router".to_owned())?,
            },
        )
        .await
    }
    pub async fn fleet_request_from(
        &self,
        request: &fleet_admission::FleetRequest,
        operation: durable_steps::Uuid,
        step: &str,
        origin: message_board::ActorOrigin,
    ) -> TestResult<fleet_admission::FleetRequestResult> {
        let bytes = self.runtime.on_call(serde_json::to_vec(&json!({
            "source":{"kind":"connection","value":"550e8400-e29b-41d4-a716-4466554400a9"},"sourceFace":"session","authority":{"kind":"object"},
            "destination":{"kind":"fleet"},"destinationFace":"fleet",
            "identity":{"operation":operation,"step":step},
            "kind":"central.fleet-metadata","target":null,"request":{
                "request":request,"origin":origin
            }
        }))?).await.map_err(|error| format!("Fleet decision did not answer: {error:?}"))?;
        Ok(serde_json::from_slice(&bytes)?)
    }
    pub async fn open(root: &Path) -> TestResult<Self> {
        Self::open_with(root, true, Io::default()).await
    }
    pub async fn open_at(root: &Path, now: SyncTime) -> TestResult<Self> {
        Self::open_with(root, true, Io(now)).await
    }
    pub async fn before_fleet_migrations(root: &Path) -> TestResult<Self> {
        Self::open_with(root, false, Io::default()).await
    }
    pub async fn initialize(&self) -> TestResult<usize> {
        Ok(self.runtime.initialize().await?)
    }
    async fn open_with(root: &Path, initialize: bool, io: Io) -> TestResult<Self> {
        let objects = ObjectStore::open(root).await?;
        let receiver = ReceiverId::new(durable_steps::Uuid::now_v7())?;
        let object = objects
            .object(
                ObjectKey::new("fleet")?,
                Default::default(),
                Arc::new(CoreHost::for_store(io, receiver)),
            )
            .await?;
        let runtime = ObjectRuntime::fleet(
            Arc::new(TokioStorageHost::new(object.clone())),
            io,
            receiver,
        )?;
        if initialize {
            runtime.initialize().await?;
        }
        Ok(Self {
            receiver,
            objects,
            object,
            runtime,
        })
    }
    pub async fn import(&self, offered: &FleetImportBundle) -> TestResult<FleetImportResult> {
        let call = import_bytes(offered)?;
        let bytes = self
            .runtime
            .on_call(call)
            .await
            .map_err(|error| format!("Fleet import did not answer: {error:?}"))?;
        let FleetResult::Imported(result) = serde_json::from_slice(&bytes)?;
        Ok(result)
    }
    pub async fn counts(&self) -> TestResult<Vec<i64>> {
        Ok(self.object.transaction(|scope| Box::pin(async move {
            let row = query::<sqlx_turso::Turso>("SELECT (SELECT count(*) FROM workspaces) AS workspaces,(SELECT count(*) FROM boards) AS boards,(SELECT count(*) FROM project_directory) AS directory,(SELECT count(*) FROM repository_attachments) AS repositories,(SELECT count(*) FROM fleet_feed) AS feed")
                .fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            ["workspaces","boards","directory","repositories","feed"].into_iter()
                .map(|name| row.try_get(name).map_err(StoreError::backend)).collect::<Result<Vec<i64>,_>>()
        })).await?)
    }
    pub async fn assert_import_headers(&self) -> TestResult {
        let (time, basis, origin, machine, digest, payload) = self.object.transaction(|scope| Box::pin(async move {
            let row = query::<sqlx_turso::Turso>("SELECT router_time,time_basis,origin_installation,origin_machine,import_digest,payload FROM workspaces")
                .fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            Ok((row.try_get::<Option<String>,_>("router_time").map_err(StoreError::backend)?,
                row.try_get::<String,_>("time_basis").map_err(StoreError::backend)?,
                row.try_get::<String,_>("origin_installation").map_err(StoreError::backend)?,
                row.try_get::<String,_>("origin_machine").map_err(StoreError::backend)?,
                row.try_get::<Option<String>,_>("import_digest").map_err(StoreError::backend)?,
                row.try_get::<String,_>("payload").map_err(StoreError::backend)?))
        })).await?;
        assert_eq!(time, None);
        assert_eq!(basis, "legacy_unknown");
        assert_eq!(origin, "550e8400-e29b-41d4-a716-4466554400a8");
        assert_eq!(machine, "legacy importer");
        assert!(digest.is_some_and(|digest| digest.len() == 64));
        let record: message_board::Record = serde_json::from_str(&payload)?;
        assert_eq!(
            record.envelope().time,
            message_board::RecordTime::LegacyUnknown
        );
        Ok(())
    }
}

pub fn bundle() -> TestResult<FleetImportBundle> {
    Ok(serde_json::from_value(json!({"provenance":{
        "installation":"550e8400-e29b-41d4-a716-4466554400a8","machine":"legacy importer"
    },"declaredOwner":"owner","records":[
        {"kind":"workspace","id":"019f0000-0000-7000-8000-000000000a81","name":"Imported fleet","description":"legacy", "repositories":[{"kind":"origin","normalizedOrigin":"github.com/example/repository"}],"time":{"kind":"legacyUnknown"}},
        {"kind":"board","id":"019f0000-0000-7000-8000-000000000a82","workspace":"019f0000-0000-7000-8000-000000000a81","name":"Imported board","description":"legacy","state":"active","time":{"kind":"router","value":"2026-10-18T10:00:00Z"}},
        {"kind":"directory","project":"019f0000-0000-7000-8000-000000000a83","board":"019f0000-0000-7000-8000-000000000a82","title":"Imported project","time":{"kind":"legacyUnknown"}}
    ]}))?)
}

pub fn import_bytes(offered: &FleetImportBundle) -> TestResult<Vec<u8>> {
    let owner = OwnerIdentity::from_access(AccessCaller::Owner {
        email: "fixture@example.test".into(),
    })?;
    Ok(serde_json::to_vec(&json!({
        "source":{"kind":"registry"},"sourceFace":"edge","authority":{"kind":"owner","value":owner},
        "destination":{"kind":"fleet"},"destinationFace":"fleet",
        "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"import_fleet"},
        "kind":"central.object","target":null,"request":{
            "kind":"fleet","value":{"kind":"import","value":{"bundle":offered}}
        }
    }))?)
}
