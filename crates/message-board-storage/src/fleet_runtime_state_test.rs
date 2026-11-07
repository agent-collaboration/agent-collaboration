use super::*;
use board_sync_protocol::{FleetLinkDrop, FleetLinkDropReason, FleetLinkState};
use chrono::{DateTime, Utc};
use fleet_sync_protocol::PeerId;
type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn instant(second: u32) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&format!("2026-11-06T12:00:{second:02}Z"))
        .unwrap()
        .with_timezone(&Utc)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn link_state_and_last_contact_reopen_with_the_original_drop() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("fleet-cache.sqlite");
    let cache = FleetCache::open(&path).await?;
    let peer = PeerId::new(uuid::Uuid::now_v7())?;
    let dropped = FleetLinkRecord {
        peer,
        endpoint: "wss://central.example/session".to_owned(),
        state: FleetLinkState::Disconnected { since: instant(15) },
        last_drop: Some(FleetLinkDrop {
            reason: FleetLinkDropReason::IdleTimeout,
            at: instant(15),
        }),
        last_failure: None,
    };
    cache.save_fleet_link(&dropped).await?;
    cache.close().await;
    let cache = FleetCache::open(&path).await?;
    assert_eq!(cache.fleet_link().await?, Some(dropped.clone()));
    let mut connected = dropped.clone();
    connected.state = FleetLinkState::Connected {
        since: instant(17),
        last_contact: instant(22),
    };
    cache.save_fleet_link(&connected).await?;
    assert_eq!(cache.fleet_link().await?, Some(connected));
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn malformed_stored_link_is_refused_instead_of_coerced() -> TestResult {
    let directory = tempfile::tempdir()?;
    let cache = FleetCache::open(&directory.path().join("fleet-cache.sqlite")).await?;
    let peer = PeerId::new(uuid::Uuid::now_v7())?;
    cache
        .save_fleet_link(&FleetLinkRecord {
            peer,
            endpoint: "wss://central.example/session".to_owned(),
            state: FleetLinkState::Disconnected { since: instant(0) },
            last_drop: None,
            last_failure: None,
        })
        .await?;
    for state in [
        serde_json::json!({"kind":"future"}),
        serde_json::json!({"kind":"connected","since":instant(12),"lastContact":"invalid-timestamp"}),
    ] {
        let record = serde_json::json!({"peer":peer,"endpoint":"wss://central.example/session","state":state,"lastDrop":null}).to_string();
        sqlx::query("UPDATE fleet_runtime_state SET record = ? WHERE kind = 'centralLink'")
            .bind(record)
            .execute(&cache.database.pool)
            .await?;
        assert!(cache.fleet_link().await.is_err());
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn legacy_import_validates_the_whole_set_before_committing_or_deleting() -> TestResult {
    let directory = tempfile::tempdir()?;
    let session_home = directory.path().join("machine-session");
    let attempt_home = directory.path().join("snapshot-install-attempts");
    tokio::fs::create_dir(&session_home).await?;
    tokio::fs::create_dir(&attempt_home).await?;
    let session = ClientSessionState {
        session_id: fleet_sync_protocol::SessionId::new(uuid::Uuid::now_v7())?,
        notice_cursor: NoticePosition::new(19),
    };
    let session_path = session_home.join("state.json");
    tokio::fs::write(&session_path, serde_json::to_vec(&session)?).await?;
    let record = SnapshotInstallAttempt {
        project: ProjectId::generate(),
        epoch: Epoch::new(2)?,
        attempt: InstallAttemptId::new(uuid::Uuid::now_v7())?,
    };
    let attempt_path = attempt_home.join(format!("{}-e2.json", record.project.as_str()));
    tokio::fs::write(&attempt_path, b"{truncated").await?;
    let database = directory.path().join("fleet-cache.sqlite");
    let cache = FleetCache::open(&database).await?;
    assert!(cache.import_legacy_runtime(directory.path()).await.is_err());
    cache.close().await;
    assert!(session_path.exists());
    let mut observer = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new().filename(&database),
    )
    .await?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fleet_runtime_state")
        .fetch_one(&mut observer)
        .await?;
    assert_eq!(count, 0);
    observer.close().await?;
    tokio::fs::write(&attempt_path, serde_json::to_vec(&record)?).await?;
    let cache = FleetCache::open(&database).await?;
    cache.import_legacy_runtime(directory.path()).await?;
    assert_eq!(cache.machine_session_state().await?, Some(session));
    assert_eq!(
        cache
            .snapshot_install_attempt(&record.project, record.epoch)
            .await?,
        record.attempt
    );
    assert!(!attempt_path.exists());
    assert!(!session_path.exists());
    Ok(())
}

use sqlx::Connection;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn snapshot_attempt_preserves_the_native_epoch_domain_without_signed_narrowing() -> TestResult
{
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("fleet-cache.sqlite");
    let cache = FleetCache::open(&path).await?;
    let project = ProjectId::generate();
    let epoch = Epoch::new(u64::MAX)?;
    let original = cache.snapshot_install_attempt(&project, epoch).await?;
    cache.close().await;
    let reopened = FleetCache::open(&path).await?;
    assert_eq!(
        reopened.snapshot_install_attempt(&project, epoch).await?,
        original
    );
    Ok(())
}
