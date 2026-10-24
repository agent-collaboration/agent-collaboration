//! One owned native install runner remains retryable while the project is unpublished.
use board_fleet_sync::BoardProjectSchema;
use board_sync_protocol::BoardConsumerProtocol;
use fleet_sync_protocol::{Epoch, InstallAttemptId, ReceiverId, ReplicationPosition};
use fleet_sync_tokio_runner::{InstallStatus, NativeWriterRuntime, SessionRequests};
use fleet_sync_writer::ReplicatedDatabase;
use std::{io, path::Path, sync::Arc};
use tokio::{
    sync::{Mutex, watch},
    task::JoinSet,
};

struct InstallTasks {
    running: JoinSet<io::Result<()>>,
    failure: Option<String>,
}

pub(super) struct SnapshotInstallIdentity {
    pub attempt: InstallAttemptId,
    pub epoch: Epoch,
    pub start: ReplicationPosition,
}

pub(super) struct SnapshotInstallation {
    pub database: Arc<ReplicatedDatabase<BoardProjectSchema>>,
    // Correlate duplicate hints without reading target pools during rename.
    // These identities carry no current lease authority.
    pub epoch: Epoch,
    pub start: ReplicationPosition,
    pub(super) completion: Mutex<()>,
    runtime: Arc<NativeWriterRuntime<BoardProjectSchema, BoardConsumerProtocol>>,
    attempt: InstallAttemptId,
    stop: watch::Sender<bool>,
    tasks: Mutex<InstallTasks>,
}
impl SnapshotInstallation {
    #[cfg(test)]
    pub(super) fn stop_requested(&self) -> watch::Receiver<bool> {
        self.stop.subscribe()
    }

    pub async fn start(
        directory: &Path,
        database: Arc<ReplicatedDatabase<BoardProjectSchema>>,
        receiver: ReceiverId,
        requests: SessionRequests<BoardConsumerProtocol>,
        identity: SnapshotInstallIdentity,
        mut machine_stop: watch::Receiver<bool>,
    ) -> io::Result<Self> {
        let mut runtime = NativeWriterRuntime::new(database.clone(), receiver, requests)
            .await
            .map_err(io::Error::other)?;
        runtime
            .enable_snapshot_installs(directory)
            .await
            .map_err(io::Error::other)?;
        runtime
            .install_snapshot(identity.attempt)
            .await
            .map_err(io::Error::other)?;
        let runtime = Arc::new(runtime);
        let (stop, stopped) = watch::channel(false);
        let mut tasks = JoinSet::new();
        let running = runtime.clone();
        tasks.spawn(async move { running.run(stopped).await.map_err(io::Error::other) });
        let ending = stop.clone();
        let mut local_stop = stop.subscribe();
        tasks.spawn(async move {
            loop {
                if *machine_stop.borrow() { ending.send_replace(true);return Ok(()); }
                tokio::select! {
                    changed=machine_stop.changed() => if changed.is_err() { ending.send_replace(true);return Ok(()); },
                    _=local_stop.changed() => return Ok(()),
                }
            }
        });
        Ok(Self {
            database,
            epoch: identity.epoch,
            start: identity.start,
            completion: Mutex::new(()),
            runtime,
            attempt: identity.attempt,
            stop,
            tasks: Mutex::new(InstallTasks {
                running: tasks,
                failure: None,
            }),
        })
    }

    pub async fn finish_install(&self) -> io::Result<()> {
        // A cancelled request may already have drained the installer. Its
        // control pool is then closed, while this verified receipt stays durable.
        if !self
            .database
            .installed_snapshot()
            .await
            .map_err(io::Error::other)?
            .is_some_and(|receipt| receipt.ready && receipt.verified)
        {
            let installed = self
                .runtime
                .wait_install(self.attempt)
                .await
                .map_err(io::Error::other)?;
            if !matches!(installed.status, InstallStatus::Ready { .. }) {
                return Err(io::Error::other(format!(
                    "claim snapshot install did not become ready: {:?}",
                    installed.status
                )));
            }
        }
        // The pinned library does not order simultaneous same-epoch renewal
        // observations. Drain its installer/writer before the bootstrap renew.
        self.stop().await
    }

    pub async fn record_board_baseline(&self) -> io::Result<()> {
        let receipt = self
            .database
            .installed_snapshot()
            .await
            .map_err(io::Error::other)?
            .ok_or_else(|| io::Error::other("claim snapshot receipt is absent"))?;
        self.runtime
            .store()
            .rule_transaction(move |scope| {
                Box::pin(async move {
                    message_board_storage::record_snapshot_capture(scope, &receipt)
                        .await
                        .map_err(durable_steps::StoreError::backend)
                })
            })
            .await
            .map_err(io::Error::other)
    }

    pub async fn stop(&self) -> io::Result<()> {
        self.stop.send_replace(true);
        let mut tasks = self.tasks.lock().await;
        while let Some(result) = tasks.running.join_next().await {
            if let Err(error) = result.map_err(io::Error::other).and_then(|result| result) {
                // Keep observed failures with the owned tasks if this caller is
                // cancelled while another task is still draining.
                tasks.failure.get_or_insert(error.to_string());
            }
        }
        tasks
            .failure
            .clone()
            .map_or(Ok(()), |failure| Err(io::Error::other(failure)))
    }
}

#[cfg(test)]
#[path = "tests/claim_renewal_order.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/claim_notice_overlap.rs"]
mod notice_tests;
