//! Capture both real pipes while an owned child waits at its kernel exit boundary.
use super::{
    CodexChildRun, Command, Duration, ExitStatus, Instant, Output, Read, mpsc,
    stderr_transport_error_markers, thread,
};

#[cfg(test)]
pub(super) fn run_with_timeout(command: Command, timeout: Duration) -> Result<Output, String> {
    run_with_timeout_observed(command, timeout).map(|run| run.output)
}

pub(super) fn run_with_timeout_observed(
    command: Command,
    timeout: Duration,
) -> Result<CodexChildRun, String> {
    run_with_deadline(command, timeout, |_| Ok(Instant::now() + timeout))
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ChildOutputPipe {
    StandardOutput,
    StandardError,
}
impl ChildOutputPipe {
    fn name(self) -> &'static str {
        match self {
            Self::StandardOutput => "stdout",
            Self::StandardError => "stderr",
        }
    }
}

#[cfg(test)]
pub(super) fn run_with_expiry_after_output_closed(command: Command) -> Result<Output, String> {
    run_with_deadline(command, Duration::ZERO, |closed| {
        let first = closed
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| format!("first child output pipe did not close: {error}"))?;
        let second = closed
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| format!("second child output pipe did not close: {error}"))?;
        if first == second {
            return Err("one child output pipe closed twice".to_owned());
        }
        Ok(Instant::now())
    })
    .map(|run| run.output)
}

fn run_with_deadline(
    mut command: Command,
    timeout: Duration,
    deadline: impl FnOnce(mpsc::Receiver<ChildOutputPipe>) -> Result<Instant, String>,
) -> Result<CodexChildRun, String> {
    use crate::owned_cli_process::{ChildExitObservation, wait_for_sync_child};
    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to spawn installed codex: {error}"))?;
    let pid = child.id();
    let (closed, completion) = mpsc::channel();
    let stdout_reader = match child
        .stdout
        .take()
        .map(|stdout| {
            spawn_child_output_reader(ChildOutputPipe::StandardOutput, stdout, closed.clone())
        })
        .transpose()
    {
        Ok(reader) => reader,
        Err(error) => {
            let _terminated = child.kill();
            let _reaped = child.wait();
            return Err(error);
        }
    };
    let stderr_reader = match child
        .stderr
        .take()
        .map(|stderr| {
            spawn_child_output_reader(ChildOutputPipe::StandardError, stderr, closed.clone())
        })
        .transpose()
    {
        Ok(reader) => reader,
        Err(error) => {
            let _terminated = child.kill();
            let _reaped = child.wait();
            let _joined = join_child_output_reader(stdout_reader, "stdout");
            return Err(error);
        }
    };
    drop(closed);
    let observed = deadline(completion).and_then(|deadline| {
        wait_for_sync_child(&mut child, deadline)
            .map_err(|error| format!("failed to wait for installed codex: {error}"))
    });
    let observation = match observed {
        Ok(observation) => observation,
        Err(error) => {
            let _terminated = child.kill();
            let waited = child.wait();
            let _stdout = join_child_output_reader(stdout_reader, "stdout");
            let _stderr = join_child_output_reader(stderr_reader, "stderr");
            return Err(format!("{error}; child wait={waited:?}"));
        }
    };
    match observation {
        ChildExitObservation::Exited(status) => {
            let output = collect_child_output(status, stdout_reader, stderr_reader)?;
            Ok(CodexChildRun { pid, output })
        }
        ChildExitObservation::DeadlineExpired(status) => {
            let output = collect_child_output(status, stdout_reader, stderr_reader)?;
            let stdout_byte_count = output.stdout.len();
            let stderr_byte_count = output.stderr.len();
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!(
                "installed codex timed out after {}s; captured stdout/stderr suppressed to avoid leaking secrets (stdout_bytes={stdout_byte_count}, stderr_bytes={stderr_byte_count}); stdout_preview={}; stderr_preview={}; stdout_markers={}; stderr_markers={}",
                timeout.as_secs(),
                redacted_process_output_preview(&stdout),
                redacted_process_output_preview(&stderr),
                process_output_markers(&stdout),
                process_output_markers(&stderr),
            ))
        }
    }
}

fn spawn_child_output_reader<R>(
    output_pipe: ChildOutputPipe,
    mut stream: R,
    closed: mpsc::Sender<ChildOutputPipe>,
) -> Result<thread::JoinHandle<Result<Vec<u8>, String>>, String>
where
    R: Read + Send + 'static,
{
    let stream_name = output_pipe.name();
    thread::Builder::new()
        .name(format!("codex-router-installed-codex-{stream_name}-reader"))
        .spawn(move || {
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes).map_err(|error| {
                format!("failed to read installed codex {stream_name}: {error}")
            })?;
            let _received = closed.send(output_pipe);
            Ok(bytes)
        })
        .map_err(|error| format!("failed to spawn installed codex {stream_name} reader: {error}"))
}

pub(super) fn collect_child_output(
    status: ExitStatus,
    stdout_reader: Option<thread::JoinHandle<Result<Vec<u8>, String>>>,
    stderr_reader: Option<thread::JoinHandle<Result<Vec<u8>, String>>>,
) -> Result<Output, String> {
    let stdout = join_child_output_reader(stdout_reader, "stdout")?;
    let stderr = join_child_output_reader(stderr_reader, "stderr")?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

pub(super) fn join_child_output_reader(
    reader: Option<thread::JoinHandle<Result<Vec<u8>, String>>>,
    stream_name: &str,
) -> Result<Vec<u8>, String> {
    match reader {
        Some(reader) => reader
            .join()
            .map_err(|_| format!("installed codex {stream_name} reader panicked"))?,
        None => Ok(Vec::new()),
    }
}

pub(super) fn redacted_process_output_preview(output: &str) -> String {
    if output.trim().is_empty() {
        "<empty>".to_owned()
    } else {
        "<suppressed>".to_owned()
    }
}

pub(super) fn process_output_markers(output: &str) -> String {
    let markers = stderr_transport_error_markers(output);
    if markers.is_empty() {
        "<none>".to_owned()
    } else {
        markers.join(",")
    }
}
