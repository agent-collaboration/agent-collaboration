//! Claim and receiving grants share the same unpublished native install owner.
use super::{
    project_writer::ProjectWriter,
    project_writers::{ProjectWriterSlot, ProjectWriters},
    snapshot_installation::{SnapshotInstallIdentity, SnapshotInstallation},
};
use collaboration_runtime::ProjectStoreRegistry;
use fleet_sync_protocol::{
    Epoch, LeaseAnswer, LeaseRequest, LeaseState, OperationAnswer, OperationRequest, RenewAnswer,
    RenewRequest, ReplicationPosition,
};
use fleet_sync_writer::BootstrapGrant;
use message_board::ProjectId;
use runtime_primitives::{Clock, IdSource, SystemClock, UuidV7Ids};
use std::{io, sync::Arc, time::Duration};

pub(super) enum SnapshotAdmission {
    Ready,
    Pending,
    NoLongerOwned,
    Stopped,
}

impl ProjectWriters {
    /// Return after the durable attempt is admitted and its native runner is owned.
    pub(super) async fn prepare_snapshot_install(
        &self,
        project: &ProjectId,
        grant: BootstrapGrant,
    ) -> io::Result<()> {
        if grant.lease.holder != self.peer
            || grant.lease.database.to_string() != project.as_str()
            || grant.lease.state != LeaseState::Held
        {
            return Err(io::Error::other(
                "snapshot grant names another holder or project",
            ));
        }
        let registry = self.registry()?;
        let slot = self.writer_slot(project).await;
        let mut owner = slot.lock().await;
        let epoch = grant.lease.epoch;
        let start = grant.start;
        if *self.stopped.borrow() {
            return Err(io::Error::other("machine session has stopped"));
        }
        match &*owner {
            ProjectWriterSlot::Ready(writer) => {
                let state = writer
                    .store
                    .writer_state()
                    .await
                    .map_err(io::Error::other)?;
                if matches!(state,
                    fleet_sync_lease::WriterState::Writing { epoch: actual }
                    | fleet_sync_lease::WriterState::Paused { epoch: actual, .. }
                    | fleet_sync_lease::WriterState::Seeding { epoch: actual }
                    | fleet_sync_lease::WriterState::Draining { epoch: actual, .. }
                    | fleet_sync_lease::WriterState::Released { epoch: actual, .. }
                    if actual == epoch)
                {
                    // A same-epoch rejection must not stop the live owner.
                    // Its ordinary renewal or release still owns this file.
                    return require_published_grant(&registry, project, writer, epoch, start).await;
                }
                if known_epoch_bound(&state).is_some_and(|actual| actual > epoch) {
                    return Ok(());
                }
                writer.stop().await?;
                let database = registry.database(project).await.map_err(io::Error::other)?;
                self.close_superseded_file(&registry, project, &database, state, epoch)
                    .await?;
            }
            ProjectWriterSlot::Installing(install) => {
                if install.epoch == epoch {
                    return if install.start == start {
                        Ok(())
                    } else {
                        Err(io::Error::other(
                            "same-epoch grant changed its snapshot start",
                        ))
                    };
                }
                if install.epoch > epoch {
                    return Ok(());
                }
                install.stop().await?;
                let state = install
                    .database
                    .writer_state()
                    .await
                    .map_err(io::Error::other)?;
                self.close_superseded_file(&registry, project, &install.database, state, epoch)
                    .await?;
            }
            ProjectWriterSlot::Terminal { .. } => {
                let current = registry.superseded_current(project).await.ok_or_else(|| {
                    io::Error::other("project runtime is terminal without superseded authority")
                })?;
                if current.is_none_or(|current| current.get() > epoch.value()) {
                    return Ok(());
                }
            }
            ProjectWriterSlot::Empty => {}
        }
        *owner = ProjectWriterSlot::Empty;
        let attempt =
            super::install_attempts::recorded_attempt(&self.directory, project, epoch).await?;
        let seed = registry
            .prepare_claim_seed(project, grant)
            .await
            .map_err(io::Error::other)?;
        let install = SnapshotInstallation::start(
            // The existing SDK control/staging home survives the holder-side rename.
            &self.directory.join("claim-installs").join(project.as_str()),
            seed,
            self.receiver,
            self.requests.clone(),
            SnapshotInstallIdentity {
                attempt,
                epoch,
                start,
            },
            self.stopped.clone(),
        )
        .await?;
        *owner = ProjectWriterSlot::Installing(Arc::new(install));
        Ok(())
    }

    pub(super) async fn finish_snapshot_install(
        &self,
        project: &ProjectId,
        epoch: Epoch,
        start: ReplicationPosition,
    ) -> io::Result<SnapshotAdmission> {
        let mut stopped = self.stopped.clone();
        // The machine stop also drains and closes the native install control
        // owner. Stop observing it first, including after Ready and before Renew.
        // Its runner stays owned by the slot and is joined separately at shutdown.
        tokio::select! {
            biased;
            requested = stopped.wait_for(|requested| *requested) => {
                requested.map_err(io::Error::other)?;
                Ok(SnapshotAdmission::Stopped)
            }
            result = self.finish_attached_snapshot_install(project, epoch, start) => result,
        }
    }

    async fn finish_attached_snapshot_install(
        &self,
        project: &ProjectId,
        epoch: Epoch,
        start: ReplicationPosition,
    ) -> io::Result<SnapshotAdmission> {
        let registry = self.registry()?;
        let slot = self.writer_slot(project).await;
        let install = {
            let owner = slot.lock().await;
            match &*owner {
                ProjectWriterSlot::Ready(writer) => {
                    if !matches!(writer.store.writer_state().await.map_err(io::Error::other)?, fleet_sync_lease::WriterState::Writing { epoch: actual } if actual == epoch)
                    {
                        return Ok(SnapshotAdmission::NoLongerOwned);
                    }
                    require_published_grant(&registry, project, writer, epoch, start).await?;
                    return Ok(SnapshotAdmission::Ready);
                }
                ProjectWriterSlot::Installing(install) => install.clone(),
                _ => return Ok(SnapshotAdmission::NoLongerOwned),
            }
        };
        if install.epoch != epoch || install.start != start {
            return Ok(SnapshotAdmission::NoLongerOwned);
        }
        // Preparation/duplicate notice handling needs the project slot while
        // native installation is pending. Only completion serializes this wait.
        let completed = tokio::time::timeout(Duration::from_secs(30), async {
            let _completing = install.completion.lock().await;
            let installed = install.finish_install().await;
            if let Err(error) = installed {
                let owner = slot.lock().await;
                return if matches!(&*owner, ProjectWriterSlot::Installing(current) if Arc::ptr_eq(current, &install)) {
                    Err(error)
                } else { Ok(SnapshotAdmission::NoLongerOwned) };
            }
            let mut owner = slot.lock().await;
            match &*owner {
                ProjectWriterSlot::Ready(writer) => {
                    if !matches!(writer.store.writer_state().await.map_err(io::Error::other)?, fleet_sync_lease::WriterState::Writing { epoch: actual } if actual == epoch) {
                        return Ok(SnapshotAdmission::NoLongerOwned);
                    }
                    require_published_grant(&registry, project, writer, epoch, start).await?;
                    return Ok(SnapshotAdmission::Ready);
                }
                ProjectWriterSlot::Installing(current) if Arc::ptr_eq(current, &install) => {}
                _ => return Ok(SnapshotAdmission::NoLongerOwned),
            }
            if *self.stopped.borrow() { return Ok(SnapshotAdmission::Stopped); }
            if !self.renew_stopped_writer(&install.database, epoch).await? { return Ok(SnapshotAdmission::NoLongerOwned); }
            install.database.migrate().await.map_err(io::Error::other)?;
            install.record_board_baseline().await?;
            registry.publish_claim_seed(project, install.database.clone(), start).await.map_err(io::Error::other)?;
            let writer = Arc::new(ProjectWriter::open(
                &registry, project, self.receiver, self.requests.clone(),
                self.stopped.clone(), self.lifecycle_signals.clone(),
            ).await?);
            writer.activate(project).await?;
            *owner = ProjectWriterSlot::Ready(writer);
            Ok(SnapshotAdmission::Ready)
        }).await;
        completed.unwrap_or(Ok(SnapshotAdmission::Pending))
    }

    async fn close_superseded_file(
        &self,
        registry: &ProjectStoreRegistry,
        project: &ProjectId,
        database: &fleet_sync_writer::ReplicatedDatabase<board_fleet_sync::BoardProjectSchema>,
        state: fleet_sync_lease::WriterState,
        new_epoch: Epoch,
    ) -> io::Result<()> {
        // The previous runner is drained. Persist an actual newer-epoch refusal
        // before closing it; no synthetic supersession can confer authority.
        if !matches!(state, fleet_sync_lease::WriterState::Superseded { .. }) {
            let previous = known_epoch_bound(&state)
                .filter(|previous| *previous < new_epoch)
                .ok_or_else(|| {
                    io::Error::other("snapshot replacement lacks an older native epoch")
                })?;
            if self.renew_stopped_writer(database, previous).await? {
                return Err(io::Error::other(
                    "previous epoch unexpectedly retained authority",
                ));
            }
        }
        registry
            .supersede_project(project)
            .await
            .map_err(io::Error::other)
    }

    async fn renew_stopped_writer(
        &self,
        database: &fleet_sync_writer::ReplicatedDatabase<board_fleet_sync::BoardProjectSchema>,
        epoch: Epoch,
    ) -> io::Result<bool> {
        // No installer task remains and the live writer has not started.
        if *self.stopped.borrow() {
            return Err(io::Error::other(
                "machine session stopped during installation",
            ));
        }
        let request = RenewRequest {
            attempt: fleet_sync_protocol::RequestId::new(UuidV7Ids.new_id())
                .map_err(io::Error::other)?,
            database: database.database(),
            epoch,
        };
        let request_started = SystemClock.now();
        let answer = self
            .requests
            .request(OperationRequest::Lease(LeaseRequest::Renew(request)))
            .await
            .map_err(|failure| {
                io::Error::other(format!("snapshot renewal request: {failure:?}"))
            })?;
        let OperationAnswer::Lease(LeaseAnswer::Renew(answer)) = answer else {
            return Err(io::Error::other(
                "snapshot renewal returned another operation answer",
            ));
        };
        let authorized = matches!(&answer, RenewAnswer::Renewed { current, .. }
            if current.database == database.database()
                && current.state == LeaseState::Held
                && current.holder == self.peer
                && current.epoch == epoch);
        // Persist refusals too; an old grant cannot revive a superseded file.
        database
            .observe_renewal(answer, request_started)
            .await
            .map_err(io::Error::other)?;
        if !authorized {
            return Ok(false);
        }
        match database.writer_state().await.map_err(io::Error::other)? {
            fleet_sync_lease::WriterState::Writing { epoch: actual } if actual == epoch => Ok(true),
            state => Err(io::Error::other(format!(
                "snapshot renewal is not writable: {state:?}"
            ))),
        }
    }
}

async fn require_published_grant(
    registry: &ProjectStoreRegistry,
    project: &ProjectId,
    writer: &ProjectWriter,
    epoch: Epoch,
    start: ReplicationPosition,
) -> io::Result<()> {
    let receipt = registry
        .database(project)
        .await
        .map_err(io::Error::other)?
        .installed_snapshot()
        .await
        .map_err(io::Error::other)?;
    let state = writer
        .store
        .writer_state()
        .await
        .map_err(io::Error::other)?;
    if receipt.as_ref().is_some_and(|receipt| {
        receipt.verified && receipt.ready && receipt.install.position == start
    }) && matches!(state, fleet_sync_lease::WriterState::Writing { epoch: actual } if actual == epoch)
    {
        Ok(())
    } else {
        Err(io::Error::other(
            "grant conflicts with an existing project writer",
        ))
    }
}

fn known_epoch_bound(state: &fleet_sync_lease::WriterState) -> Option<Epoch> {
    use fleet_sync_lease::WriterState;
    match state {
        WriterState::Provisional => None,
        WriterState::Seeding { epoch }
        | WriterState::Writing { epoch }
        | WriterState::Paused { epoch, .. }
        | WriterState::Draining { epoch, .. }
        | WriterState::Released { epoch, .. } => Some(*epoch),
        WriterState::Superseded { current } => Some(*current),
    }
}
