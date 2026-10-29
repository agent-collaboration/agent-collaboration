//! The synchronous harness owns actual child streams, readiness, exit and teardown.
use super::process_runtime::{spawn_router_output_reader, wait_for_router_readiness};
use super::{RouterProcessGuard, RouterProcessObservation};
use crate::FixtureResult;
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

fn held_fixture(script: &str) -> FixtureResult<RouterProcessGuard> {
    let child = Command::new("/bin/sh")
        .args(["-c", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let pid = child.id();
    Ok(RouterProcessGuard {
        child: Some(child),
        stdout_handle: None,
        stderr_handle: None,
        observation: RouterProcessObservation {
            binary_path: PathBuf::from("/bin/sh"),
            pid,
            argv: vec!["-c".to_owned(), script.to_owned()],
            listener: String::new(),
            readiness_line: String::new(),
            cleanup_result: "not-cleaned".to_owned(),
        },
    })
}

fn capture_fixture(
    guard: &mut RouterProcessGuard,
    lines: Option<mpsc::Sender<String>>,
) -> FixtureResult {
    let child = guard.child.as_mut().ok_or("owned fixture child missing")?;
    let stdout = child.stdout.take().ok_or("owned fixture stdout missing")?;
    let stderr = child.stderr.take().ok_or("owned fixture stderr missing")?;
    guard.stdout_handle = Some(spawn_router_output_reader("fixture stdout", stdout, lines)?);
    guard.stderr_handle = Some(spawn_router_output_reader("fixture stderr", stderr, None)?);
    Ok(())
}

#[test]
fn readiness_uses_the_real_stdout_receipt_then_stop_settles_the_child() -> FixtureResult {
    let mut guard =
        held_fixture("printf 'starting\\nlistening: 127.0.0.1:43210\\n'; read fixture_input")?;
    let (sent, received) = mpsc::channel();
    capture_fixture(&mut guard, Some(sent))?;
    let readiness = wait_for_router_readiness(
        guard.child.as_mut().ok_or("fixture child missing")?,
        &received,
        43210,
    );
    let stopped = guard.stop("readiness fixture");
    if readiness? != "listening: 127.0.0.1:43210" {
        return Err("readiness lost its exact real stdout line".into());
    }
    if !stopped?.cleanup_result.starts_with("terminated:signal: 9") {
        return Err("readiness fixture returned before actual forced termination".into());
    }
    Ok(())
}

#[test]
fn closed_stdout_is_a_causal_readiness_failure_while_the_child_is_held() -> FixtureResult {
    let mut guard = held_fixture("printf 'starting\\n'; exec 1>&-; read fixture_input")?;
    let (sent, received) = mpsc::channel();
    capture_fixture(&mut guard, Some(sent))?;
    let readiness = wait_for_router_readiness(
        guard.child.as_mut().ok_or("fixture child missing")?,
        &received,
        43210,
    );
    let stopped = guard.stop("closed-stdout fixture");
    stopped?;
    match readiness {
        Err(error) if error.contains("stdout closed before readiness") => Ok(()),
        _ => Err("stdout completion was not reported as the causal readiness failure".into()),
    }
}

#[test]
fn router_guard_waits_for_actual_successful_exit() -> FixtureResult {
    let mut guard =
        held_fixture("printf 'fixture stdout\\n'; printf 'fixture stderr\\n' >&2; exit 0")?;
    capture_fixture(&mut guard, None)?;
    let observed = guard.wait("completed fixture", Duration::from_secs(5))?;
    if !observed.cleanup_result.starts_with("exited:exit status: 0") {
        return Err("router guard did not retain its actual successful exit status".into());
    }
    Ok(())
}

#[test]
fn failed_guard_exit_retains_both_real_output_streams() -> FixtureResult {
    let mut guard =
        held_fixture("printf 'fixture stdout\\n'; printf 'fixture stderr\\n' >&2; exit 7")?;
    capture_fixture(&mut guard, None)?;
    match guard.wait("failed fixture", Duration::from_secs(5)) {
        Err(error)
            if error.contains("exited with status exit status: 7")
                && error.matches("fixture stdout").count() == 1
                && error.matches("fixture stderr").count() == 1 =>
        {
            Ok(())
        }
        _ => Err(
            "failed guard did not retain the actual status and both literal output streams".into(),
        ),
    }
}
