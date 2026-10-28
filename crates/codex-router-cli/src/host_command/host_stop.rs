//! Stop requests reuse the Host's signal path; only process exit and lock release prove success.

use std::fs::{OpenOptions, TryLockError};
use std::io;
use std::path::Path;
use std::time::Duration;

use codex_router_host::{HostCoordinationPaths, OperatorRequest, encode_operator_request};
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;

use super::process_exit_observer::ProcessExitObserver;

pub(super) enum HostStopOutcome {
    Stopped,
    NotRunning,
}

impl HostStopOutcome {
    pub(super) const fn message(&self) -> &'static str {
        match self {
            Self::Stopped => "Host stopped.",
            Self::NotRunning => "Host is not running.",
        }
    }
}

#[derive(Debug, Error)]
pub enum HostStopError {
    #[error("Host did not exit within its shutdown deadline; no forced termination was sent")]
    Deadline,
    #[error("Host lock is still held; shutdown could not be confirmed")]
    LockHeld,
    #[error("Host operator peer has no verifiable same-owner process identity")]
    PeerIdentity,
    #[error("Host stop failed: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Protocol(#[from] codex_router_host::OperatorProtocolError),
}

pub(super) async fn stop_host(
    paths: &HostCoordinationPaths,
    deadline: Duration,
) -> Result<HostStopOutcome, HostStopError> {
    let started = std::time::Instant::now();
    let exit = tokio::time::timeout(deadline, async {
        let mut stream = match UnixStream::connect(paths.operator_socket()).await {
            Ok(stream) => stream,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                ) =>
            {
                require_released_lock(paths.instance_lock())?;
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
        };
        let peer = stream.peer_cred()?;
        if peer.uid() != nix::unistd::getuid().as_raw() {
            return Err(HostStopError::PeerIdentity);
        }
        let process_id = peer
            .pid()
            .filter(|pid| *pid > 0)
            .ok_or(HostStopError::PeerIdentity)?;
        // Register before sending: a fast orderly shutdown must not outrun observation.
        let exit = ProcessExitObserver::observe(process_id)?;
        stream
            .write_all(&encode_operator_request(&OperatorRequest::StopHost)?)
            .await?;
        stream.shutdown().await?;
        Ok(Some(exit))
    })
    .await
    .map_err(|_| HostStopError::Deadline)??;
    let Some(exit) = exit else {
        return Ok(HostStopOutcome::NotRunning);
    };
    exit.wait(deadline.saturating_sub(started.elapsed()))
        .await
        .map_err(|error| {
            if error.kind() == io::ErrorKind::TimedOut {
                HostStopError::Deadline
            } else {
                HostStopError::Io(error)
            }
        })?;
    require_released_lock(paths.instance_lock())?;
    Ok(HostStopOutcome::Stopped)
}

fn require_released_lock(path: &Path) -> Result<(), HostStopError> {
    let file = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    match file.try_lock() {
        Ok(()) => Ok(()),
        Err(TryLockError::WouldBlock) => Err(HostStopError::LockHeld),
        Err(TryLockError::Error(error)) => Err(error.into()),
    }
}
