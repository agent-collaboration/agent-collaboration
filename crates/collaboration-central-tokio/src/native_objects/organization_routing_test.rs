//! Real native object files, stores and reopen routing, including the durable Copy note.
use super::*;
use collaboration_central_core::FleetLocationPolicy;
use durable_steps::{AtomicStore, storage::TransactionScope};
use sqlx_core::{query::query, row::Row};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
const LEGACY: &str = "019f0000-0000-7000-8000-00000000ac01";
const FIRST: &str = "019f0000-0000-7000-8000-00000000ac02";
const SECOND: &str = "019f0000-0000-7000-8000-00000000ac03";
struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(1_792_281_600_000)
    }
}
#[derive(Default)]
struct FixtureIds(AtomicU64);
impl IdSource for FixtureIds {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::from_u128(
            0x019f0000000070008000000000000000 + u128::from(self.0.fetch_add(1, Ordering::Relaxed)),
        )
    }
}
fn fleet(organization: &str) -> TestResult<ObjectAddress> {
    Ok(ObjectAddress::Fleet {
        organization: organization.to_owned().try_into()?,
    })
}
async fn open(root: &Path) -> TestResult<Arc<NativeObjects>> {
    Ok(NativeObjects::open_with_controls(
        root,
        Arc::new(FixedClock),
        Arc::new(FixtureIds::default()),
        FleetLocationPolicy::new(LEGACY.to_owned().try_into()?),
        configuration::registry_config()?,
        crate::DeliveryConfig {
            relay_peer: "019f0000-0000-7000-8000-00000000ac04"
                .to_owned()
                .try_into()?,
        },
        Duration::from_secs(2),
        Duration::from_secs(2),
        vec![],
    )
    .await?)
}
async fn write_marker(slot: &ObjectSlot, marker: i64) -> TestResult {
    let host = slot.host.get().ok_or("Fleet host missing")?;
    host.object.transaction(move |scope| Box::pin(async move {
        query::<sqlx_turso::Turso>("CREATE TABLE b2_routing_marker (id TEXT PRIMARY KEY NOT NULL, marker INTEGER NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL)")
            .execute(scope.executor()).await.map_err(StoreError::backend)?;
        query::<sqlx_turso::Turso>("INSERT INTO b2_routing_marker VALUES ('019f0000-0000-7000-8000-00000000ac05', ?, 1792281600000, 1792281600000)")
            .bind(marker).execute(scope.executor()).await.map_err(StoreError::backend)?;
        Ok(())
    })).await?;
    Ok(())
}
async fn read_marker(slot: &ObjectSlot) -> TestResult<i64> {
    let host = slot.host.get().ok_or("Fleet host missing")?;
    Ok(host
        .object
        .transaction(|scope| {
            Box::pin(async move {
                let row = query::<sqlx_turso::Turso>("SELECT marker FROM b2_routing_marker")
                    .fetch_one(scope.executor())
                    .await
                    .map_err(StoreError::backend)?;
                row.try_get::<i64, _>("marker").map_err(StoreError::backend)
            })
        })
        .await?)
}
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions exercise physical storage isolation and preservation while fixture errors propagate."
)]
#[tokio::test(start_paused = true)]
async fn organization_routing_isolates_real_native_stores_and_reopens_the_same_data() -> TestResult
{
    let root = tempfile::tempdir()?;
    let objects = open(root.path()).await?;
    for (organization, marker, name) in [
        (LEGACY, 10, "fleet.db".to_owned()),
        (FIRST, 20, format!("fleet-{FIRST}.db")),
        (SECOND, 30, format!("fleet-{SECOND}.db")),
    ] {
        let slot = objects.slot(fleet(organization)?).await?;
        assert!(
            slot.host
                .get()
                .ok_or("Fleet host missing")?
                .object
                .path()
                .ends_with(&name),
            "organization {organization} reached another Fleet file"
        );
        write_marker(&slot, marker).await?;
        assert!(Arc::ptr_eq(
            &slot,
            &objects.slot(fleet(organization)?).await?
        ));
    }
    objects.shutdown().await?;
    drop(objects);
    let reopened = open(root.path()).await?;
    for (organization, expected) in [(LEGACY, 10), (FIRST, 20), (SECOND, 30)] {
        let slot = reopened.slot(fleet(organization)?).await?;
        assert_eq!(
            read_marker(&slot).await?,
            expected,
            "restart selected another organization's data"
        );
    }
    reopened.shutdown().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions reject rebinding at the real initialized Copy store while fixture errors propagate."
)]
#[tokio::test(start_paused = true)]
async fn organization_routing_copy_note_survives_reopen_and_refuses_another_organization()
-> TestResult {
    let root = tempfile::tempdir()?;
    let objects = open(root.path()).await?;
    let database: fleet_sync_protocol::DatabaseId = "019f0000-0000-7000-8000-00000000ac10"
        .to_owned()
        .try_into()?;
    let receiver = objects.config.authoritative_receiver;
    let create = |organization: &str| -> TestResult<Vec<u8>> {
        Ok(serde_json::to_vec(&serde_json::json!({
            "organization":organization,
            "source":fleet(organization)?,"sourceFace":"registry","authority":{"kind":"object"},
            "destination":{"kind":"copy","value":database},"destinationFace":"copy",
            "identity":{"operation":"019f0000-0000-7000-8000-00000000ac11","step":"create_copy"},
            "kind":"fleet-sync.copy-create","target":{"copy":receiver},
            "request":{"database":database,"epoch":1,"holder":"019f0000-0000-7000-8000-00000000ac12","origin":{"kind":"empty"}}
        }))?)
    };
    objects
        .call(ObjectAddress::Copy(database), create(FIRST)?)
        .await
        .map_err(|error| format!("create fixture: {error:?}"))?;
    let slot = objects.slot(ObjectAddress::Copy(database)).await?;
    let RuntimeFace::Copy(copy) = slot.runtime.get().ok_or("copy runtime missing")?.as_ref() else {
        return Err("wrong face".into());
    };
    let before = copy
        .read_copy_status()
        .await
        .map_err(|error| format!("Copy inspection: {error:?}"))?;
    assert!(matches!(
        before,
        fleet_sync_protocol::CopyStatusAnswer::Status(_)
    ));
    objects.shutdown().await?;
    drop(slot);
    drop(objects);
    let reopened = open(root.path()).await?;
    assert!(
        reopened
            .call(ObjectAddress::Copy(database), create(SECOND)?)
            .await
            .is_err()
    );
    let slot = reopened.slot(ObjectAddress::Copy(database)).await?;
    // Wrong build caught: an in-memory note lost at restart, or a second
    // organization's creation rebinding the Copy.
    let host = slot.host.get().ok_or("Copy host missing")?;
    let note = host
        .object
        .transaction(|scope| {
            Box::pin(async move {
                let row = query::<sqlx_turso::Turso>(
                    "SELECT project_id, fleet_address_key FROM _copy_fleet_reference",
                )
                .fetch_all(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                row.iter()
                    .map(|row| {
                        Ok((
                            row.try_get::<String, _>("project_id")
                                .map_err(StoreError::backend)?,
                            row.try_get::<String, _>("fleet_address_key")
                                .map_err(StoreError::backend)?,
                        ))
                    })
                    .collect::<Result<Vec<_>, StoreError>>()
            })
        })
        .await?;
    assert_eq!(note, vec![(database.to_string(), FIRST.to_owned())]);
    let RuntimeFace::Copy(copy) = slot.runtime.get().ok_or("copy runtime missing")?.as_ref() else {
        return Err("wrong face".into());
    };
    assert_eq!(
        copy.read_copy_status()
            .await
            .map_err(|error| format!("Copy inspection: {error:?}"))?,
        before
    );
    reopened.shutdown().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A physical alias must fail reopening instead of sharing the configured legacy store."
)]
#[tokio::test(start_paused = true)]
async fn organization_routing_refuses_a_legacy_alias_file() -> TestResult {
    let root = tempfile::tempdir()?;
    tokio::fs::write(root.path().join(format!("fleet-{LEGACY}.db")), []).await?;
    assert!(open(root.path()).await.is_err());
    Ok(())
}
