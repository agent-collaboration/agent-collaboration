//! Run one child process with a deadline and byte bounds enforced while reading:
//! output past its bound fails the run instead of being buffered. The child is killed
//! on drop, so a timeout or overflow stops only the process this run started.
use std::{process::ExitStatus, process::Stdio, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt};

pub(crate) struct BoundedOutput {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

async fn read_bounded(
    stream: Option<impl AsyncRead + Unpin>,
    limit: usize,
    name: &str,
) -> Result<Vec<u8>, String> {
    let Some(stream) = stream else {
        return Ok(Vec::new());
    };
    let mut bytes = Vec::new();
    stream
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| format!("{name} unreadable: {error}"))?;
    if bytes.len() > limit {
        return Err(format!("{name} exceeds its {limit}-byte bound"));
    }
    Ok(bytes)
}

pub(crate) async fn run_bounded(
    mut command: tokio::process::Command,
    label: &str,
    timeout: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
) -> Result<BoundedOutput, String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("{label} could not run: {error}"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (stdout_name, stderr_name) = (format!("{label} output"), format!("{label} diagnostics"));
    let collected = async {
        let (stdout, stderr) = tokio::try_join!(
            read_bounded(stdout, stdout_limit, &stdout_name),
            read_bounded(stderr, stderr_limit, &stderr_name),
        )?;
        let status = child
            .wait()
            .await
            .map_err(|error| format!("{label} wait failed: {error}"))?;
        Ok::<_, String>(BoundedOutput {
            status,
            stdout,
            stderr,
        })
    };
    tokio::time::timeout(timeout, collected)
        .await
        .map_err(|_| format!("{label} did not finish within its bound"))?
}
