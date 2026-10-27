//! Register the real shutdown handlers before the example publishes readiness.
use std::io;
use tokio::signal::unix::{Signal, SignalKind, signal};

pub(super) struct ShutdownSignals {
    terminate_signal: Signal,
    interrupt_signal: Signal,
}

impl ShutdownSignals {
    pub(super) fn register_handlers() -> io::Result<Self> {
        Ok(Self {
            terminate_signal: signal(SignalKind::terminate())?,
            interrupt_signal: signal(SignalKind::interrupt())?,
        })
    }

    pub(super) async fn wait_for_signal(&mut self) -> io::Result<()> {
        tokio::select! {
            _ = self.terminate_signal.recv() => Ok(()),
            _ = self.interrupt_signal.recv() => Ok(()),
        }
    }
}
