//! Real process shutdown driven through its original grace boundary.
use super::*;

#[path = "../support/native_file_observation.rs"]
mod native_file_observation;

#[tokio::test]
async fn app_server_force_escalation_signals_and_reaps_once()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("forced")?;
    let event_file = directory.path().join("events.log");
    let identity = executable_identity(&std::env::current_exe()?).await?;
    let mut command = signal_fixture_command(SignalFixtureMode::IgnoreTerminate, &event_file)?;
    let process = ProcessGroupChild::spawn(&mut command)?;
    wait_for_fixture_ready(&event_file).await?;
    let mut app_server = AppServerChild::new(process, identity);

    let fixture_deadlines =
        AppServerShutdownDeadlines::new(Duration::from_millis(50), Duration::from_secs(2))
            .ok_or("fixture shutdown deadlines must be ordered")?;
    let mut signals = native_file_observation::NativeFileObservation::subscribe(&event_file)?;
    let outcome = {
        let shutdown = app_server.shutdown_with_deadlines(fixture_deadlines);
        tokio::pin!(shutdown);
        tokio::select! {
            result = &mut shutdown => {
                result?;
                return Err("shutdown completed before the child acknowledged SIGTERM".into());
            }
            observed = signals.wait_for_contents("sigterm\n", Duration::from_secs(2)) => observed?,
        }
        let pending_before_grace = futures_util::poll!(shutdown.as_mut()).is_pending();
        tokio::time::pause();
        tokio::time::advance(Duration::from_millis(51)).await;
        tokio::time::resume();
        let outcome = shutdown.await?;
        if !pending_before_grace {
            return Err("SIGTERM-ignoring shutdown settled before controlled grace".into());
        }
        outcome
    };
    check_equal(
        outcome,
        ShutdownOutcome::Killed,
        "SIGTERM-ignoring app-server must use the kill backstop",
    )?;
    check_equal(
        app_server.expected_exit().map(ExpectedExit::term_sent),
        Some(true),
        "force path must retain the first-signal progress",
    )?;
    check_equal(
        app_server.expected_exit().map(ExpectedExit::kill_sent),
        Some(true),
        "force path must retain the one SIGKILL escalation",
    )?;
    check(
        std::fs::read_to_string(&event_file)?.starts_with("ready\nsigterm\n"),
        "fixture must observe the initial TERM before SIGKILL",
    )?;
    Ok(())
}

#[tokio::test]
async fn app_server_force_escalation_kills_the_complete_process_group()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("app-server-process-group")?;
    let process_log = directory.path().join("processes.log");
    std::fs::write(&process_log, b"")?;
    let identity = executable_identity(&std::env::current_exe()?).await?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("--exact")
        .arg("app_server_group_parent_child_entrypoint")
        .arg("--nocapture")
        .env("CODEX_ROUTER_HOST_APP_SERVER_GROUP_PARENT", "1")
        .env("CODEX_ROUTER_HOST_APP_SERVER_GROUP_LOG", &process_log)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let process = ProcessGroupChild::spawn(&mut command)?;
    let mut app_server = AppServerChild::new(process, identity);
    let process_ids = wait_for_process_ids(&process_log, 2).await?;

    let fixture_deadlines =
        AppServerShutdownDeadlines::new(Duration::from_millis(50), Duration::from_secs(2))
            .ok_or("fixture shutdown deadlines must be ordered")?;
    let outcome = {
        let shutdown = app_server.shutdown_with_deadlines(fixture_deadlines);
        tokio::pin!(shutdown);
        if futures_util::poll!(shutdown.as_mut()).is_ready() {
            return Err("owned process group settled before its controlled kill grace".into());
        }
        // Both children acknowledged readiness; the actual shutdown poll sent
        // SIGTERM and armed its existing grace before this explicit advance.
        tokio::time::pause();
        tokio::time::advance(Duration::from_millis(51)).await;
        tokio::time::resume();
        shutdown.await?
    };
    check_equal(
        outcome,
        ShutdownOutcome::Killed,
        "SIGTERM-ignoring app-server must be force-killed",
    )?;
    wait_for_process_exit(process_ids[1]).await?;
    Ok(())
}
