//! The shared scenario on the Tokio host's real store. workerd runs the same
//! script on Durable Object SQLite and must produce the same report.
use super::super::sql_store_fixture::{FixtureObject, TestError};
use super::{expected_scenario, run_scenario};
use crate::CoreHost;
use crate::fleet_storage_fixture::FixtureIo;
use fleet_sync_protocol::ReceiverId;
use fleet_sync_tokio_runner::{ObjectKey, ObjectStore};

#[tokio::test]
async fn the_tokio_store_produces_the_shared_scenario_report() -> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    let objects = ObjectStore::open(root.path()).await?;
    let host = std::sync::Arc::new(CoreHost::for_store(
        FixtureIo,
        ReceiverId::new(durable_steps::Uuid::now_v7())?,
    ));
    let object = objects
        .object(ObjectKey::new("fleet")?, Default::default(), host)
        .await?;

    let report = run_scenario::<FixtureObject, sqlx_turso::Turso>(object).await?;

    let expected = expected_scenario();
    for (observed, wanted) in report.iter().zip(&expected) {
        assert_eq!(observed, wanted);
    }
    assert_eq!(report.len(), expected.len());
    objects.close().await?;
    Ok(())
}
