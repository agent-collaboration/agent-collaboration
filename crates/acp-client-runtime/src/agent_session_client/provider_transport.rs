//! Where one ACP connection's frames travel (design note §11.8, DS8): the
//! provider process's stdio in production, or, in this crate's scenario
//! tests, an in-process agent over a duplex pair. The connection task reads
//! and writes JSON lines on either and owns the agent's lifetime through it.

use super::{AcpAgent, ExternalProviderLaunch, ExternalProviderRuntimeError};
use std::pin::Pin;
use tokio::io::{AsyncRead, AsyncWrite};

/// One connection's frame streams and the agent behind them.
pub(super) struct ProviderTransport {
    pub(super) writer: Pin<Box<dyn AsyncWrite + Send>>,
    pub(super) reader: Pin<Box<dyn AsyncRead + Send>>,
    /// The process's stderr, drained into diagnostics; none in process.
    pub(super) stderr: Option<async_process::ChildStderr>,
    pub(super) lifetime: ProviderLifetime,
}

/// What runs the agent, and so what ends it.
pub(super) enum ProviderLifetime {
    Process(async_process::Child),
    /// A scenario test's in-process agent task.
    #[cfg(feature = "test-support")]
    InProcess(tokio::task::JoinHandle<()>),
}

impl ProviderTransport {
    /// Launches the provider process and takes its stdio.
    pub(super) fn spawn(
        launch: &ExternalProviderLaunch,
    ) -> Result<Self, ExternalProviderRuntimeError> {
        use tokio_util::compat::{FuturesAsyncReadCompatExt as _, FuturesAsyncWriteCompatExt as _};
        let (stdin, stdout, stderr, child) = AcpAgent::new(launch.sdk_config())
            .spawn_process()
            .map_err(|error| ExternalProviderRuntimeError::Launch(error.to_string()))?;
        Ok(Self {
            writer: Box::pin(stdin.compat_write()),
            reader: Box::pin(stdout.compat()),
            stderr: Some(stderr),
            lifetime: ProviderLifetime::Process(child),
        })
    }

    /// An in-process agent's end of a duplex pair, and the task serving it.
    #[cfg(feature = "test-support")]
    pub(super) fn in_process(
        connection: tokio::io::DuplexStream,
        agent: tokio::task::JoinHandle<()>,
    ) -> Self {
        let (reader, writer) = tokio::io::split(connection);
        Self {
            writer: Box::pin(writer),
            reader: Box::pin(reader),
            stderr: None,
            lifetime: ProviderLifetime::InProcess(agent),
        }
    }
}

impl ProviderLifetime {
    /// Resolves once the agent has exited on its own.
    pub(super) async fn exited(&mut self) {
        match self {
            Self::Process(child) => {
                let _status = child.status().await;
            }
            #[cfg(feature = "test-support")]
            Self::InProcess(agent) => {
                let _joined = agent.await;
            }
        }
    }

    /// Ends an agent that hasn't exited: the process group is killed and the
    /// child reaped; an in-process agent is aborted.
    pub(super) async fn end(self) {
        match self {
            Self::Process(mut child) => {
                #[cfg(unix)]
                if let Some(process_id) = rustix::process::Pid::from_raw(child.id().cast_signed()) {
                    let _result = rustix::process::kill_process_group(
                        process_id,
                        rustix::process::Signal::KILL,
                    );
                }
                let _result = child.kill();
                let _result = child.status().await;
            }
            #[cfg(feature = "test-support")]
            Self::InProcess(agent) => {
                agent.abort();
                let _joined = agent.await;
            }
        }
    }
}
