//! Exact private row IDs prove the real writer used the injected capability.
use super::support::{Fixture, Io, TestResult, bundle, import_bytes};
use collaboration_central_core::{FleetKind, ObjectAddress, ObjectIo, ObjectRuntime};
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
impl ObjectIo for TracedIds {
    fn event_deadline(
        &self,
        remaining: std::time::Duration,
    ) -> impl std::future::Future<Output = ()> + Send {
        tokio::time::sleep(remaining)
    }

    async fn object_call(
        &self,
        address: ObjectAddress,
        bytes: Vec<u8>,
    ) -> Result<Vec<u8>, CallFailure> {
        Io::default().object_call(address, bytes).await
    }
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn injected_id_source_owns_the_repository_feed_state_and_feed_entry_ids() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let next = Arc::new(AtomicU64::new(0xa90));
    let runtime = ObjectRuntime::<FleetKind, _>::fleet(
        Arc::new(crate::io_alarm::io_platform(
            fixture.object.clone(),
            std::sync::Arc::new(TracedIds(next.clone())),
        )),
        fleet_project_lease::RegistryConfig {
            authoritative_receiver: fixture.receiver,
            lease: fleet_project_lease::LeasePolicy {
                lease_duration: std::time::Duration::from_secs(60),
                renew_interval: std::time::Duration::from_secs(20),
            },
            release_unsent_for: std::time::Duration::from_secs(10),
            move_needs_renewal_within: None,
        },
        collaboration_central_tokio::DeliveryConfig {
            relay_peer: "019f0000-0000-7000-8000-000000005240"
                .to_owned()
                .try_into()?,
        },
        std::time::Duration::from_secs(2),
        fleet_protocol::FrameCapabilities::default(),
    )?;
    let proof=async {
        runtime.on_call(import_bytes(&bundle()?)?).await.map_err(|error|format!("import did not answer: {error:?}"))?;
        let ids=fixture.object.transaction(|scope|Box::pin(async move {
            query::<sqlx_turso::Turso>("SELECT row_id AS id,0 AS ordering FROM fleet_workspace_repository_refs UNION ALL SELECT attachment_id AS id,1 AS ordering FROM fleet_repository_attachments UNION ALL SELECT state_id AS id,2 AS ordering FROM fleet_feed_state UNION ALL SELECT entry_id AS id,position*2+1 AS ordering FROM fleet_feed UNION ALL SELECT child.row_id AS id,parent.position*2+2 AS ordering FROM fleet_feed_repository_refs AS child JOIN fleet_feed AS parent ON parent.entry_id=child.entry_id ORDER BY ordering")
                .fetch_all(scope.executor()).await.map_err(StoreError::backend)?.into_iter().map(|row|row.try_get::<String,_>("id").map_err(StoreError::backend)).collect::<Result<Vec<_>,_>>()
        })).await?;
        assert_eq!(ids, vec![
            "019f0000-0000-7000-8000-000000000a90", "019f0000-0000-7000-8000-000000000a91",
            "019f0000-0000-7000-8000-000000000a92", "019f0000-0000-7000-8000-000000000a93",
            "019f0000-0000-7000-8000-000000000a94", "019f0000-0000-7000-8000-000000000a95",
            "019f0000-0000-7000-8000-000000000a96",
        ]);
        assert_eq!(next.load(Ordering::SeqCst),0xa97);
        assert_eq!(fixture.counts().await?,vec![1,1,1,1,3]);
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    fixture.objects.close().await?;
    proof
}
