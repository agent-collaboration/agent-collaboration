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
        let waited = wait_for_child(&mut child, timeout);
        if !matches!(waited, Ok(Some(_))) {
            let _ = child.kill();
            let _ = child.wait();
        }
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
