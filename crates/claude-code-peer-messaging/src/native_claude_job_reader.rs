//! One bounded, read-only native list command; Claude owns every observed job.
use collaboration_protocol::{
    ClaudeCodeInteractiveStatus, NativeClaudeBackgroundState, NativeClaudeJobHandle,
    NativeInventoryUnavailableReason, ProviderWorkingDirectory, SessionId,
};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt};

#[path = "native_claude_job_decoder.rs"]
mod native_claude_job_decoder;
use native_claude_job_decoder::decode_background_jobs;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeClaudeBackgroundJob {
    pub job_handle: NativeClaudeJobHandle,
    pub session_id: SessionId,
    pub working_directory: ProviderWorkingDirectory,
    pub name: Option<String>,
    pub started_at: i64,
    pub state: NativeClaudeBackgroundState,
    pub status: Option<ClaudeCodeInteractiveStatus>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeClaudeJobInventory {
    pub jobs: Vec<NativeClaudeBackgroundJob>,
    pub unidentified_jobs: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum NativeClaudeJobReadError {
    #[error("native background inventory unavailable")]
    Unavailable(NativeInventoryUnavailableReason),
    #[error("invalid native background inventory")]
    InvalidOutput,
}

#[derive(Clone, Debug)]
pub struct NativeClaudeJobReader {
    executable: PathBuf,
    timeout: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
}
impl Default for NativeClaudeJobReader {
    fn default() -> Self {
        Self::for_executable(PathBuf::from("claude"))
    }
}
impl NativeClaudeJobReader {
    /// Trusted Host composition selects this executable; no API request can supply it.
    #[must_use]
    pub fn for_executable(executable: PathBuf) -> Self {
        Self {
            executable,
            timeout: Duration::from_secs(5),
            stdout_limit: collaboration_protocol::MAX_CONTROL_FRAME_BYTES,
            stderr_limit: 8192,
        }
    }
    pub async fn read_background_jobs(
        &self,
    ) -> Result<NativeClaudeJobInventory, NativeClaudeJobReadError> {
        use NativeClaudeJobReadError::Unavailable;
        let mut child = tokio::process::Command::new(&self.executable)
            .args(["agents", "--json", "--all"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                Unavailable(if error.kind() == std::io::ErrorKind::NotFound {
                    NativeInventoryUnavailableReason::ExecutableUnavailable
                } else {
                    NativeInventoryUnavailableReason::ReadUnavailable
                })
            })?;
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            let _settled = child.kill().await;
            return Err(Unavailable(
                NativeInventoryUnavailableReason::ReadUnavailable,
            ));
        };
        // Cancellation kills this child on drop; Tokio's Unix reaper is best-effort.
        // On ordinary timeout/limit/error, explicitly await only this child's settlement.
        let observed = tokio::time::timeout(self.timeout, async {
            tokio::try_join!(
                read_bounded(stdout, self.stdout_limit),
                read_bounded(stderr, self.stderr_limit),
                async { child.wait().await.map_err(|_| NativePipeFailure::Io) }
            )
        })
        .await;
        match observed {
            Err(_) => {
                let _settled = child.kill().await;
                Err(Unavailable(NativeInventoryUnavailableReason::ReadTimedOut))
            }
            Ok(Err(NativePipeFailure::OutputLimit)) => {
                let _settled = child.kill().await;
                Err(NativeClaudeJobReadError::InvalidOutput)
            }
            Ok(Err(NativePipeFailure::Io)) => {
                let _settled = child.kill().await;
                Err(Unavailable(
                    NativeInventoryUnavailableReason::ReadUnavailable,
                ))
            }
            Ok(Ok((stdout, _stderr, status))) if status.success() => {
                decode_background_jobs(&stdout)
            }
            Ok(Ok(_)) => Err(Unavailable(
                NativeInventoryUnavailableReason::ReadUnavailable,
            )),
        }
    }
}

enum NativePipeFailure {
    Io,
    OutputLimit,
}

async fn read_bounded(
    reader: impl AsyncRead + Unpin,
    limit: usize,
) -> Result<Vec<u8>, NativePipeFailure> {
    let mut bytes = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| NativePipeFailure::Io)?;
    if bytes.len() > limit {
        return Err(NativePipeFailure::OutputLimit);
    }
    Ok(bytes)
}

#[cfg(test)]
#[path = "native_claude_job_reader_tests.rs"]
mod tests;
