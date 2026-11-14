//! One cancellable driver per central object, using the host's injected clock.
use super::{ObjectStorage, TokioWorkspaceStores};
use collaboration_central_core::WorkspaceRuntime;
use router_clock::{ClockInstant, RuntimeClock};
use runtime_primitives::SyncTime;
use std::{sync::Arc, time::Duration};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

type Runtime = WorkspaceRuntime<ObjectStorage, TokioWorkspaceStores>;

pub(super) struct WorkspaceAlarmOwner {
    stop: CancellationToken,
    task: Option<JoinHandle<()>>,
}

impl WorkspaceAlarmOwner {
    pub(super) fn start(runtime: Arc<Runtime>, clock: Arc<dyn RuntimeClock>) -> Self {
        let stop = CancellationToken::new();
        let task = tokio::spawn(drive(runtime, clock, stop.clone()));
        Self {
            stop,
            task: Some(task),
        }
    }

    pub(super) async fn close(mut self) {
        self.stop.cancel();
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for WorkspaceAlarmOwner {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

async fn drive(runtime: Arc<Runtime>, clock: Arc<dyn RuntimeClock>, stop: CancellationToken) {
    let mut alarm = runtime.statement_host().subscribe_alarm();
    let mut retry: Option<(SyncTime, ClockInstant)> = None;
    let mut retry_seconds = 1_u64;
    loop {
        let due = *alarm.borrow_and_update();
        let deadline = due.map(|due| {
            let durable_deadline = clock.wall_deadline(due);
            match retry {
                Some((failed_due, retry_at)) if due >= failed_due => durable_deadline.max(retry_at),
                _ => durable_deadline,
            }
        });
        tokio::select! {
            () = stop.cancelled() => return,
            changed = alarm.changed() => if changed.is_err() { return; },
            () = async {
                match deadline {
                    Some(deadline) => clock.sleep_until(deadline).await,
                    None => std::future::pending().await,
                }
            } => {
                let result = tokio::select! {
                    () = stop.cancelled() => return,
                    result = runtime.on_alarm() => result,
                };
                match result {
                    Ok(_) => { retry = None; retry_seconds = 1; }
                    Err(error) => {
                        tracing::warn!(?error, "central alarm drive failed; retrying");
                        if let Some(failed_due) = due {
                            retry = Some((failed_due, clock.monotonic_now().after(Duration::from_secs(retry_seconds))));
                            retry_seconds = retry_seconds.saturating_mul(2).min(30);
                        }
                    }
                }
            }
        }
    }
}
