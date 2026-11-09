use super::*;
use board_sync_protocol::{FleetLinkDrop, FleetLinkDropReason, FleetLinkRecord, FleetLinkState};
use chrono::{DateTime, Utc};
use fleet_protocol::PeerId;
type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn approved_runtime_schema_uses_three_typed_tables_without_blob_or_checks() -> TestResult {
    let directory = tempfile::tempdir()?;
    let cache = FleetCache::open(&directory.path().join("fleet-cache.sqlite")).await?;
    let definitions: Vec<String> = sqlx::query_scalar(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name IN ('fleet_session','fleet_link_state','fleet_install_attempts') ORDER BY name"
    ).fetch_all(&cache.database.pool).await?;
    assert_eq!(definitions.len(), 3, "approved runtime tables are missing");
    for definition in definitions {
        assert!(!definition.to_uppercase().contains("CHECK"));
        assert!(!definition.contains("record TEXT"));
    }
    let old: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE name IN ('fleet_runtime_state','fleet_snapshot_install_attempts')")
        .fetch_one(&cache.database.pool).await?;
    assert_eq!(old, 0, "unshipped blob tables must be replaced entirely");
    // Literal column sets come from the owner-approved 8ad923956 table page.
    for (statement, expected) in [
        (
            "PRAGMA table_info(fleet_session)",
            vec![
                "session_id",
                "notice_cursor",
                "handled_through",
                "created_at",
                "updated_at",
            ],
        ),
        (
            "PRAGMA table_info(fleet_link_state)",
            vec![
                "id",
                "peer_id",
                "endpoint",
                "instance",
                "connection",
                "revision",
                "state_kind",
                "state_since",
                "connecting_attempt",
                "last_frame_at",
                "refusal_kind",
                "refusal_detail",
                "last_drop_kind",
                "last_drop_at",
                "last_drop_connection",
                "last_failure_kind",
                "last_failure_detail",
                "last_failure_at",
                "created_at",
                "updated_at",
            ],
        ),
        (
            "PRAGMA table_info(fleet_install_attempts)",
            vec!["id", "project_id", "epoch", "created_at", "updated_at"],
        ),
    ] {
        let columns = sqlx::query_as::<_, ApprovedColumn>(statement)
            .fetch_all(&cache.database.pool)
            .await?;
        assert_eq!(
            columns
                .iter()
                .map(|column| column.name.as_str())
                .collect::<Vec<_>>(),
            expected
        );
        for column in columns {
            assert_eq!(
                column.storage_type,
                if column.name == "connecting_attempt" {
                    "INTEGER"
                } else {
                    "TEXT"
                }
            );
            assert_eq!(
                column.pk,
                i64::from(column.name == "id" || column.name == "session_id")
            );
        }
    }
    cache.close().await;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct ApprovedColumn {
    name: String,
    #[sqlx(rename = "type")]
    storage_type: String,
    pk: i64,
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn reenrollment_clears_session_and_link_but_retains_unfinished_attempt() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("fleet-cache.sqlite");
    let cache = FleetCache::open(&path).await?;
    let state = ClientSessionState {
        session_id: fleet_protocol::SessionId::new(uuid::Uuid::now_v7())?,
        notice_cursor: NoticePosition::new(u64::MAX),
    };
    cache.save_machine_session_state(&state).await?;
    cache
        .save_machine_notice_progress(NoticePosition::new(9))
        .await?;
    let project = ProjectId::generate();
    let epoch = Epoch::new(u64::MAX)?;
    let attempt = cache.snapshot_install_attempt(&project, epoch).await?;
    cache.clear_machine_runtime().await?;
    cache.close().await;
    let cache = FleetCache::open(&path).await?;
    assert_eq!(cache.machine_session_state().await?, None);
    assert_eq!(
        cache.machine_notice_progress().await?,
        NoticePosition::new(0)
    );
    assert_eq!(cache.fleet_link().await?, None);
    assert_eq!(
        cache.snapshot_install_attempt(&project, epoch).await?,
        attempt,
        "re-enrollment discarded an unfinished install identity"
    );
    cache.close().await;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn orphan_progress_is_discarded_without_creating_a_session_or_blocking_start() -> TestResult {
    let directory = tempfile::tempdir()?;
    tokio::fs::create_dir(directory.path().join("machine-session")).await?;
    let source = directory
        .path()
        .join("machine-session/machine-notice-progress.json");
    tokio::fs::write(&source, br#"{"handledThrough":73}"#).await?;
    let cache = FleetCache::open(&directory.path().join("fleet-cache.sqlite")).await?;
    cache.import_legacy_runtime(directory.path()).await?;
    assert_eq!(cache.machine_session_state().await?, None);
    assert_eq!(
        cache.machine_notice_progress().await?,
        NoticePosition::new(0),
        "orphan progress was imported without its native session"
    );
    assert!(!source.exists());
    let state = ClientSessionState {
        session_id: fleet_protocol::SessionId::new(uuid::Uuid::now_v7())?,
        notice_cursor: NoticePosition::new(0),
    };
    cache.save_machine_session_state(&state).await?;
    assert_eq!(cache.machine_session_state().await?, Some(state));
    cache.close().await;
    Ok(())
}

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

        instance: 0,
        connection: 0,
        revision: 0,
        last_frame_at: None,
        state: FleetLinkState::Disconnected { since: instant(15) },
        last_drop: Some(FleetLinkDrop {
            reason: FleetLinkDropReason::IdleTimeout,
            connection: 0,
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
    connected.last_frame_at = Some(instant(22));
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

            instance: 0,
            connection: 0,
            revision: 0,
            last_frame_at: None,
            state: FleetLinkState::Disconnected { since: instant(0) },
            last_drop: None,
            last_failure: None,
        })
        .await?;
    for mutation in [
        "UPDATE fleet_link_state SET state_kind = 'future'",
        "UPDATE fleet_link_state SET state_since = 'invalid-time'",
        "UPDATE fleet_link_state SET instance = '-1'",
        "UPDATE fleet_link_state SET connection = '01'",
        "UPDATE fleet_link_state SET revision = '18446744073709551616'",
        "UPDATE fleet_link_state SET connecting_attempt = 0",
        "UPDATE fleet_link_state SET last_drop_kind = 'idleTimeout'",
        "UPDATE fleet_link_state SET refusal_detail = 'unexpected'",
        "UPDATE fleet_link_state SET last_failure_at = '2026-11-06T12:00:00Z'",
        "UPDATE fleet_link_state SET created_at = 'not-a-time'",
    ] {
        sqlx::query(mutation).execute(&cache.database.pool).await?;
        assert!(
            cache.fleet_link().await.is_err(),
            "accepted invalid stored {mutation}"
        );
        sqlx::query("DELETE FROM fleet_link_state")
            .execute(&cache.database.pool)
            .await?;
        cache
            .save_fleet_link(&FleetLinkRecord {
                peer,
                endpoint: "wss://central.example/session".to_owned(),
                instance: 0,
                connection: 0,
                revision: 0,
                last_frame_at: None,
                state: FleetLinkState::Disconnected { since: instant(0) },
                last_drop: None,
                last_failure: None,
            })
            .await?;
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
        session_id: fleet_protocol::SessionId::new(uuid::Uuid::now_v7())?,
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
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fleet_session")
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

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn malformed_attempt_epoch_is_rejected_without_selecting_a_second_identity() -> TestResult {
    let root = tempfile::tempdir()?;
    let cache = FleetCache::open(&root.path().join("fleet-cache.sqlite")).await?;
    let project = ProjectId::generate();
    let epoch = Epoch::new(1)?;
    cache.snapshot_install_attempt(&project, epoch).await?;
    sqlx::query("UPDATE fleet_install_attempts SET epoch = '01'")
        .execute(&cache.database.pool)
        .await?;
    assert!(
        cache
            .snapshot_install_attempt(&project, epoch)
            .await
            .is_err(),
        "invalid stored epoch was hidden by selecting a second attempt"
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fleet_install_attempts")
        .fetch_one(&cache.database.pool)
        .await?;
    assert_eq!(count, 1);
    cache.close().await;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn malformed_session_columns_and_duplicate_singletons_are_rejected() -> TestResult {
    let root = tempfile::tempdir()?;
    let cache = FleetCache::open(&root.path().join("fleet-cache.sqlite")).await?;
    let state = ClientSessionState {
        session_id: fleet_protocol::SessionId::new(uuid::Uuid::now_v7())?,
        notice_cursor: NoticePosition::new(u64::MAX),
    };
    for statement in [
        "UPDATE fleet_session SET notice_cursor = '01'",
        "UPDATE fleet_session SET handled_through = '-1'",
        "UPDATE fleet_session SET session_id = '550e8400-e29b-41d4-a716-446655440000'",
        "UPDATE fleet_session SET created_at = 'invalid'",
    ] {
        cache.save_machine_session_state(&state).await?;
        sqlx::query(statement).execute(&cache.database.pool).await?;
        assert!(
            cache.machine_session_state().await.is_err(),
            "accepted {statement}"
        );
        assert!(
            cache.machine_notice_progress().await.is_err(),
            "partially accepted {statement}"
        );
        sqlx::query("DELETE FROM fleet_session")
            .execute(&cache.database.pool)
            .await?;
    }
    cache.save_machine_session_state(&state).await?;
    let second = uuid::Uuid::now_v7().to_string();
    sqlx::query("INSERT INTO fleet_session SELECT ?, notice_cursor, handled_through, created_at, updated_at FROM fleet_session").bind(second).execute(&cache.database.pool).await?;
    assert!(cache.machine_session_state().await.is_err());
    assert!(cache.save_machine_session_state(&state).await.is_err());
    cache.close().await;
    Ok(())
}
