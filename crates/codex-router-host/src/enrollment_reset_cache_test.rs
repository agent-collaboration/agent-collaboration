//! The Router link store keeps its non-cache facts across reset and process loss.
use super::*;
use sqlx::Connection;
use sqlx_turso::{TursoConnectOptions, TursoConnection};
use std::{
    os::unix::{fs::PermissionsExt, process::ExitStatusExt},
    process::Stdio,
    time::Duration,
};
use tokio::io::AsyncReadExt;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
const BOUND: Duration = Duration::from_secs(30);
const CHILD_ROOT: &str = "CODEX_ROUTER_TEST_CACHE_RESET_ROOT";
fn child_test_selector() -> String {
    let module = module_path!()
        .split_once("::")
        .expect("test module has crate")
        .1;
    format!("{module}::cache_reset_process_child")
}

async fn seed_status_cursor(storage: &Path) -> TestResult {
    interaction_storage::InteractionStore::open(storage.join("interaction.sqlite"))
        .await?
        .close()
        .await?;
    let mut writer = connection(&storage.join("interaction.sqlite")).await?;
    sqlx::query("INSERT INTO _fleet_sync_outbox_status(singleton,cursor,created_at,updated_at) VALUES(1,100,1,1)")
        .execute(&mut writer).await?;
    writer.close().await?;
    Ok(())
}

async fn assert_status_cursor_cleared(storage: &Path) -> TestResult {
    let mut reader = connection(&storage.join("interaction.sqlite")).await?;
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM _fleet_sync_outbox_status")
        .fetch_one(&mut reader)
        .await?;
    assert_eq!(
        rows, 0,
        "the previous central mail stream cursor survived reset"
    );
    reader.close().await?;
    Ok(())
}

async fn connection(path: &Path) -> Result<TursoConnection, sqlx::Error> {
    TursoConnection::connect_with(&TursoConnectOptions::new().filename(path)).await
}

async fn seed_link_store(storage: &Path) -> TestResult {
    tokio::fs::create_dir_all(storage).await?;
    let path = storage.join(message_board_storage::FleetStore::FILE_NAME);
    message_board_storage::FleetStore::open(&path)
        .await?
        .close()
        .await;
    let mut writer = connection(&path).await?;
    let key = collaboration_protocol::OperationId::generate();
    // Cache bodies are deliberately opaque to reset: even an obsolete body is disposable.
    sqlx::query("INSERT INTO cached_fleet_records(kind,id,payload) VALUES('workspace',?,'{}')")
        .bind(key.as_str())
        .execute(&mut writer)
        .await?;
    sqlx::query("INSERT INTO fleet_cache_rebuild_records(row_id,id,kind,payload) VALUES(?,?,'workspace','{}')")
        .bind(key.as_str()).bind(key.as_str()).execute(&mut writer).await?;
    sqlx::query("UPDATE fleet_cache_checkpoint SET position=9 WHERE singleton=1")
        .execute(&mut writer)
        .await?;
    // This test-only row stands in for login/session registrations owned by another lane.
    sqlx::raw_sql("CREATE TABLE test_login_state(id TEXT PRIMARY KEY NOT NULL,value TEXT NOT NULL,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL)")
        .execute(&mut writer).await?;
    sqlx::query("INSERT INTO test_login_state VALUES(?,'retained login state',1,1)")
        .bind(key.as_str())
        .execute(&mut writer)
        .await?;
    writer.close().await?;
    Ok(())
}

async fn assert_link_store(storage: &Path) -> TestResult {
    let path = storage.join(message_board_storage::FleetStore::FILE_NAME);
    assert!(path.exists(), "reset deleted the shared Router link store");
    let cache_store = message_board_storage::FleetStore::open(&path).await?;
    let cache = cache_store.cache();
    assert_eq!(
        cache.checkpoint().await?,
        message_board::FleetPosition::ZERO
    );
    cache_store.close().await;
    let mut reader = connection(&path).await?;
    for (table, statement) in [
        (
            "cached_fleet_records",
            "SELECT count(*) FROM cached_fleet_records",
        ),
        (
            "fleet_cache_rebuild_records",
            "SELECT count(*) FROM fleet_cache_rebuild_records",
        ),
    ] {
        let count: i64 = sqlx::query_scalar(statement).fetch_one(&mut reader).await?;
        assert_eq!(count, 0, "central cache table survived: {table}");
    }
    let value: String = sqlx::query_scalar("SELECT value FROM test_login_state")
        .fetch_one(&mut reader)
        .await?;
    assert_eq!(value, "retained login state");
    reader.close().await?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; setup propagates errors."
)]
async fn enrollment_reset_clears_only_cache_rows_and_preserves_non_cache_tables() -> TestResult {
    // Arrange one real migrated link store with cache rows and a foreign-owned fact.
    let root = tempfile::tempdir()?;
    let storage = root.path().join("storage");
    let journal = root.path().join("enrollment");
    seed_link_store(&storage).await?;
    seed_status_cursor(&storage).await?;
    drop(record_pending_enrollment_reset(&journal)?);
    // Act through the actual reset owner.
    resume_enrollment_reset(&journal, &storage).await?;
    // Assert only learned cache data disappeared.
    assert_link_store(&storage).await?;
    assert_status_cursor_cleared(&storage).await?;
    assert!(!reset_pending_in_directory(&journal)?);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn reenrollment_preserves_unfinished_typed_and_legacy_attempts_and_clears_session_link()
-> TestResult {
    let root = tempfile::tempdir()?;
    let storage = root.path().join("storage");
    let journal = root.path().join("enrollment");
    tokio::fs::create_dir_all(storage.join("machine-session")).await?;
    tokio::fs::create_dir_all(storage.join("snapshot-install-attempts")).await?;
    // Reset discards incompatible old resume bytes; it must not start decoding
    // the state it was explicitly instructed to forget merely to preserve attempts.
    tokio::fs::write(
        storage.join("machine-session/state.json"),
        b"incompatible old state",
    )
    .await?;
    let cache_store = message_board_storage::FleetStore::open(
        &storage.join(message_board_storage::FleetStore::FILE_NAME),
    )
    .await?;
    let cache = cache_store.cache();
    let native = fleet_sync_session::ClientSessionState {
        session_id: fleet_sync_protocol::SessionId::new(uuid::Uuid::now_v7())?,
        notice_cursor: fleet_sync_protocol::NoticePosition::new(19),
    };
    cache.save_machine_session_state(&native).await?;
    cache
        .save_machine_notice_progress(fleet_sync_protocol::NoticePosition::new(17))
        .await?;
    cache
        .save_fleet_link(&board_sync_protocol::FleetLinkRecord {
            peer: fleet_sync_protocol::PeerId::new(uuid::Uuid::now_v7())?,
            endpoint: "wss://central.example/session".to_owned(),
            instance: u64::MAX,
            connection: 4,
            revision: 99,
            last_frame_at: Some(chrono::Utc::now()),
            state: board_sync_protocol::FleetLinkState::Disconnected {
                since: chrono::Utc::now(),
            },
            last_drop: None,
            last_failure: None,
        })
        .await?;
    let project = message_board::ProjectId::generate();
    let epoch = fleet_sync_protocol::Epoch::new(3)?;
    let typed = cache.snapshot_install_attempt(&project, epoch).await?;
    cache_store.close().await;
    let legacy = message_board_storage::SnapshotInstallAttempt {
        project: message_board::ProjectId::generate(),
        epoch,
        attempt: fleet_sync_protocol::InstallAttemptId::new(uuid::Uuid::now_v7())?,
    };
    let legacy_path = storage.join(format!(
        "snapshot-install-attempts/{}-e3.json",
        legacy.project.as_str()
    ));
    tokio::fs::write(&legacy_path, serde_json::to_vec(&legacy)?).await?;
    drop(record_pending_enrollment_reset(&journal)?);

    resume_enrollment_reset(&journal, &storage).await?;

    let cache_store = message_board_storage::FleetStore::open(
        &storage.join(message_board_storage::FleetStore::FILE_NAME),
    )
    .await?;
    let cache = cache_store.cache();
    assert_eq!(cache.machine_session_state().await?, None);
    assert_eq!(
        cache.machine_notice_progress().await?,
        fleet_sync_protocol::NoticePosition::new(0)
    );
    assert_eq!(cache.fleet_link().await?, None);
    assert_eq!(
        cache.snapshot_install_attempt(&project, epoch).await?,
        typed
    );
    assert_eq!(
        cache
            .snapshot_install_attempt(&legacy.project, epoch)
            .await?,
        legacy.attempt,
        "reset discarded a legacy unfinished attempt before importing it"
    );
    assert!(!legacy_path.exists());
    assert!(!storage.join("machine-session/state.json").exists());
    cache_store.close().await;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture failures propagate."
)]
async fn legacy_cleared_phase_discards_old_mail_cursor_before_completing_reset() -> TestResult {
    let root = tempfile::tempdir()?;
    let storage = root.path().join("storage");
    let journal = root.path().join("enrollment");
    seed_link_store(&storage).await?;
    clear_router_state(
        &storage,
        ResetFleetStore::CreateWhenMissing(
            &storage.join(message_board_storage::FleetStore::FILE_NAME),
        ),
    )
    .await?;
    seed_status_cursor(&storage).await?;
    drop(record_pending_enrollment_reset(&journal)?);
    let reset = load_reset(&journal)?.ok_or("pending reset")?;
    // A previous version persisted this phase without clearing mail observations.
    persist_phase(&journal, &reset, ResetPhase::RouterStateCleared).await?;
    resume_enrollment_reset(&journal, &storage).await?;
    assert_status_cursor_cleared(&storage).await?;
    // Completion is replayed without forgetting progress from the new stream.
    seed_status_cursor(&storage).await?;
    resume_enrollment_reset(&journal, &storage).await?;
    let mut reader = connection(&storage.join("interaction.sqlite")).await?;
    let cursor: i64 =
        sqlx::query_scalar("SELECT cursor FROM _fleet_sync_outbox_status WHERE singleton=1")
            .fetch_one(&mut reader)
            .await?;
    assert_eq!(cursor, 100);
    reader.close().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "Task-owned child, invoked only by the cache-reset crash parent"]
async fn cache_reset_process_child() -> TestResult {
    let root =
        std::path::PathBuf::from(std::env::var_os(CHILD_ROOT).ok_or("cache reset child root")?);
    resume_enrollment_reset(&root.join("enrollment"), &root.join("storage")).await?;
    Err("reset escaped its held checkpoint".into())
}

enum ResetCrashCut {
    MailCursorCleared,
    RouterStateCleared,
}

async fn kill_reset_at(cut: ResetCrashCut) -> TestResult {
    // Arrange a private checkpoint socket and one child-owned reset.
    let root = tempfile::Builder::new()
        .prefix("c5cache-")
        .tempdir_in("/tmp")?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    seed_link_store(&root.path().join("storage")).await?;
    seed_status_cursor(&root.path().join("storage")).await?;
    drop(record_pending_enrollment_reset(
        &root.path().join("enrollment"),
    )?);
    let socket = root.path().join("reset.sock");
    let listener = tokio::net::UnixListener::bind(&socket)?;
    let log = std::fs::File::create(root.path().join("child.log"))?;
    let (barrier, expected_marker) = match cut {
        ResetCrashCut::MailCursorCleared => (
            "CODEX_ROUTER_TEST_MAIL_CURSOR_RESET_BARRIER_SOCKET",
            b"mail-status-cursor-cleared\n".as_slice(),
        ),
        ResetCrashCut::RouterStateCleared => (
            "CODEX_ROUTER_DEBUG_ENROLLMENT_RESET_BARRIER_SOCKET",
            b"router-state-cleared\n".as_slice(),
        ),
    };
    let mut child = tokio::process::Command::new(std::env::current_exe()?)
        .args([
            "--ignored",
            "--exact",
            &child_test_selector(),
            "--nocapture",
        ])
        .env(CHILD_ROOT, root.path())
        .env(barrier, socket)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .kill_on_drop(true)
        .spawn()?;
    let checkpoint = tokio::time::timeout(BOUND, async {
        let (mut stream, _) = listener.accept().await?;
        let mut marker = vec![0; expected_marker.len()];
        stream.read_exact(&mut marker).await?;
        Ok::<_, io::Error>((stream, marker))
    })
    .await;
    // Act: SIGKILL only after the durable partial-reset checkpoint.
    child.kill().await?;
    let status = child.wait().await?;
    let checkpoint = checkpoint.map_err(|error| {
        format!(
            "reset checkpoint: {error}; {}",
            std::fs::read_to_string(root.path().join("child.log")).unwrap_or_default()
        )
    })??;
    assert_eq!(checkpoint.1, expected_marker);
    assert_eq!(status.signal(), Some(9));
    drop(checkpoint);
    let journal = root.path().join("enrollment");
    let phase = load_reset(&journal)?.ok_or("reset journal")?.phase;
    assert!(match cut {
        ResetCrashCut::MailCursorCleared => matches!(phase, ResetPhase::Pending),
        ResetCrashCut::RouterStateCleared => matches!(phase, ResetPhase::RouterStateCleared),
    });
    // Assert the owning reopen and ordinary restart finish the same reset.
    assert_link_store(&root.path().join("storage")).await?;
    assert_status_cursor_cleared(&root.path().join("storage")).await?;
    resume_enrollment_reset(&journal, &root.path().join("storage")).await?;
    assert!(!reset_pending_in_directory(&journal)?);
    assert_link_store(&root.path().join("storage")).await?;
    assert_status_cursor_cleared(&root.path().join("storage")).await?;
    Ok(())
}

#[tokio::test]
async fn cache_reset_sigkill_resumes_and_keeps_the_non_cache_login_row() -> TestResult {
    kill_reset_at(ResetCrashCut::RouterStateCleared).await
}

#[tokio::test]
async fn sigkill_during_mail_cursor_clearing_replays_pending_reset_without_restoring_old_cursor()
-> TestResult {
    kill_reset_at(ResetCrashCut::MailCursorCleared).await
}
