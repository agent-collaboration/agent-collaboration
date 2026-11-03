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
const CHILD_TEST: &str = "enrollment_reset::cache_tests::cache_reset_process_child";

async fn connection(path: &Path) -> Result<TursoConnection, sqlx::Error> {
    TursoConnection::connect_with(&TursoConnectOptions::new().filename(path)).await
}

async fn seed_link_store(storage: &Path) -> TestResult {
    tokio::fs::create_dir_all(storage).await?;
    let path = storage.join("fleet-cache.sqlite");
    message_board_storage::FleetCache::open(&path)
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
    let path = storage.join("fleet-cache.sqlite");
    assert!(path.exists(), "reset deleted the shared Router link store");
    let cache = message_board_storage::FleetCache::open(&path).await?;
    assert_eq!(
        cache.checkpoint().await?,
        message_board::FleetPosition::ZERO
    );
    cache.close().await;
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
    drop(record_pending_enrollment_reset(&journal)?);
    // Act through the actual reset owner.
    resume_enrollment_reset(&journal, &storage).await?;
    // Assert only learned cache data disappeared.
    assert_link_store(&storage).await?;
    assert!(!reset_pending_in_directory(&journal)?);
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

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; setup propagates errors."
)]
async fn cache_reset_sigkill_resumes_and_keeps_the_non_cache_login_row() -> TestResult {
    // Arrange a private checkpoint socket and one child-owned reset.
    let root = tempfile::Builder::new()
        .prefix("c5cache-")
        .tempdir_in("/tmp")?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    seed_link_store(&root.path().join("storage")).await?;
    drop(record_pending_enrollment_reset(
        &root.path().join("enrollment"),
    )?);
    let socket = root.path().join("reset.sock");
    let listener = tokio::net::UnixListener::bind(&socket)?;
    let log = std::fs::File::create(root.path().join("child.log"))?;
    let mut child = tokio::process::Command::new(std::env::current_exe()?)
        .args(["--ignored", "--exact", CHILD_TEST, "--nocapture"])
        .env(CHILD_ROOT, root.path())
        .env("CODEX_ROUTER_DEBUG_ENROLLMENT_RESET_BARRIER_SOCKET", socket)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .kill_on_drop(true)
        .spawn()?;
    let checkpoint = tokio::time::timeout(BOUND, async {
        let (mut stream, _) = listener.accept().await?;
        let mut marker = vec![0; b"router-state-cleared\n".len()];
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
    assert_eq!(checkpoint.1, b"router-state-cleared\n");
    assert_eq!(status.signal(), Some(9));
    drop(checkpoint);
    let journal = root.path().join("enrollment");
    assert!(matches!(
        load_reset(&journal)?.ok_or("reset journal")?.phase,
        ResetPhase::RouterStateCleared
    ));
    // Assert the owning reopen and ordinary restart finish the same reset.
    assert_link_store(&root.path().join("storage")).await?;
    resume_enrollment_reset(&journal, &root.path().join("storage")).await?;
    assert!(!reset_pending_in_directory(&journal)?);
    assert_link_store(&root.path().join("storage")).await?;
    Ok(())
}
