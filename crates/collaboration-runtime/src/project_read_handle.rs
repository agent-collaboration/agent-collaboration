//! A registry snapshot holds its project's shared gate until the native read finishes.
use crate::ProjectRegistryError;
use crate::project_store_registry::ProjectRuntime;
use fleet_sync_lease::{HolderError, WriterState};
use fleet_sync_protocol::GateReason;
use fleet_sync_writer::OpenError;
use message_board::{Identity, PostId, ProjectId, ThreadId};
use message_board_storage::{ProjectSnapshot, ProjectStorageError, StoreSchemaState};
use std::{
    collections::BTreeSet,
    ops::{Deref, DerefMut},
    sync::Arc,
};
use tokio::sync::{OwnedRwLockReadGuard, RwLock};

#[derive(Clone)]
pub struct ProjectReadHandle {
    project: ProjectId,
    source: ProjectReadSource,
    unavailable: Arc<RwLock<BTreeSet<ProjectId>>>,
}

pub struct ProjectReadSnapshot {
    snapshot: ProjectSnapshot,
    status: ProjectReadStatus,
    _owner: ProjectReadOwner,
}

#[derive(Clone)]
enum ProjectReadSource {
    Writer(Arc<RwLock<ProjectRuntime>>),
    Central(Arc<crate::central_project_reads::CentralReadGeneration>),
}
enum ProjectReadOwner {
    Writer(OwnedRwLockReadGuard<ProjectRuntime>),
    Central(Arc<crate::central_project_reads::CentralReadGeneration>),
}
impl ProjectReadOwner {
    fn reader<'a>(
        &'a self,
        project: &ProjectId,
    ) -> Result<message_board_storage::ProjectReadStore<'a>, ProjectRegistryError> {
        match self {
            Self::Writer(owner) => owner.reader(project),
            Self::Central(generation) => Ok(generation.copy.reader()),
        }
    }
}

enum ProjectReadStatus {
    Central {
        writer: message_board::CurrentWriter,
        observation: Option<message_board::WriterObservation>,
    },
    Writer(WriterState),
    RegistrationRejected {
        reason: GateReason,
    },
}

impl ProjectReadHandle {
    pub(crate) fn new(
        project: ProjectId,
        runtime: Arc<RwLock<ProjectRuntime>>,
        unavailable: Arc<RwLock<BTreeSet<ProjectId>>>,
    ) -> Self {
        Self {
            project,
            source: ProjectReadSource::Writer(runtime),
            unavailable,
        }
    }

    pub(crate) fn central(
        project: ProjectId,
        generation: Arc<crate::central_project_reads::CentralReadGeneration>,
        unavailable: Arc<RwLock<BTreeSet<ProjectId>>>,
    ) -> Self {
        Self {
            project,
            source: ProjectReadSource::Central(generation),
            unavailable,
        }
    }
    async fn acquire(&self) -> ProjectReadOwner {
        match &self.source {
            ProjectReadSource::Writer(runtime) => {
                ProjectReadOwner::Writer(runtime.clone().read_owned().await)
            }
            ProjectReadSource::Central(generation) => ProjectReadOwner::Central(generation.clone()),
        }
    }

    pub async fn schema_state(&self) -> Result<StoreSchemaState, ProjectRegistryError> {
        let owner = self.acquire().await;
        self.check_available().await?;
        Ok(owner.reader(&self.project)?.schema_state().clone())
    }

    pub async fn registration_outcome(
        &self,
    ) -> Result<Option<fleet_sync_writer::WriterRegistrationOutcome>, ProjectRegistryError> {
        let owner = self.acquire().await;
        self.check_available().await?;
        match &owner {
            ProjectReadOwner::Writer(owner) => match &**owner {
                ProjectRuntime::Writable { store, .. } => {
                    Ok(Some(store.registration_outcome().await?))
                }
                _ => Err(self.unavailable()),
            },
            ProjectReadOwner::Central(_) => Ok(None),
        }
    }

    pub async fn snapshot(&self) -> Result<ProjectReadSnapshot, ProjectRegistryError> {
        let owner = self.acquire().await;
        self.check_available().await?;
        // Obtain writer status before checking out a read connection. No read
        // waits on a writer task or callback that may need its connection back.
        let status = match &owner {
            ProjectReadOwner::Central(generation) => {
                let (writer, observation) =
                    generation.authority().map_err(|_| self.unavailable())?;
                ProjectReadStatus::Central {
                    writer,
                    observation,
                }
            }
            ProjectReadOwner::Writer(owner) => match &**owner {
                ProjectRuntime::Writable { store, .. } => match store.writer_state().await {
                    Ok(WriterState::Superseded { .. } | WriterState::Released { .. }) => {
                        return Err(self.unavailable());
                    }
                    Ok(state) => ProjectReadStatus::Writer(state),
                    Err(ProjectStorageError::Writer(OpenError::Holder {
                        error: HolderError::RegistrationRejected { reason },
                    })) => ProjectReadStatus::RegistrationRejected { reason },
                    Err(error) => return Err(error.into()),
                },
                ProjectRuntime::SnapshotInstalling { .. } => {
                    return Err(ProjectRegistryError::Installing {
                        project: self.project.clone(),
                    });
                }
                ProjectRuntime::Superseded { .. } | ProjectRuntime::Unavailable { .. } => {
                    return Err(self.unavailable());
                }
            },
        };
        let snapshot = owner.reader(&self.project)?.snapshot().await?;
        Ok(ProjectReadSnapshot {
            snapshot,
            status,
            _owner: owner,
        })
    }

    pub async fn has_thread(&self, thread: &ThreadId) -> Result<bool, ProjectRegistryError> {
        let owner = self.acquire().await;
        self.check_available().await?;
        Ok(owner.reader(&self.project)?.has_thread(thread).await?)
    }

    pub async fn has_post(&self, post: &PostId) -> Result<bool, ProjectRegistryError> {
        let owner = self.acquire().await;
        self.check_available().await?;
        Ok(owner.reader(&self.project)?.has_post(post).await?)
    }

    pub async fn has_subscription_memberships(
        &self,
        reader: Option<&Identity>,
    ) -> Result<bool, ProjectRegistryError> {
        let owner = self.acquire().await;
        self.check_available().await?;
        Ok(owner
            .reader(&self.project)?
            .has_subscription_memberships(reader)
            .await?)
    }

    pub async fn has_active_inbox_watch(
        &self,
        reader: &Identity,
    ) -> Result<bool, ProjectRegistryError> {
        let owner = self.acquire().await;
        self.check_available().await?;
        Ok(owner
            .reader(&self.project)?
            .has_active_inbox_watch(reader)
            .await?)
    }

    async fn check_available(&self) -> Result<(), ProjectRegistryError> {
        if self.unavailable.read().await.contains(&self.project) {
            return Err(ProjectRegistryError::Quarantined {
                project: self.project.clone(),
            });
        }
        Ok(())
    }

    fn unavailable(&self) -> ProjectRegistryError {
        ProjectRegistryError::Unavailable {
            project: self.project.clone(),
        }
    }
}

impl ProjectReadSnapshot {
    pub fn view_context(
        &self,
    ) -> Option<(
        message_board::CurrentWriter,
        Option<message_board::WriterObservation>,
        message_board::CopySource,
    )> {
        use message_board::{CopySource, CurrentWriter};
        match &self.status {
            ProjectReadStatus::Central {
                writer,
                observation,
            } => Some((*writer, *observation, CopySource::ReadCopy {})),
            ProjectReadStatus::RegistrationRejected { reason } => {
                tracing::debug!(?reason, "rejected registration has no project view");
                None
            }
            ProjectReadStatus::Writer(state) => {
                let epoch = match state {
                    WriterState::Seeding { epoch }
                    | WriterState::Writing { epoch }
                    | WriterState::Paused { epoch, .. }
                    | WriterState::Draining { epoch, .. } => *epoch,
                    _ => return None,
                };
                Some((
                    CurrentWriter::Held {
                        epoch: epoch.value().try_into().ok()?,
                    },
                    None,
                    CopySource::Writer {},
                ))
            }
        }
    }

    pub async fn finish(self) -> Result<(), ProjectStorageError> {
        self.snapshot.finish().await
    }
}

impl Deref for ProjectReadSnapshot {
    type Target = ProjectSnapshot;

    fn deref(&self) -> &Self::Target {
        &self.snapshot
    }
}

impl DerefMut for ProjectReadSnapshot {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.snapshot
    }
}
