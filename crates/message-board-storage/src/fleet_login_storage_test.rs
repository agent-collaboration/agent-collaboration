use super::FleetCache;
use crate::{
    fleet_login::{FleetLoginCandidate, SavedFleetCentral},
    fleet_login_repository::{FleetLoginRepository, FleetLoginStorageFailure},
};
use runtime_primitives::{Clock, IdSource, SyncTime};
use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
async fn fleet_login_table_reopens_in_the_existing_populated_cache() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("fleet-cache.sqlite");
    let cache = FleetCache::open(&path).await?;
    sqlx::query("UPDATE fleet_cache_checkpoint SET position=7 WHERE singleton=1")
        .execute(&cache.database.pool)
        .await?;
    sqlx::query("INSERT INTO fleet_login(row_id,enrollment_id,sign_in,organization_id,machine_environment,receiver_id,relay_peer_id,created_at,updated_at) VALUES ('01900000-0000-7000-8000-000000000001','01900000-0000-7000-8000-000000000002','https://sign-in.invalid','01900000-0000-7000-8000-000000000003','physical','01900000-0000-7000-8000-000000000004','01900000-0000-7000-8000-000000000005',1000,1000)")
        .execute(&cache.database.pool).await?;
    cache.close().await;
    let reopened = FleetCache::open(&path).await?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fleet_login")
        .fetch_one(&reopened.database.pool)
        .await?;
    let checkpoint: i64 =
        sqlx::query_scalar("SELECT position FROM fleet_cache_checkpoint WHERE singleton=1")
            .fetch_one(&reopened.database.pool)
            .await?;
    if count != 1 || checkpoint != 7 {
        return Err("login or existing cache row did not survive reopen".into());
    }
    reopened.close().await;
    Ok(())
}

pub(super) struct FixedClock(pub(super) AtomicI64);
impl Clock for FixedClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
}
pub(super) struct FixedIds;
impl IdSource for FixedIds {
    fn new_id(&self) -> uuid::Uuid {
        uuid::Uuid::from_u128(0x01900000_0000_7000_8000_000000000099)
    }
}
fn candidate() -> FleetLoginCandidate {
    FleetLoginCandidate {
        enrollment_id: "01900000-0000-7000-8000-000000000001"
            .to_owned()
            .try_into()
            .unwrap(),
        organization_id: "01900000-0000-7000-8000-000000000002"
            .to_owned()
            .try_into()
            .unwrap(),
        sign_in: "https://sign-in.invalid".to_owned().try_into().unwrap(),
        requested_machine_environment: None,
        central: SavedFleetCentral {
            receiver: "01900000-0000-7000-8000-000000000003"
                .to_owned()
                .try_into()
                .unwrap(),
            relay: "01900000-0000-7000-8000-000000000004"
                .to_owned()
                .try_into()
                .unwrap(),
        },
    }
}

#[tokio::test]
async fn fleet_login_writer_keeps_the_shared_atomic_repository_contract() {
    let directory = tempfile::tempdir().unwrap();
    let cache = FleetCache::open_with_sources(
        &directory.path().join("fleet-cache.sqlite"),
        Arc::new(FixedClock(AtomicI64::new(1000))),
        Arc::new(FixedIds),
    )
    .await
    .unwrap();
    crate::fleet_test_support::verify_login_repository_contract(&cache)
        .await
        .unwrap();
    cache.close().await;
}

#[tokio::test]
async fn fleet_login_writer_uses_exact_clock_and_keeps_one_row_without_a_database_singleton() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("fleet-cache.sqlite");
    let clock = Arc::new(FixedClock(AtomicI64::new(1000)));
    let cache = FleetCache::open_with_sources(&path, clock.clone(), Arc::new(FixedIds))
        .await
        .unwrap();
    let first = cache.replace_fleet_login(&candidate()).await.unwrap();
    let original: (String, i64, i64) =
        sqlx::query_as("SELECT row_id,created_at,updated_at FROM fleet_login")
            .fetch_one(&cache.database.pool)
            .await
            .unwrap();
    assert_eq!(
        original,
        (
            "01900000-0000-7000-8000-000000000099".to_owned(),
            1000,
            1000
        )
    );
    clock.0.store(2000, Ordering::SeqCst);
    let mut next = candidate();
    next.enrollment_id = "01900000-0000-7000-8000-000000000005"
        .to_owned()
        .try_into()
        .unwrap();
    cache.replace_fleet_login(&next).await.unwrap();
    let updated: (String, i64, i64) =
        sqlx::query_as("SELECT row_id,created_at,updated_at FROM fleet_login")
            .fetch_one(&cache.database.pool)
            .await
            .unwrap();
    assert_eq!(updated, (original.0, 1000, 2000));
    assert!(
        !cache
            .delete_fleet_login(&first.enrollment_id)
            .await
            .unwrap()
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fleet_login")
        .fetch_one(&cache.database.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    // The SQL schema permits multiple rows; the domain writer/decoder is the authority.
    sqlx::query("INSERT INTO fleet_login SELECT '01900000-0000-7000-8000-000000000098','01900000-0000-7000-8000-000000000097',sign_in,organization_id,machine_environment,receiver_id,relay_peer_id,created_at,updated_at FROM fleet_login")
        .execute(&cache.database.pool).await.unwrap();
    assert!(matches!(
        cache.load_fleet_login().await,
        Err(FleetLoginStorageFailure::InvalidRecord { field: "singleton" })
    ));
    assert!(cache.replace_fleet_login(&next).await.is_err());
    cache.close().await;
}

#[tokio::test]
async fn fleet_login_reopen_rejects_corrupt_domain_fields_and_timestamps_without_repair() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("fleet-cache.sqlite");
    let cache = FleetCache::open_with_sources(
        &path,
        Arc::new(FixedClock(AtomicI64::new(1000))),
        Arc::new(FixedIds),
    )
    .await
    .unwrap();
    let saved = cache.replace_fleet_login(&candidate()).await.unwrap();
    sqlx::query("UPDATE fleet_login SET updated_at=999")
        .execute(&cache.database.pool)
        .await
        .unwrap();
    cache.close().await;
    let reopened = FleetCache::open_with_sources(
        &path,
        Arc::new(FixedClock(AtomicI64::new(2000))),
        Arc::new(FixedIds),
    )
    .await
    .unwrap();
    assert!(matches!(
        reopened.load_fleet_login().await,
        Err(FleetLoginStorageFailure::InvalidRecord {
            field: "timestamps"
        })
    ));
    assert!(reopened.replace_fleet_login(&candidate()).await.is_err());
    let retained: (String, i64) =
        sqlx::query_as("SELECT enrollment_id,updated_at FROM fleet_login")
            .fetch_one(&reopened.database.pool)
            .await
            .unwrap();
    assert_eq!(retained, (saved.enrollment_id.as_str().to_owned(), 999));
    reopened.close().await;
}
