use super::*;
use fleet_sync_protocol::{Epoch, InstallAttemptId, NoticePosition, SessionId};
use std::sync::{Arc, Mutex};
use tracing::instrument::WithSubscriber;
type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

const CRASH_ROOT: &str = "CODEX_ROUTER_TEST_RUNTIME_IMPORT_CRASH_ROOT";

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn malformed_and_extra_field_orphans_are_discarded_without_logging_raw_bytes() -> TestResult {
    for (contents, readable) in [
        ("{truncated orphan fixture-secret", None),
        (
            r#"{"handledThrough":73,"unboundField":"fixture-secret"}"#,
            Some("73"),
        ),
        (r#"{"handledThrough":"fixture-secret"}"#, None),
        ("{}", None),
    ] {
        let root = tempfile::tempdir()?;
        tokio::fs::create_dir(root.path().join("machine-session")).await?;
        let source = root
            .path()
            .join("machine-session/machine-notice-progress.json");
        tokio::fs::write(&source, contents).await?;
        let cache_store =
            crate::FleetStore::open(&root.path().join(crate::FleetStore::FILE_NAME)).await?;
        let cache = cache_store.cache();
        let capture = WarningCapture::default();
        let result = cache
            .import_legacy_runtime(root.path())
            .with_subscriber(capture.clone())
            .await;
        assert!(
            result.is_ok(),
            "orphan contents blocked Host startup: {result:?}"
        );
        assert_eq!(cache.machine_session_state().await?, None);
        assert_eq!(
            cache.machine_notice_progress().await?,
            NoticePosition::new(0)
        );
        assert!(!source.exists());
        {
            let events = capture.0.lock().unwrap();
            assert!(events.iter().any(|(name, value)| name == "handled_through"
                && readable.map_or_else(
                    || value.contains("unavailable"),
                    |expected| value == expected
                )));
            assert!(!format!("{events:?}").contains("fixture-secret"));
        }
        cache_store.close().await;
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn killed_importer_after_commit_reopens_without_losing_rows_or_overwriting_progress()
-> TestResult {
    use std::os::unix::process::ExitStatusExt;
    use tokio::io::{AsyncBufReadExt, BufReader};
    let root = tempfile::tempdir()?;
    tokio::fs::create_dir(root.path().join("machine-session")).await?;
    let state = ClientSessionState {
        session_id: SessionId::new(uuid::Uuid::now_v7())?,
        notice_cursor: NoticePosition::new(19),
    };
    let source = root.path().join("machine-session/state.json");
    let progress = root
        .path()
        .join("machine-session/machine-notice-progress.json");
    tokio::fs::write(&source, serde_json::to_vec(&state)?).await?;
    tokio::fs::write(&progress, br#"{"handledThrough":17}"#).await?;
    let namespace = module_path!()
        .split_once("::")
        .ok_or("test namespace missing")?
        .1;
    let selector = format!("{namespace}::import_crash_child");
    let mut child = tokio::process::Command::new(std::env::current_exe()?)
        .args(["--exact", &selector, "--ignored", "--nocapture"])
        .env(CRASH_ROOT, root.path())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut lines = BufReader::new(child.stdout.take().ok_or("child stdout missing")?).lines();
    let entered = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        while let Some(line) = lines.next_line().await? {
            if line == "RUNTIME_IMPORT_COMMITTED_FILES_RETAINED" {
                return Ok::<_, std::io::Error>(());
            }
        }
        Err(std::io::Error::other(
            "import child selector did not reach its commit checkpoint",
        ))
    })
    .await;
    // Always kill and reap before inspecting the outcome, including a bad selector.
    child.kill().await?;
    let status = child.wait().await?;
    entered??;
    assert_eq!(
        status.signal(),
        Some(9),
        "import child was not actually killed"
    );
    assert!(source.exists() && progress.exists());
    let cache_store =
        crate::FleetStore::open(&root.path().join(crate::FleetStore::FILE_NAME)).await?;
    let cache = cache_store.cache();
    cache.import_legacy_runtime(root.path()).await?;
    assert_eq!(
        cache
            .machine_session_state()
            .await?
            .ok_or("committed session lost")?
            .session_id,
        state.session_id
    );
    assert_eq!(
        cache
            .machine_session_state()
            .await?
            .ok_or("committed cursor lost")?
            .notice_cursor,
        NoticePosition::new(31)
    );
    assert_eq!(
        cache.machine_notice_progress().await?,
        NoticePosition::new(29)
    );
    assert!(!source.exists() && !progress.exists());
    cache_store.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "owned child for the importer crash parent"]
async fn import_crash_child() -> TestResult {
    use std::io::Write as _;
    let root = PathBuf::from(std::env::var_os(CRASH_ROOT).ok_or("import crash root missing")?);
    let cache_store = crate::FleetStore::open(&root.join(crate::FleetStore::FILE_NAME)).await?;
    let cache = cache_store.cache();
    let pending = cache.commit_legacy_runtime(&root).await?;
    if pending.len() != 2 {
        return Err("child fixture did not import both files".into());
    }
    let mut state = cache
        .machine_session_state()
        .await?
        .ok_or("child session missing")?;
    state.notice_cursor = NoticePosition::new(31);
    cache.save_machine_session_state(&state).await?;
    cache
        .save_machine_notice_progress(NoticePosition::new(29))
        .await?;
    println!("RUNTIME_IMPORT_COMMITTED_FILES_RETAINED");
    std::io::stdout().flush()?;
    std::future::pending::<()>().await;
    Err("crash child escaped its checkpoint".into())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn failed_attempt_insert_rolls_back_every_legacy_row_and_preserves_all_files() -> TestResult {
    let root = tempfile::tempdir()?;
    tokio::fs::create_dir(root.path().join("machine-session")).await?;
    tokio::fs::create_dir(root.path().join("snapshot-install-attempts")).await?;
    let state = ClientSessionState {
        session_id: SessionId::new(uuid::Uuid::now_v7())?,
        notice_cursor: NoticePosition::new(7),
    };
    let attempt = SnapshotInstallAttempt {
        project: message_board::ProjectId::generate(),
        epoch: Epoch::new(1)?,
        attempt: InstallAttemptId::new(uuid::Uuid::now_v7())?,
    };
    let session = root.path().join("machine-session/state.json");
    let progress = root
        .path()
        .join("machine-session/machine-notice-progress.json");
    let install = root.path().join(format!(
        "snapshot-install-attempts/{}-e1.json",
        attempt.project.as_str()
    ));
    tokio::fs::write(&session, serde_json::to_vec(&state)?).await?;
    tokio::fs::write(&progress, br#"{"handledThrough":5}"#).await?;
    tokio::fs::write(&install, serde_json::to_vec(&attempt)?).await?;
    let cache_store =
        crate::FleetStore::open(&root.path().join(crate::FleetStore::FILE_NAME)).await?;
    let cache = cache_store.cache();
    sqlx::raw_sql("CREATE TRIGGER fixture_import_failure BEFORE INSERT ON fleet_install_attempts BEGIN SELECT RAISE(ABORT,'fixture import commit failure'); END").execute(&cache.database.pool).await?;
    assert!(cache.import_legacy_runtime(root.path()).await.is_err());
    assert_eq!(cache.machine_session_state().await?, None);
    assert_eq!(
        cache.machine_notice_progress().await?,
        NoticePosition::new(0)
    );
    assert!(session.exists() && progress.exists() && install.exists());
    sqlx::query("DROP TRIGGER fixture_import_failure")
        .execute(&cache.database.pool)
        .await?;
    cache.import_legacy_runtime(root.path()).await?;
    assert_eq!(cache.machine_session_state().await?, Some(state));
    assert_eq!(
        cache.machine_notice_progress().await?,
        NoticePosition::new(5)
    );
    assert_eq!(
        cache
            .snapshot_install_attempt(&attempt.project, attempt.epoch)
            .await?,
        attempt.attempt
    );
    assert!(!session.exists() && !progress.exists() && !install.exists());
    cache_store.close().await;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn interrupted_import_cleanup_retries_without_overwriting_newer_committed_state() -> TestResult
{
    // Arrange all legacy pieces with valid native identities and unsigned limits.
    let root = tempfile::tempdir()?;
    let session_home = root.path().join("machine-session");
    let attempts = root.path().join("snapshot-install-attempts");
    tokio::fs::create_dir(&session_home).await?;
    tokio::fs::create_dir(&attempts).await?;
    let original = ClientSessionState {
        session_id: SessionId::new(uuid::Uuid::now_v7())?,
        notice_cursor: NoticePosition::new(u64::MAX - 1),
    };
    let attempt = SnapshotInstallAttempt {
        project: message_board::ProjectId::generate(),
        epoch: Epoch::new(u64::MAX)?,
        attempt: InstallAttemptId::new(uuid::Uuid::now_v7())?,
    };
    let session = session_home.join("state.json");
    let progress = session_home.join("machine-notice-progress.json");
    let install = attempts.join(format!(
        "{}-e{}.json",
        attempt.project.as_str(),
        attempt.epoch.value()
    ));
    tokio::fs::write(&session, serde_json::to_vec(&original)?).await?;
    tokio::fs::write(&progress, br#"{"handledThrough":73}"#).await?;
    tokio::fs::write(&install, serde_json::to_vec(&attempt)?).await?;
    let path = root.path().join(crate::FleetStore::FILE_NAME);
    let cache_store = crate::FleetStore::open(&path).await?;
    let cache = cache_store.cache();

    // Act: stop at the actual commit/cleanup boundary, exactly the durable
    // state left by a process dying after commit and before unlinking files.
    let pending_cleanup = cache.commit_legacy_runtime(root.path()).await?;
    assert_eq!(pending_cleanup.len(), 3);
    assert!(session.exists() && progress.exists() && install.exists());
    assert_eq!(cache.machine_session_state().await?, Some(original.clone()));
    assert_eq!(
        cache.machine_notice_progress().await?,
        NoticePosition::new(73)
    );
    let newer = ClientSessionState {
        notice_cursor: NoticePosition::new(u64::MAX),
        ..original
    };
    cache.save_machine_session_state(&newer).await?;
    cache
        .save_machine_notice_progress(NoticePosition::new(91))
        .await?;
    cache_store.close().await;
    let cache_store = crate::FleetStore::open(&path).await?;
    let cache = cache_store.cache();
    cache.import_legacy_runtime(root.path()).await?;

    // Assert real reopen/retry preserves both cursors and the exact attempt.
    assert_eq!(cache.machine_session_state().await?, Some(newer));
    assert_eq!(
        cache.machine_notice_progress().await?,
        NoticePosition::new(91)
    );
    assert_eq!(
        cache
            .snapshot_install_attempt(&attempt.project, attempt.epoch)
            .await?,
        attempt.attempt
    );
    assert!(!session.exists() && !progress.exists() && !install.exists());
    cache_store.close().await;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn orphan_import_warns_with_its_value_and_leaves_no_session_row() -> TestResult {
    let root = tempfile::tempdir()?;
    tokio::fs::create_dir(root.path().join("machine-session")).await?;
    let source = root
        .path()
        .join("machine-session/machine-notice-progress.json");
    tokio::fs::write(&source, br#"{"handledThrough":18446744073709551615}"#).await?;
    let cache_store =
        crate::FleetStore::open(&root.path().join(crate::FleetStore::FILE_NAME)).await?;
    let cache = cache_store.cache();
    let capture = WarningCapture::default();
    cache
        .import_legacy_runtime(root.path())
        .with_subscriber(capture.clone())
        .await?;
    assert!(
        capture
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|(name, value)| name == "event"
                && value.contains("fleet.runtime.orphanNoticeProgressDiscarded"))
    );
    assert!(
        capture
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|(name, value)| name == "handled_through" && value == "18446744073709551615")
    );
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fleet_session")
        .fetch_one(&cache.database.pool)
        .await?;
    assert_eq!(rows, 0);
    assert_eq!(cache.machine_session_state().await?, None);
    assert_eq!(
        cache.machine_notice_progress().await?,
        NoticePosition::new(0)
    );
    assert!(!source.exists());
    cache_store.close().await;
    Ok(())
}

#[derive(Clone, Default)]
struct WarningCapture(Arc<Mutex<Vec<(String, String)>>>);
impl tracing::Subscriber for WarningCapture {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        *metadata.level() == tracing::Level::WARN
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        event.record(&mut self.clone());
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}
impl tracing::field::Visit for WarningCapture {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0
            .lock()
            .unwrap()
            .push((field.name().to_owned(), format!("{value:?}")));
    }
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.0
            .lock()
            .unwrap()
            .push((field.name().to_owned(), value.to_string()));
    }
}
