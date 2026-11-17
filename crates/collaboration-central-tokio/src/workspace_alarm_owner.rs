//! One cancellable driver per central object, using the host's injected clock.
use super::{ObjectStorage, TokioWorkspaceStores};
use crate::object_storage::LastAlarmPublication;
use collaboration_central_core::WorkspaceRuntime;
use router_clock::{ClockInstant, RuntimeClock};
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

type Runtime = WorkspaceRuntime<ObjectStorage, TokioWorkspaceStores>;

#[derive(Clone, Copy)]
enum AlarmWait {
    Until(ClockInstant),
    UntilPublication,
}

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
    let mut publication = runtime.statement_host().subscribe_publication_status();
    let mut retry: Option<AlarmWait> = None;
    loop {
        let status = *publication.borrow_and_update();
        let deadline = retry.unwrap_or_else(|| match status {
            LastAlarmPublication::Published { deadline } => deadline
                .map_or(AlarmWait::UntilPublication, |at| {
                    AlarmWait::Until(clock.wall_deadline(at))
                }),
            LastAlarmPublication::Failed { at } => {
                next_mark_deadline(clock.as_ref(), at.unix_millis())
            }
        });
        tokio::select! {
            () = stop.cancelled() => return,
            changed = publication.changed() => {
                if changed.is_err() { return; }
                retry = None;
            },
            () = async {
                match deadline {
                    AlarmWait::Until(deadline) => clock.sleep_until(deadline).await,
                    AlarmWait::UntilPublication => std::future::pending().await,
                }
            } => {
                let result = tokio::select! {
                    () = stop.cancelled() => return,
                    result = runtime.after_event() => result,
                };
                match result {
                    Ok(_) => {
                        #[cfg(feature = "test-support")]
                        runtime.mark_alarm_completed();
                        retry = None;
                    }
                    Err(error) => {
                        tracing::warn!(?error, "central alarm drive failed; retrying");
                        #[cfg(feature = "test-support")]
                        runtime.mark_alarm_failed(format!("{error:?}"));
                        retry = Some(next_mark_deadline(clock.as_ref(), clock.utc_now().timestamp_millis()));
                    }
                }
            }
        }
    }
}

fn next_mark_deadline(clock: &dyn RuntimeClock, now: i64) -> AlarmWait {
    central_storage::alarm_publication_time(Some(now), now)
        .ok()
        .flatten()
        .map_or(AlarmWait::UntilPublication, |millis| {
            AlarmWait::Until(
                clock.wall_deadline(runtime_primitives::SyncTime::from_unix_millis(millis)),
            )
        })
}
