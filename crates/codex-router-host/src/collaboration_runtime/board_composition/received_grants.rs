//! Session hints start only a grant confirmed by central's Copy and Registry.
use super::{project_installation::SnapshotAdmission, project_writers::ProjectWriters};
use fleet_sync_protocol::{
    CopyRole, CopyStatusAnswer, DatabaseId, Epoch, LeaseAnswer, LeaseRequest, LeaseState,
    OperationAnswer, OperationRequest, RenewAnswer, RenewRequest, ReplicationAnswer,
    ReplicationRequest, RequestId,
};
use fleet_sync_writer::BootstrapGrant;
use message_board::ProjectId;
use runtime_primitives::{Clock, IdSource, SystemClock, UuidV7Ids};
use std::{io, sync::Arc};

impl ProjectWriters {
    /// Success means stale, already present, or durably handed to the project owner.
    pub(super) async fn receive_grant(
        self: &Arc<Self>,
        project: &ProjectId,
        hinted_epoch: Epoch,
    ) -> io::Result<()> {
        if *self.stopped.borrow() || !*self.welcomed.borrow() {
            return Err(io::Error::other(
                "grant arrived without an attached machine session",
            ));
        }
        let Some(grant) = self.confirmed_snapshot_grant(project, hinted_epoch).await? else {
            return Ok(());
        };
        let registry = self.registry()?;
        if registry.superseded_current(project).await.is_none() {
            match registry.existing_project(project).await {
                Ok(Some(store)) => {
                    let state = store.writer_state().await.map_err(io::Error::other)?;
                    if matches!(state,
                    fleet_sync_lease::WriterState::Writing { epoch }
                    | fleet_sync_lease::WriterState::Paused { epoch, .. }
                    | fleet_sync_lease::WriterState::Seeding { epoch } if epoch == hinted_epoch)
                    {
                        // The current-epoch local file already has its own native
                        // writer. Prompt that owner; do not seed over it.
                        self.prompt_project_renewal(project).await?;
                        return Ok(());
                    }
                }
                Ok(None)
                | Err(
                    collaboration_runtime::ProjectRegistryError::Unavailable { .. }
                    | collaboration_runtime::ProjectRegistryError::Installing { .. },
                ) => {}
                Err(error) => return Err(io::Error::other(error)),
            }
        }
        let start = grant.start;
        self.prepare_snapshot_install(project, grant).await?;
        let finishing = Arc::downgrade(self);
        let project = project.clone();
        // This owner outlives the notice handler and is drained at shutdown.
        // No Ready wait belongs to receipt acknowledgement.
        self.operations.lock().await.spawn(async move {
            let projects = finishing
                .upgrade()
                .ok_or_else(|| io::Error::other("project owner closed"))?;
            loop {
                match projects
                    .finish_snapshot_install(&project, hinted_epoch, start)
                    .await?
                {
                    SnapshotAdmission::Ready
                    | SnapshotAdmission::NoLongerOwned
                    | SnapshotAdmission::Stopped => return Ok(()),
                    SnapshotAdmission::Pending => {}
                }
            }
        });
        Ok(())
    }

    /// Hints carry no authority: use CopyStatus.start and a fresh typed Renew.
    pub(super) async fn confirmed_snapshot_grant(
        &self,
        project: &ProjectId,
        hinted_epoch: Epoch,
    ) -> io::Result<Option<BootstrapGrant>> {
        let database =
            DatabaseId::try_from(project.as_str().to_owned()).map_err(io::Error::other)?;
        let answer = self
            .requests
            .request(OperationRequest::Replication(
                ReplicationRequest::CopyStatus { database },
            ))
            .await
            .map_err(|failure| io::Error::other(format!("grant CopyStatus: {failure:?}")))?;
        let status = match answer {
            OperationAnswer::Replication(ReplicationAnswer::CopyStatus(
                CopyStatusAnswer::Status(status),
            )) => status,
            OperationAnswer::Replication(ReplicationAnswer::CopyStatus(
                CopyStatusAnswer::NotInitialized | CopyStatusAnswer::Retired,
            )) => return Ok(None),
            OperationAnswer::Replication(ReplicationAnswer::CopyUnavailable { reason }) => {
                return Err(io::Error::other(format!(
                    "grant CopyStatus is unavailable: {reason:?}"
                )));
            }
            _ => {
                return Err(io::Error::other(
                    "grant CopyStatus returned an unexpected answer",
                ));
            }
        };
        if status.database != database
            || status.role != CopyRole::Authoritative
            || status.fence != hinted_epoch
            || status.holder != Some(self.peer)
        {
            return Ok(None);
        }
        let request_started = SystemClock.now();
        let answer = self
            .requests
            .request(OperationRequest::Lease(LeaseRequest::Renew(RenewRequest {
                attempt: RequestId::new(UuidV7Ids.new_id()).map_err(io::Error::other)?,
                database,
                epoch: hinted_epoch,
            })))
            .await
            .map_err(|failure| {
                io::Error::other(format!("grant confirmation Renew: {failure:?}"))
            })?;
        let current = match answer {
            OperationAnswer::Lease(LeaseAnswer::Renew(RenewAnswer::Renewed {
                current, ..
            })) => current,
            OperationAnswer::Lease(LeaseAnswer::Renew(RenewAnswer::Refused(_))) => return Ok(None),
            _ => {
                return Err(io::Error::other(
                    "grant confirmation returned another answer",
                ));
            }
        };
        if current.database != database
            || current.epoch != hinted_epoch
            || current.holder != self.peer
            || current.state != LeaseState::Held
        {
            return Ok(None);
        }
        Ok(Some(BootstrapGrant {
            lease: fleet_sync_protocol::LeaseStatus {
                remaining_ms: 0,
                ..current
            },
            request_started,
            start: status.start,
        }))
    }
}
