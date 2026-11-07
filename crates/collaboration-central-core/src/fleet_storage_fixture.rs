//! Real owning-store fixture for Fleet's private query and domain-codec tests.
use crate::{CoreHost, ObjectAddress, PlatformIo, generated_enrollment};
use durable_steps::{
    AtomicStore, CallFailure, NotSentReason, StoreError, storage::TransactionScope,
};
use fleet_protocol::{PeerId, ReceiverId};
use fleet_tokio_objects::{ObjectKey, ObjectStore, TokioObject};
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use sqlx_core::executor::Executor;
use std::sync::Arc;

pub(crate) type TestError = Box<dyn std::error::Error + Send + Sync>;
pub(crate) const NOW: SyncTime = SyncTime::from_unix_millis(1_792_281_600_000);
#[derive(Clone, Copy)]
pub(crate) struct FixtureIo;
impl Clock for FixtureIo {
    fn now(&self) -> SyncTime {
        NOW
    }
}
impl IdSource for FixtureIo {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::now_v7()
    }
}
impl WakeScheduler for FixtureIo {
    async fn wake_at(&self, _: SyncTime) -> Result<(), WakeFailure> {
        Ok(())
    }
}
impl PlatformIo for FixtureIo {
    fn reply_deadline(
        &self,
        remaining: std::time::Duration,
    ) -> impl std::future::Future<Output = ()> + Send {
        tokio::time::sleep(remaining)
    }

    async fn object_call(&self, _: ObjectAddress, _: Vec<u8>) -> Result<Vec<u8>, CallFailure> {
        Err(CallFailure::NotSent {
            reason: NotSentReason::Unreachable { last_seen: None },
        })
    }
}

pub(crate) struct FleetStorageFixture {
    pub object: Arc<TokioObject<CoreHost<FixtureIo>>>,
    objects: ObjectStore<CoreHost<FixtureIo>>,
    pub root: tempfile::TempDir,
}
impl FleetStorageFixture {
    pub async fn open() -> Result<Self, TestError> {
        let root = tempfile::tempdir()?;
        let objects = ObjectStore::open(root.path()).await?;
        let host = Arc::new(CoreHost::for_store(
            FixtureIo,
            ReceiverId::new(durable_steps::Uuid::now_v7())?,
        ));
        let object = objects
            .object(ObjectKey::new("fleet")?, Default::default(), host)
            .await?;
        object
            .transaction(|scope| {
                Box::pin(async move {
                    scope
                        .executor()
                        .execute(include_str!(
                            "../registry_migrations/0001_machine_enrollments.sql"
                        ))
                        .await
                        .map_err(StoreError::backend)?;
                    scope
                        .executor()
                        .execute(include_str!(
                            "../session_migrations/0011_machine_notices.sql"
                        ))
                        .await
                        .map_err(StoreError::backend)?;
                    scope
                        .executor()
                        .execute(include_str!(
                            "../session_migrations/0012_forward_correlations.sql"
                        ))
                        .await
                        .map_err(StoreError::backend)?;
                    Ok(())
                })
            })
            .await?;
        Ok(Self {
            object,
            objects,
            root,
        })
    }
    pub async fn enroll(&self, peer: PeerId, service: &str) -> Result<(), TestError> {
        let service = service.to_owned();
        self.object
            .transaction(move |scope| {
                Box::pin(async move {
                    generated_enrollment::insert_enrollment(
                        scope,
                        peer.to_string(),
                        service.clone(),
                        "storage fixture".into(),
                        format!("fixture-client-{peer}"),
                        format!("fixture-hash-{peer}"),
                        "2033-05-18T03:33:20+00:00".into(),
                        "2026-10-18T00:00:00+00:00".into(),
                        "2026-10-18T00:00:00+00:00".into(),
                    )
                    .await
                    .map_err(StoreError::backend)
                })
            })
            .await?;
        Ok(())
    }
    pub async fn close(self) -> Result<(), TestError> {
        self.objects.close().await?;
        Ok(())
    }
    pub async fn reopen(self) -> Result<Self, TestError> {
        let Self {
            object,
            objects,
            root,
        } = self;
        objects.close().await?;
        drop(object);
        drop(objects);
        let objects = ObjectStore::open(root.path()).await?;
        let host = Arc::new(CoreHost::for_store(
            FixtureIo,
            ReceiverId::new(durable_steps::Uuid::now_v7())?,
        ));
        let object = objects
            .object(ObjectKey::new("fleet")?, Default::default(), host)
            .await?;
        Ok(Self {
            object,
            objects,
            root,
        })
    }
}
