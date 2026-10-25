//! Exact private row IDs prove the real writer used the injected capability.
use super::support::{Fixture, Io, TestResult, bundle, import_bytes};
use collaboration_central_core::{FleetKind, ObjectAddress, ObjectRuntime, PlatformIo};
use collaboration_central_tokio::TokioStorageHost;
use durable_steps::{AtomicStore, CallFailure, StoreError, storage::TransactionScope};
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use sqlx_core::{query::query, row::Row};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

struct TracedIds(Arc<AtomicU64>);
impl IdSource for TracedIds {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::from_u128(
            0x019f_0000_0000_7000_8000_0000_0000_0000
                | u128::from(self.0.fetch_add(1, Ordering::SeqCst)),
        )
    }
}
impl Clock for TracedIds {
    fn now(&self) -> SyncTime {
        Io::default().now()
    }
}
impl WakeScheduler for TracedIds {
    async fn wake_at(&self, at: SyncTime) -> Result<(), WakeFailure> {
        Io::default().wake_at(at).await
    }
}
impl PlatformIo for TracedIds {
    async fn object_call(
        &self,
        address: ObjectAddress,
        bytes: Vec<u8>,
    ) -> Result<Vec<u8>, CallFailure> {
        Io::default().object_call(address, bytes).await
    }
}

#[tokio::test]
async fn injected_id_source_owns_the_repository_feed_state_and_feed_entry_ids() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let next = Arc::new(AtomicU64::new(0xa90));
    let runtime = ObjectRuntime::<FleetKind, _, _>::fleet(
        Arc::new(TokioStorageHost::new(fixture.object.clone())),
        TracedIds(next.clone()),
        fixture.receiver,
    )?;
    let proof=async {
        runtime.on_call(import_bytes(&bundle()?)?).await.map_err(|error|format!("import did not answer: {error:?}"))?;
        let ids=fixture.object.transaction(|scope|Box::pin(async move {
            query::<sqlx_turso::Turso>("SELECT attachment_id AS id, 0 AS ordering FROM repository_attachments UNION ALL SELECT state_id AS id, 1 AS ordering FROM fleet_feed_state UNION ALL SELECT entry_id AS id, position + 1 AS ordering FROM fleet_feed ORDER BY ordering")
                .fetch_all(scope.executor()).await.map_err(StoreError::backend)?.into_iter().map(|row|row.try_get::<String,_>("id").map_err(StoreError::backend)).collect::<Result<Vec<_>,_>>()
        })).await?;
        assert_eq!(ids, vec![
            "019f0000-0000-7000-8000-000000000a90", "019f0000-0000-7000-8000-000000000a91",
            "019f0000-0000-7000-8000-000000000a92", "019f0000-0000-7000-8000-000000000a93",
            "019f0000-0000-7000-8000-000000000a94",
        ]);
        assert_eq!(next.load(Ordering::SeqCst),0xa95);
        assert_eq!(fixture.counts().await?,vec![1,1,1,1,3]);
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    fixture.objects.close().await?;
    proof
}
