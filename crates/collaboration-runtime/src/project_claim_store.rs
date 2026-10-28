//! Claim installation keeps the native writer outside admission until its verified receipt is ready.
use crate::project_store_registry::{ProjectRuntime, ProjectStoreRegistry};
use crate::{NativeProjectStore, ProjectRegistryError};
use board_fleet_sync::{BOARD_LEASE_POLICY, BoardProjectSchema};
use fleet_sync_protocol::ReplicationPosition;
use fleet_sync_writer::{BootstrapGrant, ReplicatedDatabase};
use message_board::ProjectId;
use std::sync::Arc;
use tokio::sync::{OnceCell, RwLock};

impl ProjectStoreRegistry {
    /// Only the Host's per-project claim owner calls this with a real central grant.
    pub async fn prepare_claim_seed(
        &self,
        project: &ProjectId,
        grant: BootstrapGrant,
    ) -> Result<Arc<ReplicatedDatabase<BoardProjectSchema>>, ProjectRegistryError> {
        if grant.lease.database.to_string() != project.as_str()
            || Some(grant.lease.holder) != self.peer
        {
            return Err(ProjectRegistryError::InvalidTransition);
        }
        let path = self
            .projects_directory
            .join(project.as_str())
            .join(format!("e{}.sqlite", grant.lease.epoch.value()));
        let slot = self
            .projects
            .lock()
            .await
            .entry(project.clone())
            .or_insert_with(|| Arc::new(OnceCell::new()))
            .clone();
        let database_id = grant.lease.database;
        let expected_start = grant.start;
        let expected_epoch = grant.lease.epoch;
        let runtime = slot
            .get_or_try_init(|| async {
                tokio::fs::create_dir_all(
                    path.parent()
                        .ok_or(ProjectRegistryError::InvalidTransition)?,
                )
                .await?;
                let database = Arc::new(
                    ReplicatedDatabase::seeding(
                        &path,
                        BoardProjectSchema::new()?,
                        grant.clone(),
                        BOARD_LEASE_POLICY,
                    )
                    .await
                    .map_err(message_board_storage::ProjectStorageError::from)?,
                );
                Ok::<_, ProjectRegistryError>(Arc::new(RwLock::new(
                    ProjectRuntime::SnapshotInstalling { database },
                )))
            })
            .await?;
        let mut owner = runtime.write().await;
        if let ProjectRuntime::Superseded { current } = &*owner {
            if current.is_none_or(|current| current.get() > expected_epoch.value()) {
                return Err(ProjectRegistryError::InvalidTransition);
            }
            // The previous native runtime is closed. Retain its file and ledger
            // unread; only the newly confirmed epoch's file becomes an owner.
            let database = Arc::new(
                ReplicatedDatabase::seeding(
                    &path,
                    BoardProjectSchema::new()?,
                    grant,
                    BOARD_LEASE_POLICY,
                )
                .await
                .map_err(message_board_storage::ProjectStorageError::from)?,
            );
            *owner = ProjectRuntime::SnapshotInstalling { database };
        }
        let database = match &*owner {
            ProjectRuntime::SnapshotInstalling { database }
                if database.path() == path && database.database() == database_id =>
            {
                database.clone()
            }
            _ => return Err(ProjectRegistryError::InvalidTransition),
        };
        if let Some(receipt) = database
            .installed_snapshot()
            .await
            .map_err(message_board_storage::ProjectStorageError::from)?
            && receipt.install.position != expected_start
        {
            return Err(ProjectRegistryError::InvalidTransition);
        }
        Ok(database)
    }

    /// Ready is produced by the library installer after schema/prefix verification.
    pub async fn publish_claim_seed(
        &self,
        project: &ProjectId,
        database: Arc<ReplicatedDatabase<BoardProjectSchema>>,
        start: ReplicationPosition,
    ) -> Result<Arc<NativeProjectStore>, ProjectRegistryError> {
        let runtime = self
            .projects
            .lock()
            .await
            .get(project)
            .and_then(|slot| slot.get())
            .cloned()
            .ok_or(ProjectRegistryError::InvalidTransition)?;
        let mut owner = runtime.write().await;
        match &*owner {
            ProjectRuntime::SnapshotInstalling { database: expected }
            | ProjectRuntime::Writable {
                database: expected, ..
            } if Arc::ptr_eq(expected, &database) => {}
            _ => return Err(ProjectRegistryError::InvalidTransition),
        }
        let receipt = database
            .installed_snapshot()
            .await
            .map_err(message_board_storage::ProjectStorageError::from)?
            .ok_or(ProjectRegistryError::InvalidTransition)?;
        if !receipt.verified
            || !receipt.ready
            || receipt.install.position != start
            || receipt.install.database.to_string() != project.as_str()
        {
            return Err(ProjectRegistryError::InvalidTransition);
        }
        if !matches!(
            database
                .writer_state()
                .await
                .map_err(message_board_storage::ProjectStorageError::from)?,
            fleet_sync_lease::WriterState::Writing { .. }
        ) {
            return Err(ProjectRegistryError::InvalidTransition);
        }
        let store =
            Arc::new(NativeProjectStore::from_database(project.clone(), database.clone()).await?);
        *owner = ProjectRuntime::Writable {
            store: store.clone(),
            database,
        };
        Ok(store)
    }
}
