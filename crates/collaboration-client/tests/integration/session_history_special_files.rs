#![cfg(unix)]

use collaboration_client::session_catalog::{
    SessionHistorySource, read_session_conversation_history,
};
use std::{
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct HistoryFixture(PathBuf);

impl Drop for HistoryFixture {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn history_rejects_fifo_without_waiting_for_a_writer() -> TestResult {
    let suffix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let root = std::env::temp_dir().join(format!(
        "collaboration-history-fifo-{}-{suffix}",
        std::process::id()
    ));
    std::fs::DirBuilder::new().mode(0o700).create(&root)?;
    let fixture = HistoryFixture(root);
    let sessions = fixture.0.join("codex-home/sessions");
    std::fs::create_dir_all(&sessions)?;
    if !Command::new("mkfifo")
        .arg(sessions.join("rollout.jsonl"))
        .status()?
        .success()
    {
        return Err("could not create owned FIFO fixture".into());
    }

    // A missing regular-file guard must fail the test, not hang the test runner.
    let child_stdout_path = fixture.0.join("history-child.stdout");
    let child_namespace = module_path!()
        .split_once("::")
        .map_or(module_path!(), |(_, namespace)| namespace);
    let child_selector = format!("{child_namespace}::fifo_history_child");
    let mut child = tokio::process::Command::new(std::env::current_exe()?)
        .args(["--ignored", "--exact", &child_selector])
        .env("COLLABORATION_HISTORY_FIXTURE_ROOT", &fixture.0)
        .stdin(Stdio::null())
        .stdout(std::fs::File::create(&child_stdout_path)?)
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;
    match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
        Ok(status) => {
            if status?.success() {
                let stdout = std::fs::read_to_string(&child_stdout_path)?;
                if stdout.contains("test result: ok. 1 passed;") {
                    Ok(())
                } else {
                    Err(
                        format!("history child selector must complete exactly one case: {stdout}")
                            .into(),
                    )
                }
            } else {
                Err("history child did not report unavailable for a FIFO".into())
            }
        }
        Err(_) => {
            child.start_kill()?;
            let _reaped = child.wait().await?;
            Err("history reader blocked on a FIFO with no writer".into())
        }
    }
}

#[test]
#[ignore = "subprocess helper invoked by the FIFO boundary test"]
fn fifo_history_child() -> TestResult {
    let root = PathBuf::from(
        std::env::var_os("COLLABORATION_HISTORY_FIXTURE_ROOT")
            .ok_or("missing owned history fixture")?,
    );
    let source = SessionHistorySource::new(
        root.join("codex-home/sessions/rollout.jsonl")
            .to_string_lossy()
            .into_owned(),
        root.join("codex-home"),
    );
    let history = read_session_conversation_history(Some(&source));
    if history.unavailable_reason.as_deref() != Some("history unavailable")
        || !history.snippets.is_empty()
    {
        return Err("nonregular history must be unavailable".into());
    }
    Ok(())
}
