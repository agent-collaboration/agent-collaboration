//! Prevent real SQLite IO from auto-advancing paused Tokio failure guards.
use std::time::Duration;

pub(super) struct ExplicitRetryTime {
    release: Option<std::sync::mpsc::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl ExplicitRetryTime {
    pub(super) fn new(bound: Duration) -> Self {
        let (release, released) = std::sync::mpsc::channel();
        let task = tokio::task::spawn_blocking(move || {
            released
                .recv_timeout(bound)
                .expect("SQLite retry proof exceeded its causal IO bound");
        });
        Self {
            release: Some(release),
            task: Some(task),
        }
    }

    pub(super) async fn finish(mut self) {
        self.release_owner();
        self.task
            .take()
            .expect("retry clock guard owns its task")
            .await
            .expect("retry clock guard completes");
    }

    fn release_owner(&mut self) {
        if let Some(release) = self.release.take() {
            let _released = release.send(());
        }
    }
}

impl Drop for ExplicitRetryTime {
    fn drop(&mut self) {
        self.release_owner();
    }
}
