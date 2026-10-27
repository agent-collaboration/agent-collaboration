//! The parent test kills a separate native writer process after its SQL writes.
use crate::ProjectStore;
use message_board::ProjectId;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn killed_writer_leaves_no_record_feed_checkpoint_or_captured_change() -> TestResult {
    use sqlx::{ConnectOptions, Connection};
    use std::{
        io::{BufRead, BufReader},
        process::{Command, Stdio},
        time::Duration,
    };
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("crash.sqlite");
    let project_id = ProjectId::generate();
    ProjectStore::open(&path, project_id.clone())
        .await?
        .close()
        .await;
    let mut child = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "project_crash_tests::child_writer_waits_after_uncommitted_record",
            "--ignored",
            "--nocapture",
        ])
        .env("M2_CRASH_PROJECT_PATH", &path)
        .env("M2_CRASH_PROJECT_ID", project_id.as_str())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().ok_or("child stdout unavailable")?;
    let ready = tokio::task::spawn_blocking(move || -> std::io::Result<bool> {
        for line in BufReader::new(stdout).lines() {
            if line? == "M2_WRITER_HAS_UNCOMMITTED_RECORD" {
                return Ok(true);
            }
        }
        Ok(false)
    });
    let readiness = tokio::time::timeout(Duration::from_secs(10), ready).await;
    child.kill()?;
    let result = child.wait_with_output()?;
    assert!(
        !result.status.success(),
        "writer must be killed, not exit normally"
    );
    assert!(
        matches!(readiness, Ok(Ok(Ok(true)))),
        "writer did not reach uncommitted marker: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let reopened = ProjectStore::open(&path, project_id.clone()).await?;
    let mut read = reopened.snapshot().await?;
    assert_eq!(read.high_water().position.get(), 0);
    assert_eq!(read.record(&project_id.clone().into()).await?, None);
    read.finish().await?;
    let mut diagnostic = sqlx_turso::TursoConnectOptions::new()
        .filename(&path)
        .connect()
        .await?;
    let captured: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM turso_cdc WHERE table_name IN ('project_state','project_feed','epoch_lines') AND change_type <> 2").fetch_one(&mut diagnostic).await?;
    assert_eq!(
        captured, 0,
        "the killed admission committed no application changes"
    );
    diagnostic.close().await?;
    let mut next = reopened
        .begin_admission(crate::project_test_support::origin())
        .await?;
    assert_eq!(
        next.append_record(crate::project_store_tests::project_record(project_id))
            .await?
            .position
            .get(),
        1
    );
    next.commit().await?;
    reopened.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "subprocess fixture, invoked and killed by the parent crash scenario"]
async fn child_writer_waits_after_uncommitted_record() -> TestResult {
    use std::io::Write;
    let path = std::path::PathBuf::from(
        std::env::var_os("M2_CRASH_PROJECT_PATH").ok_or("child project path required")?,
    );
    let project_id = ProjectId::try_from(std::env::var("M2_CRASH_PROJECT_ID")?)?;
    let store = ProjectStore::open(&path, project_id.clone()).await?;
    let mut transaction = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    transaction
        .append_record(crate::project_store_tests::project_record(project_id))
        .await?;
    println!("M2_WRITER_HAS_UNCOMMITTED_RECORD");
    std::io::stdout().flush()?;
    std::future::pending::<()>().await;
    transaction.commit().await?;
    Ok(())
}
