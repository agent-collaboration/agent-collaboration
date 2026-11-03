//! Real Fleet and project owners with a manual injected clock and explicit alarm driving.
use collaboration_central_core::{
    CopyKind, CoreHost, FleetKind, ObjectAddress, ObjectRuntime, PlatformIo,
};
use collaboration_central_tokio::TokioStorageHost;
use durable_steps::{
    AtomicStore, BoxFuture, CallFailure, NotSentReason, StoreError, storage::TransactionScope,
};
use fleet_sync_protocol::{DatabaseId, PeerId, ReceiverId};
use fleet_sync_tokio_runner::{ObjectKey, ObjectOptions, ObjectStore, TokioObject};
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use serde_json::{Value, json};
use sqlx_core::{executor::Executor, query::query, row::Row};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
};
pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub const PROJECT: &str = "019f0000-0000-7000-8000-000000008199";
pub const A: &str = "019f0000-0000-7000-8000-000000008111";
pub const B: &str = "019f0000-0000-7000-8000-000000008112";
pub const C: &str = "019f0000-0000-7000-8000-000000008113";
pub const SERVICE_A: &str = "019f0000-0000-7000-8000-000000008101";
pub const SERVICE_B: &str = "019f0000-0000-7000-8000-000000008102";
pub const SERVICE_C: &str = "019f0000-0000-7000-8000-000000008103";
pub const NOW: i64 = 1_792_281_600_000;
type Route = Arc<dyn Fn(Vec<u8>) -> BoxFuture<'static, Result<Vec<u8>, CallFailure>> + Send + Sync>;
#[derive(Clone)]
pub struct Io {
    pub now: Arc<AtomicI64>,
    routes: Arc<Mutex<Option<(Route, Route)>>>,
    wakes: Arc<Mutex<Vec<SyncTime>>>,
    time_changed: tokio::sync::watch::Sender<i64>,
    pub reply_deadlines: tokio::sync::watch::Sender<usize>,
}
impl Io {
    pub fn advance(&self, millis: i64) {
        let now = self.now.fetch_add(millis, Ordering::SeqCst) + millis;
        self.time_changed.send_replace(now);
    }
}
impl Clock for Io {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.now.load(Ordering::SeqCst))
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
                message: "fixture wake owner poisoned".into(),
            })?
            .push(at);
        Ok(())
    }
}
impl PlatformIo for Io {
    #[expect(
        clippy::expect_used,
        reason = "The fixture retains its injected clock for every active call."
    )]
    fn reply_deadline(
        &self,
        remaining: std::time::Duration,
    ) -> impl std::future::Future<Output = ()> + Send {
        let deadline = self.now().after(remaining);
        let mut changed = self.time_changed.subscribe();
        let now = self.now.clone();
        let started = self.reply_deadlines.clone();
        async move {
            started.send_modify(|count| *count += 1);
            while now.load(Ordering::SeqCst) < deadline.unix_millis() {
                changed
                    .changed()
                    .await
                    .expect("fixture clock remains owned");
            }
        }
    }

    async fn object_call(
        &self,
        destination: ObjectAddress,
        bytes: Vec<u8>,
    ) -> Result<Vec<u8>, CallFailure> {
        let (fleet, project) = self
            .routes
            .lock()
            .map_err(|_| no_route())?
            .clone()
            .ok_or_else(no_route)?;
        match destination {
            ObjectAddress::Fleet => fleet(bytes).await,
            ObjectAddress::Copy(database) if database.to_string() == PROJECT => {
                project(bytes).await
            }
            _ => Err(no_route()),
        }
    }
}
fn no_route() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
type Native = TokioObject<CoreHost<Io>>;
type Host = TokioStorageHost<CoreHost<Io>>;
type Fleet = ObjectRuntime<FleetKind, Host, Io>;
type Copy = ObjectRuntime<CopyKind, Host, Io>;
pub struct Fixture {
    objects: ObjectStore<CoreHost<Io>>,
    pub store: Arc<Native>,
    pub host: Arc<Host>,
    pub runtime: Arc<Fleet>,
    _project: Arc<Copy>,
    pub io: Io,
}
impl Fixture {
    pub async fn open(path: &Path, now: i64) -> TestResult<Self> {
        let io = Io {
            now: Arc::new(AtomicI64::new(now)),
            routes: Arc::default(),
            wakes: Arc::default(),
            time_changed: tokio::sync::watch::channel(now).0,
            reply_deadlines: tokio::sync::watch::channel(0).0,
        };
        let receiver = ReceiverId::try_from("019f0000-0000-7000-8000-000000000010".to_owned())?;
        let objects = ObjectStore::open(path).await?;
        let store = objects
            .object(
                ObjectKey::new("fleet")?,
                Default::default(),
                Arc::new(CoreHost::for_store(io.clone(), receiver)),
            )
            .await?;
        let host = Arc::new(Host::new(store.clone()));
        let runtime = Arc::new(ObjectRuntime::fleet(
            host.clone(),
            io.clone(),
            fleet_sync_lease::RegistryConfig {
                authoritative_receiver: receiver,
                lease: fleet_sync_lease::LeasePolicy {
                    lease_duration: Duration::from_secs(60),
                    renew_interval: Duration::from_secs(20),
                },
                release_unsent_for: Duration::ZERO,
                move_needs_renewal_within: None,
            },
            collaboration_central_tokio::DeliveryConfig {
                relay_peer: "019f0000-0000-7000-8000-000000005240"
                    .to_owned()
                    .try_into()?,
            },
            Duration::from_secs(2),
        )?);
        runtime.initialize().await?;
        let database = DatabaseId::try_from(PROJECT.to_owned())?;
        let project_store = objects
            .object(
                ObjectKey::new(format!("copy-{database}"))?,
                ObjectOptions {
                    foreign_keys: false,
                },
                Arc::new(CoreHost::for_store(io.clone(), receiver)),
            )
            .await?;
        let project = Arc::new(ObjectRuntime::copy(
            Arc::new(Host::new(project_store)),
            io.clone(),
            receiver,
            database,
            Duration::from_secs(2),
        ));
        project.initialize().await?;
        let fleet_weak = Arc::downgrade(&runtime);
        let project_weak = Arc::downgrade(&project);
        *io.routes.lock().map_err(|_| "fixture routes poisoned")? = Some((
            Arc::new(move |bytes| {
                let weak = fleet_weak.clone();
                Box::pin(async move { weak.upgrade().ok_or_else(no_route)?.on_call(bytes).await })
            }),
            Arc::new(move |bytes| {
                let weak = project_weak.clone();
                Box::pin(async move { weak.upgrade().ok_or_else(no_route)?.on_call(bytes).await })
            }),
        ));
        // Supplied authenticated enrollment is this ObjectRuntime component fixture's input.
        store.transaction(|scope|Box::pin(async move {
            for (peer,service) in [(A,SERVICE_A),(B,SERVICE_B),(C,SERVICE_C)] {
                query::<sqlx_turso::Turso>("INSERT INTO machine_enrollments(machine_id,service_id,machine_label,access_client_id,credential_hash,state,expires_at,created_at,updated_at) VALUES(?,?,'composed fixture',?,?,'enrolled','2033-05-18T03:33:20+00:00','2026-10-18T00:00:00+00:00','2026-10-18T00:00:00+00:00') ON CONFLICT(machine_id) DO NOTHING")
                    .bind(peer).bind(service).bind(format!("fixture-client-{peer}")).bind(format!("fixture-credential-{peer}")).execute(scope.executor()).await.map_err(StoreError::backend)?;
            } Ok(())
        })).await?;
        Ok(Self {
            objects,
            store,
            host,
            runtime,
            _project: project,
            io,
        })
    }
    pub async fn call(
        &self,
        face: &str,
        kind: &str,
        target: Value,
        request: Value,
    ) -> TestResult<Value> {
        let authority = if face == "edge" {
            json!({"kind":"owner","value":"fixture@example.test"})
        } else {
            json!({"kind":"machine","value":{"installation":SERVICE_A}})
        };
        let bytes = serde_json::to_vec(
            &json!({"source":{"kind":"fleet"},"sourceFace":face,"authority":authority,"destination":{"kind":"fleet"},"destinationFace":if matches!(kind,"central.owner-registry" | "central.session-lease" | "central.mail-submit-registry") {"registry"} else {"fleet"},"identity":{"operation":durable_steps::Uuid::now_v7(),"step":"component_call"},"kind":kind,"target":target,"request":request}),
        )?;
        let answer = self
            .runtime
            .on_call(bytes)
            .await
            .map_err(|failure| format!("{kind}: {failure:?}"))?;
        Ok(serde_json::from_slice(&answer)?)
    }
    pub async fn metadata(&self, operation: fleet_admission::FleetOperation) -> TestResult<Value> {
        self.call("session","central.fleet-metadata",Value::Null,json!({"origin":{"installation":SERVICE_A,"machine":"composed fixture"},"request":fleet_admission::FleetRequest {
            request_id:message_board::RequestId::generate(),workspace:"019f0000-0000-7000-8000-000000008181".to_owned().try_into()?,
            actor:message_board::RecordedActor::Human {human_id:"fixture owner".to_owned().try_into()?},acting_for:None,protocol_version:message_board::ProtocolVersion::CURRENT,operation,
        }})).await
    }
    pub async fn arrange_registered(&self) -> TestResult {
        self.announce_schema().await?;
        use fleet_admission::FleetOperation;
        for operation in [
            FleetOperation::WorkspaceCreate {
                name: "composed workspace".to_owned().try_into()?,
                description: "real component owners".to_owned().try_into()?,
            },
            FleetOperation::BoardCreate {
                board: "019f0000-0000-7000-8000-000000008182"
                    .to_owned()
                    .try_into()?,
                name: "composed board".to_owned().try_into()?,
                description: "real component owners".to_owned().try_into()?,
                board_kind: message_board::BoardKind::Area {},
            },
            FleetOperation::ProjectDirectoryCreate {
                project: PROJECT.to_owned().try_into()?,
                board: "019f0000-0000-7000-8000-000000008182"
                    .to_owned()
                    .try_into()?,
                title: "composed project".to_owned().try_into()?,
            },
        ] {
            let answer = self.metadata(operation).await?;
            if !matches!(
                serde_json::from_value::<fleet_admission::FleetRequestResult>(answer.clone())?,
                fleet_admission::FleetRequestResult::Decided(
                    message_board::RequestDecision::Accepted { .. }
                )
            ) {
                return Err(format!("metadata not accepted: {answer}").into());
            }
        }
        let answer=self.call("session","central.session-lease",Value::Null,json!({"session":{"peer":A,"sessionId":durable_steps::Uuid::now_v7()},"request":{"kind":"register","value":{"attempt":durable_steps::Uuid::now_v7(),"database":PROJECT,"writer":A,"origin":{"kind":"empty"}}}})).await?;
        if answer.pointer("/value/kind") != Some(&json!("registered")) {
            return Err(format!("registration not granted: {answer}").into());
        }
        Ok(())
    }
    async fn announce_schema(&self) -> TestResult {
        use fleet_sync_protocol::{ClientFrame, ConnectionId, SchemaSupport, SessionId};
        use fleet_sync_session::{ConnectionBinding, VerifiedPeer};
        let binding = ConnectionBinding::new(
            ConnectionId::new(durable_steps::Uuid::now_v7())?,
            VerifiedPeer {
                peer: B.to_owned().try_into()?,
                expires_at: SyncTime::from_unix_millis(NOW + 3_600_000),
            },
        );
        let _socket = self
            .host
            .attach_socket(
                binding,
                collaboration_central_core::AdmissionOrigin {
                    installation: SERVICE_B.to_owned().try_into()?,
                    machine: "composed fixture".to_owned().try_into()?,
                },
            )
            .await;
        self.runtime.on_socket_opened(binding).await?;
        self.runtime
            .on_frame(
                binding,
                serde_json::to_vec(
                    &ClientFrame::<board_sync_protocol::BoardConsumerProtocol>::Hello {
                        session_id: SessionId::new(durable_steps::Uuid::now_v7())?,
                        schema_support: SchemaSupport { max_version: 7 },
                    },
                )?,
            )
            .await?;
        self.runtime.on_alarm().await?;
        self.runtime.on_socket_closed(binding).await?;
        collaboration_central_core::ObjectHost::quarantine_connections(
            self.host.as_ref(),
            vec![binding.connection],
        )
        .await;
        Ok(())
    }
    pub async fn owner(&self, request: Value) -> TestResult<Value> {
        self.call(
            "edge",
            "central.owner-registry",
            json!({"database":PROJECT}),
            request,
        )
        .await
    }
    pub async fn sql(&self, sql: &'static str) -> TestResult {
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    scope
                        .executor()
                        .execute(sql)
                        .await
                        .map_err(StoreError::backend)?;
                    Ok(())
                })
            })
            .await?;
        Ok(())
    }
    pub async fn lease_and_notices(
        &self,
    ) -> TestResult<(Value, Option<String>, i64, Vec<(String, String, String)>)> {
        Ok(self.store.transaction(|scope|Box::pin(async move {
            let snapshot=fleet_sync_lease::registry_snapshot(scope,PROJECT.to_owned().try_into().map_err(StoreError::backend)?).await.map_err(StoreError::backend)?;
            let row=query::<sqlx_turso::Turso>("SELECT lease_projection,(SELECT incarnation FROM lease_incarnation WHERE name='fleet') AS incarnation FROM project_directory WHERE project_id=?").bind(PROJECT).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            let notices=query::<sqlx_turso::Turso>("SELECT machine_id,kind,CAST(epoch AS TEXT) AS epoch FROM machine_control_notices WHERE project_id=? ORDER BY machine_id,position").bind(PROJECT).fetch_all(scope.executor()).await.map_err(StoreError::backend)?;
            Ok((serde_json::to_value(snapshot).map_err(StoreError::backend)?,row.try_get("lease_projection").map_err(StoreError::backend)?,row.try_get("incarnation").map_err(StoreError::backend)?,notices.into_iter().map(|row|Ok((row.try_get("machine_id").map_err(StoreError::backend)?,row.try_get("kind").map_err(StoreError::backend)?,row.try_get("epoch").map_err(StoreError::backend)?))).collect::<Result<_,StoreError>>()?))
        })).await?)
    }
    pub async fn close(self) -> TestResult {
        self.objects.close().await?;
        Ok(())
    }
    pub fn peer() -> TestResult<PeerId> {
        Ok(A.to_owned().try_into()?)
    }
}
