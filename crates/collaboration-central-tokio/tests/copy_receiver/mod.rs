use collaboration_central_core::{
    CopyKind, CoreHost, FleetKind, ObjectAddress, ObjectRuntime, PlatformIo,
};
use collaboration_central_tokio::TokioStorageHost;
use durable_steps::{
    AtomicStore, CallFailure, CallId, NotSentReason, StoreError, storage::TransactionScope,
};
use fleet_sync_protocol::{
    DatabaseId, Epoch, Piece, ReceiverId, ReplicationAnswer, ReplicationPosition,
};
use fleet_sync_tokio_runner::{ObjectKey, ObjectOptions, ObjectStore, TokioObject};
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};
use std::{
    path::Path,
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
};

pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
type Fleet = ObjectRuntime<FleetKind, TokioStorageHost<CoreHost<Io>>, Io>;

#[derive(Clone)]
pub struct Io {
    fleet: Arc<OnceLock<Weak<Fleet>>>,
    pub time: Arc<AtomicI64>,
    pub lose_answer: Arc<AtomicBool>,
    pub calls: Arc<Mutex<Vec<CallId>>>,
    pub wakes: Arc<Mutex<Vec<SyncTime>>>,
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
                message: "wake observation poisoned".into(),
            })?
            .push(at);
        Ok(())
    }
}
impl PlatformIo for Io {
    async fn object_call(
        &self,
        destination: ObjectAddress,
        body: Vec<u8>,
    ) -> Result<Vec<u8>, CallFailure> {
        if destination != ObjectAddress::Fleet {
            return Err(no_route());
        }
        let call: Value = serde_json::from_slice(&body).map_err(|_| CallFailure::AnswerLost)?;
        self.calls
            .lock()
            .map_err(|_| CallFailure::AnswerLost)?
            .push(
                serde_json::from_value(
                    call.get("identity")
                        .cloned()
                        .ok_or(CallFailure::AnswerLost)?,
                )
                .map_err(|_| CallFailure::AnswerLost)?,
            );
        let runtime = self
            .fleet
            .get()
            .and_then(Weak::upgrade)
            .ok_or_else(no_route)?;
        let answer = runtime.on_call(body).await?;
        if self.lose_answer.swap(false, Ordering::SeqCst) {
            return Err(CallFailure::AnswerLost);
        }
        Ok(answer)
    }
}
fn no_route() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}

pub struct Fixture {
    pub objects: ObjectStore<CoreHost<Io>>,
    pub copy: Arc<TokioObject<CoreHost<Io>>>,
    pub fleet: Arc<TokioObject<CoreHost<Io>>>,
    pub runtime: ObjectRuntime<CopyKind, TokioStorageHost<CoreHost<Io>>, Io>,
    _fleet_runtime: Arc<Fleet>,
    pub database: DatabaseId,
    pub receiver: ReceiverId,
    pub io: Io,
}
impl Fixture {
    pub async fn open(root: &Path) -> TestResult<Self> {
        let database = DatabaseId::try_from("019f0000-0000-7000-8000-000000000a83".to_owned())?;
        let receiver = ReceiverId::try_from("019f0000-0000-7000-8000-000000000010".to_owned())?;
        let io = Io {
            fleet: Arc::default(),
            time: Arc::new(AtomicI64::new(1_792_281_600_000)),
            lose_answer: Arc::default(),
            calls: Arc::default(),
            wakes: Arc::default(),
        };
        let objects = ObjectStore::open(root).await?;
        let fleet = objects
            .object(
                ObjectKey::new("fleet")?,
                Default::default(),
                Arc::new(CoreHost::for_store(io.clone(), receiver)),
            )
            .await?;
        let fleet_runtime = Arc::new(ObjectRuntime::fleet(
            Arc::new(TokioStorageHost::new(fleet.clone())),
            io.clone(),
            receiver,
        )?);
        fleet_runtime.initialize().await?;
        io.fleet
            .set(Arc::downgrade(&fleet_runtime))
            .map_err(|_| "route already set")?;
        fleet_runtime.on_call(serde_json::to_vec(&json!({
            "source":{"kind":"registry"},"sourceFace":"edge","authority":{"kind":"owner","value":"fixture@example.test"},
            "destination":{"kind":"fleet"},"destinationFace":"fleet","identity":{"operation":durable_steps::Uuid::now_v7(),"step":"import"},
            "kind":"central.object","target":null,"request":{"kind":"fleet","value":{"kind":"import","value":{"bundle":{
                "provenance":{"installation":"550e8400-e29b-41d4-a716-4466554400a8","machine":"copy fixture"},"declaredOwner":"owner","records":[
                    {"kind":"workspace","id":"019f0000-0000-7000-8000-000000000a81","name":"copy workspace","description":"copy fixture","repositories":[],"time":{"kind":"legacyUnknown"}},
                    {"kind":"board","id":"019f0000-0000-7000-8000-000000000a82","workspace":"019f0000-0000-7000-8000-000000000a81","name":"copy board","description":"copy fixture","state":"active","time":{"kind":"legacyUnknown"}},
                    {"kind":"directory","project":database,"board":"019f0000-0000-7000-8000-000000000a82","title":"copy project","time":{"kind":"legacyUnknown"}}
                ]
            }}}}
        }))?).await.map_err(|error|format!("Fleet setup failed: {error:?}"))?;
        let copy = objects
            .object(
                ObjectKey::new("copy")?,
                ObjectOptions {
                    foreign_keys: false,
                },
                Arc::new(CoreHost::for_store(io.clone(), receiver)),
            )
            .await?;
        let runtime = ObjectRuntime::copy(
            Arc::new(TokioStorageHost::new(copy.clone())),
            io.clone(),
            receiver,
            database,
            std::time::Duration::from_secs(2),
        );
        runtime.initialize().await?;
        Ok(Self {
            objects,
            copy,
            fleet,
            runtime,
            _fleet_runtime: fleet_runtime,
            database,
            receiver,
            io,
        })
    }
    pub async fn create(&self, seed: bool) -> TestResult {
        self.create_as(seed, false).await
    }
    pub async fn create_as(&self, seed: bool, admin: bool) -> TestResult {
        self.runtime.on_call(serde_json::to_vec(&json!({
            "source":{"kind":"registry"},"sourceFace":"registry","authority":{"kind":"object"},
            "destination":{"kind":"copy","value":self.database},"destinationFace":"copy","identity":{"operation":durable_steps::Uuid::now_v7(),"step":"create"},
            "kind":"fleet-sync.copy-create","target":{"copy":self.receiver},"request":{
                "database":self.database,"epoch":1,"holder":if admin {None} else {Some("019f0000-0000-7000-8000-000000000901")},
                "origin":if seed {json!({"kind":"awaitingSeed","value":{"expectedPosition":null}})} else {json!({"kind":"empty"})}
            }
        }))?).await.map_err(|error|format!("Copy setup failed: {error:?}"))?;
        Ok(())
    }
    pub async fn apply(
        &self,
        piece: Piece<board_sync_protocol::BoardAnnotation>,
    ) -> TestResult<ReplicationAnswer> {
        let response=self.runtime.on_call(serde_json::to_vec(&json!({
            "source":{"kind":"connection","value":"550e8400-e29b-41d4-a716-4466554400a8"},"sourceFace":"session","authority":{"kind":"object"},
            "destination":{"kind":"copy","value":self.database},"destinationFace":"copy","identity":{"operation":durable_steps::Uuid::now_v7(),"step":"apply"},
            "kind":"central.copy-session-write","target":{"copy":self.receiver,"database":self.database},"request":{"sender":"019f0000-0000-7000-8000-000000000901","request":{"kind":"piece","value":piece}}
        }))?).await.map_err(|error|format!("Copy apply did not answer: {error:?}"))?;
        Ok(serde_json::from_slice(&response)?)
    }
    pub async fn seed_call(&self, admin: bool, request: Value) -> TestResult<ReplicationAnswer> {
        let call = if admin {
            json!({
                "source":{"kind":"registry"},"sourceFace":"edge","authority":{"kind":"owner","value":"fixture@example.test"},
                "destination":{"kind":"copy","value":self.database},"destinationFace":"copy","identity":{"operation":durable_steps::Uuid::now_v7(),"step":"seed"},
                "kind":"central.copy-admin-seed","target":{"copy":self.receiver,"database":self.database},"request":request
            })
        } else {
            json!({
                "source":{"kind":"connection","value":"550e8400-e29b-41d4-a716-4466554400a8"},"sourceFace":"session","authority":{"kind":"object"},
                "destination":{"kind":"copy","value":self.database},"destinationFace":"copy","identity":{"operation":durable_steps::Uuid::now_v7(),"step":"seed"},
                "kind":"central.copy-session-write","target":{"copy":self.receiver,"database":self.database},"request":{"sender":"019f0000-0000-7000-8000-000000000901","request":request}
            })
        };
        let response = self
            .runtime
            .on_call(serde_json::to_vec(&call)?)
            .await
            .map_err(|error| format!("seed call failed: {error:?}"))?;
        Ok(serde_json::from_slice(&response)?)
    }
    pub async fn prepare_seed(&self, admin: bool) -> TestResult<fleet_sync_protocol::SeedComplete> {
        use fleet_sync_protocol::*;
        self.create_as(true, admin).await?;
        let attempt = SeedAttemptId::new(durable_steps::Uuid::now_v7())?;
        let kind = if admin { "begin" } else { "seedBegin" };
        let ReplicationAnswer::SeedStarted { seed } = self
            .seed_call(
                admin,
                json!({"kind":kind,"value":{"database":self.database,"attempt":attempt}}),
            )
            .await?
        else {
            return Err("seed did not start".into());
        };
        let sql = "CREATE TABLE board_rows(id TEXT NOT NULL PRIMARY KEY,value TEXT NOT NULL)";
        let schema = SchemaIdentity::new(
            1,
            schema_fingerprint(&[SchemaEntry {
                kind: SchemaObjectKind::Table,
                name: "board_rows".into(),
                sql: sql.into(),
            }])?,
        )?;
        let rows = vec![vec![
            SqlValue::Text("019f0000-0000-7000-8000-000000000b01".into()),
            SqlValue::Text("seeded row".into()),
        ]];
        let mut digest = TableDigestBuilder::new();
        for row in &rows {
            digest.push_row(row)?;
        }
        let contents = SnapshotPage {
            database: self.database,
            position: position(1)?,
            schema: schema.clone(),
            schema_steps: vec![sql.into()],
            table: "board_rows".into(),
            rows,
            next: None,
        };
        let page = SeedPage {
            seed,
            page: 0,
            digest: PageDigest::of(&contents)?,
            contents,
        };
        let kind = if admin { "page" } else { "seedPage" };
        if self
            .seed_call(admin, json!({"kind":kind,"value":page}))
            .await?
            != ReplicationAnswer::SeedPage(SeedPageAnswer::Staged)
        {
            return Err("seed page was not staged".into());
        }
        Ok(SeedComplete {
            database: self.database,
            seed,
            position: position(1)?,
            pages: 1,
            schema,
            table_digests: vec![(TableName::new("board_rows")?, digest.finish())],
        })
    }
    pub async fn complete_seed(
        &self,
        admin: bool,
        done: &fleet_sync_protocol::SeedComplete,
        summary: Option<board_sync_protocol::BoardAnnotation>,
    ) -> TestResult<ReplicationAnswer> {
        let request = if admin {
            json!({"kind":"complete","value":{"done":done,"summary":summary}})
        } else {
            json!({"kind":"seedComplete","value":done})
        };
        self.seed_call(admin, request).await
    }
    pub async fn copy_sql(&self, sql: &'static str) -> TestResult {
        self.copy
            .transaction(move |scope| {
                Box::pin(async move {
                    sqlx_core::raw_sql::raw_sql(sqlx_core::sql_str::AssertSqlSafe(sql))
                        .execute(scope.executor())
                        .await
                        .map_err(StoreError::backend)?;
                    Ok(())
                })
            })
            .await?;
        Ok(())
    }
    pub async fn counts(&self) -> TestResult<(i64, i64, i64)> {
        Ok(self.copy.transaction(|scope|Box::pin(async move {
            let row=query::<sqlx_turso::Turso>("SELECT (SELECT count(*) FROM sqlite_schema WHERE name='board_rows') AS tables,(SELECT count(*) FROM _copy_summary) AS summaries,(SELECT count(*) FROM _fleet_sync_operations) AS operations").fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            Ok((row.try_get("tables").map_err(StoreError::backend)?,row.try_get("summaries").map_err(StoreError::backend)?,row.try_get("operations").map_err(StoreError::backend)?))
        })).await?)
    }
    pub async fn summary(&self, directory: bool) -> TestResult<Option<Value>> {
        let object = if directory { &self.fleet } else { &self.copy };
        let result = object
            .transaction(move |scope| {
                Box::pin(async move {
                    let sql = if directory {
                        "SELECT copy_summary AS summary FROM project_directory"
                    } else {
                        "SELECT record AS summary FROM _copy_summary"
                    };
                    let row = query::<sqlx_turso::Turso>(sql)
                        .fetch_optional(scope.executor())
                        .await
                        .map_err(StoreError::backend)?;
                    row.map(|row| {
                        row.try_get::<Option<String>, _>("summary")
                            .map_err(StoreError::backend)
                    })
                    .transpose()
                    .map(Option::flatten)
                })
            })
            .await?;
        Ok(result
            .map(|value| serde_json::from_str(&value))
            .transpose()?)
    }
}
pub fn position(commit: u64) -> TestResult<ReplicationPosition> {
    Ok(ReplicationPosition::new(
        Epoch::new(1)?,
        fleet_sync_protocol::CommitSequence::new(commit),
    ))
}
