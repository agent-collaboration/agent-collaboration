//! Register both Unix shutdown signals before the launcher publishes readiness.
use super::launcher_failure::{LauncherFailure, LauncherResult};
use tokio::signal::unix::{Signal, SignalKind, signal};

pub(super) struct ShutdownSignals {
    terminate: Signal,
    interrupt: Signal,
}
impl ShutdownSignals {
    pub(super) fn register() -> LauncherResult<Self> {
        Ok(Self {
            terminate: signal(SignalKind::terminate())
                .map_err(|_| LauncherFailure::RegisterSignals)?,
            interrupt: signal(SignalKind::interrupt())
                .map_err(|_| LauncherFailure::RegisterSignals)?,
        })
    }
    pub(super) async fn wait(&mut self) {
        tokio::select! {_ = self.terminate.recv()=>{},_ = self.interrupt.recv()=>{}}
    }
}
