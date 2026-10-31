use std::io::Read;
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use crate::driver_error::{DriverError, DriverResult};

static INTERRUPTED: OnceLock<Arc<AtomicBool>> = OnceLock::new();

pub fn install_termination_handler() -> DriverResult<()> {
    let interrupted = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&interrupted))?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&interrupted))?;
    INTERRUPTED
        .set(interrupted)
        .map_err(|_| DriverError::Runtime("termination handler already installed"))?;
    Ok(())
}
pub fn require_continuing() -> DriverResult<()> {
    if INTERRUPTED
        .get()
        .is_some_and(|flag| flag.swap(false, Ordering::Relaxed))
    {
        return Err(DriverError::Interrupted);
    }
    Ok(())
}

#[must_use]
pub fn exit_code(status: ExitStatus) -> i32 {
    status
        .code()
        .unwrap_or_else(|| -status.signal().unwrap_or(0))
}

pub fn wait_for_child(child: &mut Child, bound: Duration) -> DriverResult<Option<ExitStatus>> {
    let until = Instant::now() + bound;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= until {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Own command completion; unlike graded Host observation, a missed bound kills it.
pub fn wait_for_owned_command(
    child: &mut Child,
    bound: Duration,
) -> DriverResult<Option<ExitStatus>> {
    if let Some(status) = child.try_wait()? {
        return Ok(Some(status));
    }
    let identity = rustix::process::Pid::from_child(child);
    let (observed, received) = std::sync::mpsc::sync_channel(1);
    std::thread::scope(|scope| {
        let waiting = scope.spawn(move || {
            let outcome = loop {
                match rustix::process::waitid(
                    rustix::process::WaitId::Pid(identity),
                    rustix::process::WaitIdOptions::EXITED | rustix::process::WaitIdOptions::NOWAIT,
                ) {
                    Err(rustix::io::Errno::INTR) => continue,
                    outcome => break outcome.map(|status| status.is_some()),
                }
            };
            let _received = observed.send(outcome);
        });
        let outcome = received.recv_timeout(bound);
        let completed = matches!(outcome, Ok(Ok(true)));
        let killed = if completed { Ok(()) } else { child.kill() };
        // NOWAIT keeps this exact child unreaped until the observer has joined,
        // including a late observer after the parent requested termination.
        let joined = waiting.join();
        let status = child.wait();
        killed?;
        joined.map_err(|_| DriverError::Runtime("child exit observer failed"))?;
        let status = status?;
        match outcome {
            Ok(Ok(true)) => Ok(Some(status)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Ok(Err(error)) => Err(std::io::Error::from(error).into()),
            Ok(Ok(false)) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                Err(DriverError::Runtime("child exit observation unavailable"))
            }
        }
    })
}

pub fn output(arguments: &[String], timeout: Duration, check: bool) -> DriverResult<Output> {
    require_continuing()?;
    let (binary, arguments) = arguments
        .split_first()
        .ok_or(DriverError::Invalid("command missing"))?;
    let mut child = Command::new(binary)
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or(DriverError::Runtime("command stdout absent"))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or(DriverError::Runtime("command stderr absent"))?;
    let result = std::thread::scope(|scope| -> DriverResult<Output> {
        let output = scope.spawn(move || {
            let mut bytes = Vec::new();
            stdout.read_to_end(&mut bytes).map(|_| bytes)
        });
        let errors = scope.spawn(move || {
            let mut bytes = Vec::new();
            stderr.read_to_end(&mut bytes).map(|_| bytes)
        });
        let waited = wait_for_owned_command(&mut child, timeout);
        let stdout = output
            .join()
            .map_err(|_| DriverError::Runtime("stdout reader failed"))??;
        let stderr = errors
            .join()
            .map_err(|_| DriverError::Runtime("stderr reader failed"))??;
        let status = waited?.ok_or(DriverError::TimedOut("subprocess timeout"))?;
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    })?;
    require_continuing()?;
    if check && !result.status.success() {
        return Err(DriverError::Command(exit_code(result.status)));
    }
    Ok(result)
}

pub fn command(arguments: &[String]) -> DriverResult<Vec<u8>> {
    let result = output(arguments, Duration::from_secs(10), true)?;
    if result.stdout.len() + result.stderr.len() > 4 * 1024 * 1024 {
        return Err(DriverError::Invalid(
            "subprocess output exceeded evidence bound",
        ));
    }
    Ok(result.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_command_completion_preserves_its_exit_status() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exit 7"])
            .spawn()
            .expect("owned command");
        let status = wait_for_owned_command(&mut child, Duration::from_secs(5))
            .expect("observe command")
            .expect("command finished");
        assert_eq!(status.code(), Some(7));
        assert_eq!(child.wait().expect("cached reaped status"), status);
    }

    fn held_command() -> Child {
        Command::new("/bin/sh")
            .args(["-c", "read fixture_input"])
            .stdin(Stdio::piped())
            .spawn()
            .expect("owned held command")
    }

    #[test]
    fn an_expired_owned_command_bound_kills_and_reaps_before_returning() {
        let mut child = held_command();
        assert!(
            wait_for_owned_command(&mut child, Duration::ZERO)
                .expect("bounded command")
                .is_none()
        );
        let status = child
            .try_wait()
            .expect("reaped command")
            .expect("owned command exited");
        assert_eq!(status.signal(), Some(9));
    }

    #[test]
    fn an_expired_graded_wait_keeps_the_child_for_its_owning_escalation() {
        let mut child = held_command();
        let observed = wait_for_child(&mut child, Duration::ZERO);
        let still_alive = child.try_wait().expect("owned child status").is_none();
        child.kill().expect("owning test releases child");
        child.wait().expect("owning test reaps child");
        assert!(observed.expect("graded observation").is_none());
        assert!(
            still_alive,
            "graded wait killed the child before its owner's escalation"
        );
    }
}
