//! One cancellable driver per central object, using the host's injected clock.
use super::{ObjectStorage, TokioWorkspaceStores};
use collaboration_central_core::WorkspaceRuntime;
use fleet_runtime::AlarmDeadline;
use fleet_runtime_ports::SyncTime;
use router_clock::RuntimeClock;
use std::sync::Arc;
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
    let mut publication = runtime
        .runtime()
        .statement_host()
        .subscribe_publication_status();
    let mut schedule = runtime.runtime().alarm_schedule();
    let wall_deadline = |at: SyncTime| {
        clock.wall_deadline(runtime_primitives::SyncTime::from_unix_millis(
            at.unix_millis(),
        ))
    };
    loop {
        let deadline = schedule.deadline(*publication.borrow_and_update(), wall_deadline);
        tokio::select! {
            () = stop.cancelled() => return,
            changed = publication.changed() => {
                if changed.is_err() { return; }
                schedule.publication_changed();
            },
            () = async {
                match deadline {
                    AlarmDeadline::Until(deadline) => clock.sleep_until(deadline).await,
                    AlarmDeadline::UntilPublication => std::future::pending().await,
                }
            } => {
                // A gated test clock's moves exclude this drive from before it
                // samples its time until its final publication.
                #[cfg(feature = "test-support")]
                let _drive = tokio::select! {
                    () = stop.cancelled() => return,
                    guard = async {
                        runtime
                            .runtime().pause_points()
                            .reached_engine(fleet_runtime::PausePoint::BeforeDriveGuard, "driver")
                            .await;
                        clock.drive_guard().await
                    } => guard,
                };
                let result = tokio::select! {
                    () = stop.cancelled() => return,
                    result = runtime.after_event() => result,
                };
                match &result {
                    Ok(_) => {
                        #[cfg(feature = "test-support")]
                        runtime.runtime().mark_alarm_completed();
                    }
                    Err(error) => {
                        tracing::warn!(?error, "central alarm drive failed; retrying");
                        #[cfg(feature = "test-support")]
                        runtime.runtime().mark_alarm_failed(format!("{error:?}"));
                    }
                }
                schedule.settle_drive(
                    &result,
                    || SyncTime::from_unix_millis(clock.utc_now().timestamp_millis()),
                    wall_deadline,
                );
            }
        }
    }
}
