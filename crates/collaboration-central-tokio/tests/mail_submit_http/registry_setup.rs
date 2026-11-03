//! Genuine library grants in the owning Registry, with a real physical Copy.
//!
//! Fleet admission and an idle holder's release answer are setup stand-ins.
//! They do not prove the unfinished normal Register/Move request composition.
use collaboration_central_conformance::ConformanceResult;
use collaboration_central_core::{CopyKind, CoreHost, ObjectAddress, ObjectRuntime, PlatformIo};
use collaboration_central_tokio::TokioStorageHost;
use durable_steps::{
    AtomicStore, CallFailure, CallId, CallTarget, Engine, Host, OperationState, Uuid,
};
use fleet_sync_lease::{
    HolderChangeTask, RegistrationTask, RegistryConfig, admit_holder_change, lifecycle_request_key,
};
use fleet_sync_protocol::*;
use fleet_sync_tokio_runner::{ObjectKey, ObjectOptions, ObjectStore, TokioObject};
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use serde_json::json;
use std::{path::Path, sync::Arc, time::Duration};

#[path = "registry_setup/imported_seed.rs"]
mod imported_seed;

/// Fixed replicated identities preserve the normal fixture's logical integer ids.
pub(super) fn normal_record_key(id: i64) -> ConformanceResult<&'static str> {
    match id {
        1 => Ok("019f0000-0000-7000-8000-000000000831"),
        2 => Ok("019f0000-0000-7000-8000-000000000832"),
        3 => Ok("019f0000-0000-7000-8000-000000000833"),
        4 => Ok("019f0000-0000-7000-8000-000000000834"),
        5 => Ok("019f0000-0000-7000-8000-000000000835"),
        6 => Ok("019f0000-0000-7000-8000-000000000836"),
        7 => Ok("019f0000-0000-7000-8000-000000000837"),
        8 => Ok("019f0000-0000-7000-8000-000000000838"),
        9 => Ok("019f0000-0000-7000-8000-000000000839"),
        _ => Err(format!("normal-state fixture has no fixed record key for id {id}").into()),
    }
}

type Store = TokioObject<CoreHost<SetupIo>>;
type Copy = ObjectRuntime<CopyKind, TokioStorageHost<CoreHost<SetupIo>>, SetupIo>;

pub(super) struct SetupIo;
impl Clock for SetupIo {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(1_792_281_600_000)
    }
}
impl IdSource for SetupIo {
    fn new_id(&self) -> Uuid {
        Uuid::now_v7()
    }
}
impl WakeScheduler for SetupIo {
    async fn wake_at(&self, _: SyncTime) -> Result<(), WakeFailure> {
        Ok(())
    }
}
impl PlatformIo for SetupIo {
    fn reply_deadline(
        &self,
        remaining: std::time::Duration,
    ) -> impl std::future::Future<Output = ()> + Send {
        tokio::time::sleep(remaining)
    }

    async fn object_call(&self, _: ObjectAddress, _: Vec<u8>) -> Result<Vec<u8>, CallFailure> {
        Err(CallFailure::AnswerLost)
    }
}
struct GrantHost {
    copy: Copy,
    database: DatabaseId,
}
impl Clock for GrantHost {
    fn now(&self) -> SyncTime {
        SetupIo.now()
    }
}
impl IdSource for GrantHost {
    fn new_id(&self) -> Uuid {
        SetupIo.new_id()
    }
}
impl WakeScheduler for GrantHost {
    async fn wake_at(&self, _: SyncTime) -> Result<(), WakeFailure> {
        Ok(())
    }
}
impl Host for GrantHost {
    async fn dispatch<C: CallTarget>(
        &self,
        target: C,
        request: C::Request,
        call: CallId,
    ) -> Result<C::Answer, CallFailure> {
        let answer = match C::KIND {
            "fleet-sync.registration-admission" | "fleet-sync.retirement-admission" => {
                serde_json::to_vec(&GateAnswer::Admitted)
            }
            "fleet-sync.holder-release" => serde_json::to_vec(&ReleaseAnswer::Released {
                final_position: ReplicationPosition::new(
                    Epoch::new(1).map_err(lost)?,
                    CommitSequence::new(0),
                ),
            }),
            "fleet-sync.copy-create"
            | "fleet-sync.copy-acknowledged"
            | "fleet-sync.copy-fence"
            | "fleet-sync.copy-status"
            | "fleet-sync.copy-retire" => {
                let bytes = serde_json::to_vec(&json!({
                    "source":{"kind":"fleet"},"sourceFace":"registry","authority":{"kind":"object"},
                    "destination":{"kind":"copy","value":self.database},"destinationFace":"copy",
                    "identity":call,"kind":C::KIND,"target":target,"request":request
                }))
                .map_err(lost)?;
                return serde_json::from_slice(&self.copy.on_call(bytes).await?).map_err(lost);
            }
            _ => return Err(CallFailure::AnswerLost),
        }
        .map_err(lost)?;
        serde_json::from_slice(&answer).map_err(lost)
    }
}
fn lost(_: impl std::fmt::Debug) -> CallFailure {
    CallFailure::AnswerLost
}

pub(super) async fn register_grant_history(
    root: &Path,
    config: RegistryConfig,
    database: DatabaseId,
    holders: Vec<(Epoch, PeerId)>,
) -> ConformanceResult<()> {
    let first = holders
        .first()
        .ok_or("grant history needs its first holder")?
        .1;
    let GrantFixture {
        objects,
        registry,
        host,
    } = GrantFixture::open(root, config, database).await?;
    let mut engine = Engine::new(registry.clone(), host.clone());
    engine.register(RegistrationTask::<
        Store,
        sqlx_turso::Turso,
        fleet_sync_lease::NoLeaseChangeHook,
    >::new(
        config,
        host.clone(),
        Arc::new(fleet_sync_lease::NoLeaseChangeHook),
    ))?;
    engine.register(HolderChangeTask::<
        Store,
        sqlx_turso::Turso,
        fleet_sync_lease::NoLeaseChangeHook,
    >::new(
        config,
        host.clone(),
        Arc::new(fleet_sync_lease::NoLeaseChangeHook),
    ))?;
    let proof = async {
        let registration = Registration { attempt: RequestId::new(Uuid::now_v7())?, database, writer: first, origin: Origin::Empty };
        let id = engine.start::<RegistrationTask<Store, sqlx_turso::Turso, fleet_sync_lease::NoLeaseChangeHook>>(
            &lifecycle_request_key(registration.attempt)?, registration).await?;
        let OperationState::Completed { result } = engine.status(&id).await?.state else {
            return Err("real registration task did not complete".into());
        };
        let answer: RegistrationAnswer = serde_json::from_slice(&result)?;
        if !matches!(answer, RegistrationAnswer::Registered { ref lease, replayed: false } if lease.epoch == Epoch::new(1)? && lease.holder == first) {
            return Err(format!("registration did not grant the first peer: {answer:?}").into());
        }
        for pair in holders.windows(2) {
            let [(from_epoch,_),(expected_epoch,target)] = pair else {
                return Err("grant-history fixture requires adjacent holder pairs".into());
            };
            let expected_epoch = *expected_epoch;
            let CopyStatusAnswer::Status(status) = host.dispatch(
                fleet_sync_lease::CopyRead {copy:config.authoritative_receiver,database}, (),
                CallId {operation:durable_steps::OperationId::new(Uuid::now_v7())?,step:durable_steps::StepName::new("grant_setup_status")?},
            ).await? else {return Err("real copy status unavailable in grant-history setup".into())};
            let target = *target;
            registry.transaction(move |scope|Box::pin(async move {
                // Normal Hello already owns its attach sequence. Reuse that
                // public report rather than inventing another session at it.
                let report = match fleet_sync_lease::accepted_schema_support(scope,target)
                    .await.map_err(durable_steps::StoreError::backend)? {
                    Some(report) if report.support.max_version >= status.schema.version => report,
                    Some(_) => return Err(durable_steps::StoreError::backend("grant setup peer does not support the real Copy schema")),
                    None => ReportSchemaSupport {
                        peer:target,session_id:SessionId::new(Uuid::now_v7()).map_err(durable_steps::StoreError::backend)?,
                        attach_seq:AttachSequence::new(1).map_err(durable_steps::StoreError::backend)?,support:SchemaSupport {max_version:status.schema.version},
                    },
                };
                fleet_sync_lease::report_schema_support(scope,&report,SetupIo.now()).await.map_err(durable_steps::StoreError::backend)?;
                Ok(())
            })).await?;
            let request = HolderChangeRequest::Move { attempt: TransferId::new(Uuid::now_v7())?, database, from_epoch: *from_epoch, to: target, reason: HolderChangeReason::new("grant-history setup")? };
            let id = registry.transaction(move |scope| Box::pin(async move {
                admit_holder_change::<Store, sqlx_turso::Turso>(scope, &request, &ActorRef::new("grant-history-setup").map_err(durable_steps::StoreError::backend)?, SetupIo.now())
                    .await.map(|admitted| admitted.id()).map_err(durable_steps::StoreError::backend)
            })).await?;
            engine.drive_due().await?;
            let OperationState::Completed { result } = engine.status(&id).await?.state else {
                return Err(format!("real move task did not complete: {:?}", engine.status(&id).await?).into());
            };
            let answer: HolderChangeAnswer = serde_json::from_slice(&result)?;
            if !matches!(answer, HolderChangeAnswer::Granted { epoch, .. } if epoch == expected_epoch) {
                return Err(format!("real move did not grant its expected epoch: {answer:?}").into());
            }
        }
        let final_epoch = holders.last().ok_or("grant history has no final holder")?.0;
        registry.transaction(move |scope| Box::pin(async move {
            for (epoch, expected) in holders {
                let actual = fleet_sync_lease::epoch_holder(scope, database, epoch).await.map_err(durable_steps::StoreError::backend)?;
                if !actual.is_some_and(|grant| grant.holder == expected && (epoch == final_epoch || grant.ended_at.is_some())) {
                    return Err(durable_steps::StoreError::backend("genuine epoch history mismatch"));
                }
            }
            Ok(())
        })).await?;
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    objects.close().await?;
    proof
}

struct GrantFixture {
    objects: ObjectStore<CoreHost<SetupIo>>,
    registry: Arc<Store>,
    host: Arc<GrantHost>,
}
impl GrantFixture {
    async fn open(
        root: &Path,
        config: RegistryConfig,
        database: DatabaseId,
    ) -> ConformanceResult<Self> {
        let objects = ObjectStore::open(root).await?;
        let registry = objects
            .object(
                ObjectKey::new("fleet")?,
                Default::default(),
                Arc::new(CoreHost::for_store(SetupIo, config.authoritative_receiver)),
            )
            .await?;
        let runtime = ObjectRuntime::<collaboration_central_core::FleetKind, _, _>::fleet(
            Arc::new(TokioStorageHost::new(registry.clone())),
            SetupIo,
            config,
            collaboration_central_tokio::DeliveryConfig {
                relay_peer: fleet_sync_protocol::PeerId::try_from(
                    "019f0000-0000-7000-8000-000000005240".to_owned(),
                )?,
            },
            std::time::Duration::from_secs(2),
        )?;
        runtime.initialize().await?;
        let copy_store = objects
            .object(
                ObjectKey::new(format!("copy-{database}"))?,
                ObjectOptions {
                    foreign_keys: false,
                },
                Arc::new(CoreHost::for_store(SetupIo, config.authoritative_receiver)),
            )
            .await?;
        let copy = ObjectRuntime::copy(
            Arc::new(TokioStorageHost::new(copy_store)),
            SetupIo,
            config.authoritative_receiver,
            database,
            Duration::from_secs(2),
        );
        copy.initialize().await?;
        let host = Arc::new(GrantHost { copy, database });
        Ok(Self {
            objects,
            registry,
            host,
        })
    }
}

/// Admit public ImportTask input; the actual native Fleet owns execution and its lease hook.
pub(super) async fn import_unheld(
    root: &Path,
    config: RegistryConfig,
    database: DatabaseId,
) -> ConformanceResult<()> {
    use durable_steps::{JournalScope, StoreError};
    use sqlx_core::{
        connection::{ConnectOptions, Connection},
        query::query,
        row::Row,
    };
    let objects = ObjectStore::open(root).await?;
    let registry = objects
        .object(
            ObjectKey::new("fleet")?,
            Default::default(),
            Arc::new(CoreHost::for_store(SetupIo, config.authoritative_receiver)),
        )
        .await?;
    let request = Import {
        attempt: RequestId::new(Uuid::now_v7())?,
        database,
        digest: ImportDigest::new("d".repeat(64))?,
    };
    let key = lifecycle_request_key(request.attempt)?;
    let operation = registry
        .transaction(move |scope| {
            Box::pin(async move {
                // Only kind/version/input are supplied here. No NoHook engine runs.
                scope
                    .admit::<fleet_sync_lease::ImportTask<
                        Store,
                        sqlx_turso::Turso,
                        fleet_sync_lease::NoLeaseChangeHook,
                    >>(&key, &request)
                    .await
                    .map(|admitted| admitted.id())
                    .map_err(StoreError::backend)
            })
        })
        .await?;
    objects.close().await?;
    let platform = collaboration_central_tokio::TokioCentralPlatform::new(
        "http://127.0.0.1:1/unused-fixture-jwks",
        Arc::new(SetupIo),
        |_| {},
        root,
        config,
        collaboration_central_tokio::DeliveryConfig {
            relay_peer: "019f0000-0000-7000-8000-000000005240"
                .to_owned()
                .try_into()?,
        },
        Duration::from_secs(2),
        Duration::from_secs(2),
    )
    .await?;
    // Opening the native owner discovers the committed task and drives its real
    // Copy creation and same-transaction Fleet hook. This is component setup,
    // not proof of the admin HTTP import workflow.
    let proof = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let mut observer = sqlx_turso::TursoConnectOptions::new()
                .filename(root.join("fleet.db"))
                .read_only(true)
                .connect()
                .await?;
            let row = query::<sqlx_turso::Turso>(
                "SELECT state,result FROM _fleet_sync_operations WHERE operation_id=?",
            )
            .bind(operation.as_uuid().to_string())
            .fetch_one(&mut observer)
            .await?;
            let state = row.try_get::<String, _>("state")?;
            let result = row.try_get::<Option<Vec<u8>>, _>("result")?;
            observer.close().await?;
            match state.as_str() {
                "running" => tokio::task::yield_now().await,
                "completed" => {
                    if serde_json::from_slice::<ImportAnswer>(
                        &result.ok_or("real import lost its result")?,
                    )? != ImportAnswer::Imported
                    {
                        return Err("real native import did not newly commit Unheld".into());
                    }
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
                _ => return Err(format!("real native import stopped as {state}").into()),
            }
        }
    })
    .await;
    let closed = platform.shutdown().await;
    proof??;
    closed?;
    Ok(())
}

/// Arrange genuine imported state and publish its physical seed before the server owns it.
/// The HTTP admin-import workflow is not exercised by this component arranger.
// This arranger is shared with targets that only need grants.
pub(super) async fn import_seeded(
    root: &Path,
    config: RegistryConfig,
    database: DatabaseId,
) -> ConformanceResult<()> {
    import_unheld(root, config, database).await?;
    let GrantFixture { objects, host, .. } = GrantFixture::open(root, config, database).await?;
    let published =
        imported_seed::publish(&host.copy, config.authoritative_receiver, database).await;
    objects.close().await?;
    published
}

/// Seed the old normal-redeploy dataset through public ImportTask and Copy calls.
pub(super) async fn import_normal_rows(
    root: &Path,
    config: RegistryConfig,
    database: DatabaseId,
) -> ConformanceResult<()> {
    import_unheld(root, config, database).await?;
    let GrantFixture { objects, host, .. } = GrantFixture::open(root, config, database).await?;
    let result =
        imported_seed::publish_normal_rows(&host.copy, config.authoritative_receiver, database)
            .await;
    objects.close().await?;
    result
}
