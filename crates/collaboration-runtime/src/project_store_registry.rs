//! Own project file handles without serializing different projects' admissions.
use board_fleet_sync::{BOARD_LEASE_POLICY, BoardProjectSchema};
use fleet_sync_lease::LeaseHolder;
use fleet_sync_protocol::{DatabaseId, Epoch, PeerId};
use fleet_sync_writer::{ReplicatedDatabase, RetainedAuthority, WriteRefusal, WriterOptions};
use message_board::{EpochNumber, ProjectId, ThreadRef};
use message_board_storage::{
    ProjectLedgerReader, ProjectReadStore, ProjectStorageError, ProjectStore,
};
use runtime_primitives::Clock;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::{Mutex, OnceCell, RwLock};

#[path = "project_registry_reads.rs"]
mod project_registry_reads;

pub type NativeProjectStore = ProjectStore<BoardProjectSchema>;
type ProjectSlot = Arc<OnceCell<Arc<RwLock<ProjectRuntime>>>>;
pub(crate) enum ProjectRuntime {
    SnapshotInstalling {
        database: Arc<ReplicatedDatabase<BoardProjectSchema>>,
    },
    Writable {
        store: Arc<NativeProjectStore>,
        database: Arc<ReplicatedDatabase<BoardProjectSchema>>,
    },
    Superseded {
        current: Option<EpochNumber>,
    },
    Unavailable {
        closing: Option<Arc<ReplicatedDatabase<BoardProjectSchema>>>,
        superseded_current: Option<EpochNumber>,
    },
}

impl ProjectRuntime {
    pub(crate) fn reader(
        &self,
        project: &ProjectId,
    ) -> Result<ProjectReadStore<'_>, ProjectRegistryError> {
        match self {
            Self::Writable { store, .. } => Ok(store.reader()),
            Self::SnapshotInstalling { .. } => Err(ProjectRegistryError::Installing {
                project: project.clone(),
            }),
            Self::Superseded { .. } | Self::Unavailable { .. } => {
                Err(ProjectRegistryError::Unavailable {
                    project: project.clone(),
                })
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProjectRegistryError {
    #[error("project directory is unavailable: {0}")]
    Directory(#[from] std::io::Error),
    #[error(transparent)]
    Storage(#[from] ProjectStorageError),
    #[error("the Router has not enrolled with central")]
    NotEnrolled,
    #[error(transparent)]
    Schema(#[from] board_fleet_sync::BoardProjectSchemaError),
    #[error(transparent)]
    Move(#[from] fleet_sync_writer::MoveAsideError),
    #[error("the project is orphaned after directory rejection")]
    ProjectRejected,
    #[error("the committed writer guard belongs to another project")]
    InvalidTransition,
    #[error("project storage is still owned during shutdown: {project:?}")]
    StillOwned { project: ProjectId },
    #[error("project storage is unavailable: {project:?}")]
    Unavailable { project: ProjectId },
    #[error("project snapshot is still installing: {project:?}")]
    Installing { project: ProjectId },
    #[error("project storage is quarantined: {project:?}")]
    Quarantined { project: ProjectId },
}

pub struct ProjectStoreRegistry {
    pub(crate) projects_directory: PathBuf,
    cursor_key: OnceCell<message_board_storage::CursorSigningKey>,
    pub(crate) projects: Mutex<BTreeMap<ProjectId, ProjectSlot>>,
    activity: tokio::sync::broadcast::Sender<ProjectId>,
    rejected: RwLock<BTreeMap<ProjectId, Arc<ProjectLedgerReader>>>,
    unavailable: Arc<RwLock<BTreeSet<ProjectId>>>,
    pub(crate) peer: Option<PeerId>,
    clock: Arc<dyn Clock>,
    central_reads: std::sync::OnceLock<crate::central_project_reads::CentralProjectReads>,
}

impl board_fleet_sync::LeaseAuthority for ProjectStoreRegistry {
    type Error = ProjectRegistryError;

    async fn writer_state(
        &self,
        project: &ProjectId,
    ) -> Result<fleet_sync_lease::WriterState, Self::Error> {
        let store = self.existing_project(project).await?.ok_or_else(|| {
            ProjectRegistryError::Unavailable {
                project: project.clone(),
            }
        })?;
        Ok(store.writer_state().await?)
    }
}

impl ProjectStoreRegistry {
    pub async fn thread_project(
        &self,
        reference: &ThreadRef,
    ) -> Result<Option<ProjectId>, ProjectRegistryError> {
        if let Some(project) = &reference.project {
            return Ok(Some(project.clone()));
        }
        let mut failure = None;
        for (project, store) in self.open_project_handles().await {
            match store.has_thread(&reference.thread).await {
                Ok(true) => return Ok(Some(project)),
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(?project, %error, "open-project thread probe unavailable");
                    failure.get_or_insert(error);
                }
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(None),
        }
    }
    async fn open_project_handles(&self) -> Vec<(ProjectId, crate::ProjectReadHandle)> {
        let unavailable = self.unavailable.read().await;
        self.projects
            .lock()
            .await
            .iter()
            .filter_map(|(project, slot)| {
                slot.get()
                    .filter(|_| !unavailable.contains(project))
                    .map(|runtime| {
                        (
                            project.clone(),
                            crate::ProjectReadHandle::new(
                                project.clone(),
                                runtime.clone(),
                                self.unavailable.clone(),
                            ),
                        )
                    })
            })
            .collect()
    }
    pub async fn post_projects(
        &self,
        post: &message_board::PostId,
    ) -> Result<Vec<ProjectId>, ProjectRegistryError> {
        let mut matches = Vec::new();
        let mut failure = None;
        for (project, store) in self.open_project_handles().await {
            match store.has_post(post).await {
                Ok(true) => {
                    matches.push(project);
                    if matches.len() == 2 {
                        return Ok(matches);
                    }
                }
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(?project, %error, "open-project post probe unavailable");
                    failure.get_or_insert(error);
                }
            }
        }
        match failure {
            Some(error) if matches.is_empty() => Err(error),
            _ => Ok(matches),
        }
    }
    #[must_use]
    pub fn new(router_directory: &Path, peer: Option<PeerId>, clock: Arc<dyn Clock>) -> Self {
        Self {
            projects_directory: router_directory.join("projects"),
            cursor_key: OnceCell::new(),
            projects: Mutex::new(BTreeMap::new()),
            activity: tokio::sync::broadcast::channel(256).0,
            rejected: RwLock::new(BTreeMap::new()),
            unavailable: Arc::new(RwLock::new(BTreeSet::new())),
            peer,
            clock,
            central_reads: std::sync::OnceLock::new(),
        }
    }

    pub fn notify_activity(&self, project: &ProjectId) {
        let _ = self.activity.send(project.clone());
    }
    pub fn subscribe_activity(&self) -> tokio::sync::broadcast::Receiver<ProjectId> {
        self.activity.subscribe()
    }

    pub async fn cursor_key(
        &self,
    ) -> Result<&message_board_storage::CursorSigningKey, ProjectRegistryError> {
        self.cursor_key
            .get_or_try_init(|| async {
                let root = self
                    .projects_directory
                    .parent()
                    .ok_or(ProjectRegistryError::InvalidTransition)?;
                tokio::fs::create_dir_all(root).await?;
                let store =
                    message_board_storage::CursorStore::open(&root.join("board-cursors.sqlite"))
                        .await
                        .map_err(|error| std::io::Error::other(error.to_string()))?;
                let mut candidate = vec![0_u8; 32];
                getrandom::fill(&mut candidate)
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
                let key = store
                    .get_or_install_key(
                        &message_board_storage::CursorSigningKey::try_from(candidate)
                            .map_err(|error| std::io::Error::other(error.to_string()))?,
                    )
                    .await
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
                store.close().await;
                Ok(key)
            })
            .await
    }
    async fn project_runtime(
        &self,
        project: &ProjectId,
    ) -> Result<Arc<RwLock<ProjectRuntime>>, ProjectRegistryError> {
        if self.unavailable.read().await.contains(project) {
            return Err(ProjectRegistryError::Quarantined {
                project: project.clone(),
            });
        }
        if self.rejected_project(project).await?.is_some() {
            return Err(ProjectRegistryError::ProjectRejected);
        }
        let slot = self
            .projects
            .lock()
            .await
            .entry(project.clone())
            .or_insert_with(|| Arc::new(OnceCell::new()))
            .clone();
        // Keep cold initialization state out of cached project lookups' future layouts.
        slot.get_or_try_init(|| Box::pin(async {
            let directory = self.projects_directory.join(project.as_str());
            let existing = crate::project_store_paths::live_file_with_epoch(&directory).await?;
            let bootstrap_origin = if let Some((epoch, path)) = &existing {
                // Inspect persisted authority before constructing any writer.
                let inspected = fleet_sync_writer::inspect_native_file(path).await
                    .map_err(ProjectStorageError::from)?;
                let expected_database = DatabaseId::new(
                    uuid::Uuid::parse_str(project.as_str())
                        .map_err(|_| ProjectRegistryError::InvalidTransition)?,
                ).map_err(|_| ProjectRegistryError::InvalidTransition)?;
                let mismatch = if inspected.origin.database != expected_database {
                    Some("writer.database")
                } else if inspected.origin.epoch.value() != epoch.get() {
                    Some("writer.epoch")
                } else { None };
                if let Some(field) = mismatch {
                    return Err(ProjectStorageError::Writer(
                        fleet_sync_writer::OpenError::Corrupt { field: field.to_owned() },
                    ).into());
                }
                match inspected.authority {
                    Some(RetainedAuthority::Superseded { current, .. }) => {
                        let current = current.map(|epoch| EpochNumber::try_from(epoch.value()))
                            .transpose().map_err(|_| ProjectRegistryError::InvalidTransition)?;
                        return Ok(Arc::new(RwLock::new(ProjectRuntime::Superseded { current })));
                    }
                    Some(_) => return Err(ProjectRegistryError::Unavailable {
                        project: project.clone(),
                    }),
                    None => {},
                }
                inspected.origin.kind == fleet_sync_writer::RetainedFileOriginKind::Bootstrap
            } else { false };
            let peer = self.peer.ok_or(ProjectRegistryError::NotEnrolled)?;
            tokio::fs::create_dir_all(&directory).await?;
            let (epoch, path) = existing.unwrap_or_else(|| {
                (EpochNumber::INITIAL, directory.join("e1.sqlite"))
            });
            let database_id = DatabaseId::new(
                uuid::Uuid::parse_str(project.as_str())
                    .map_err(|_| ProjectRegistryError::InvalidTransition)?,
            ).map_err(|_| ProjectRegistryError::InvalidTransition)?;
            let holder = if epoch == EpochNumber::INITIAL {
                LeaseHolder::provisional(database_id, peer, BOARD_LEASE_POLICY)
            } else {
                LeaseHolder::seeding(
                    database_id, peer,
                    Epoch::new(epoch.get()).map_err(|_| ProjectRegistryError::InvalidTransition)?,
                    BOARD_LEASE_POLICY,
                )
            };
            tracing::trace!(target: "message_board_storage::open", project = %project.as_str(), "project store open");
            let database = Arc::new(ReplicatedDatabase::open_with(
                &path, database_id, BoardProjectSchema::new()?, holder, self.clock.clone(),
                WriterOptions { allow_provisional_writes: false },
            ).await.map_err(ProjectStorageError::from)?);
            if bootstrap_origin && matches!(database.writer_state().await.map_err(ProjectStorageError::from)?, fleet_sync_lease::WriterState::Seeding { .. }) {
                // Reopening does not publish a granted file whose installer
                // has not reached Ready. Claim resumes its durable attempt.
                return Ok(Arc::new(RwLock::new(ProjectRuntime::SnapshotInstalling { database })));
            }
            let store = Arc::new(ProjectStore::from_database(project.clone(), database.clone()).await?);
            Ok::<_, ProjectRegistryError>(Arc::new(RwLock::new(ProjectRuntime::Writable {
                store, database,
            })))
        })).await.cloned()
    }

    pub async fn open_project(
        &self,
        project: &ProjectId,
    ) -> Result<Arc<NativeProjectStore>, ProjectRegistryError> {
        let runtime = self.project_runtime(project).await?;
        let owner = runtime.read().await;
        match &*owner {
            ProjectRuntime::Writable { store, .. } => Ok(store.clone()),
            ProjectRuntime::Superseded { current } => Err(superseded_write_refusal(*current)),
            ProjectRuntime::SnapshotInstalling { .. } => Err(ProjectRegistryError::Installing {
                project: project.clone(),
            }),
            ProjectRuntime::Unavailable { .. } => Err(ProjectRegistryError::Unavailable {
                project: project.clone(),
            }),
        }
    }

    /// The external runtime owner has stopped and joined every writer task first.
    pub async fn supersede_project(&self, project: &ProjectId) -> Result<(), ProjectRegistryError> {
        let runtime = self.project_runtime(project).await?;
        let mut owner = runtime.write().await;
        let (database, current) = match &*owner {
            ProjectRuntime::Writable { database, .. }
            | ProjectRuntime::SnapshotInstalling { database } => {
                let fleet_sync_lease::WriterState::Superseded { current } = database
                    .writer_state()
                    .await
                    .map_err(ProjectStorageError::from)?
                else {
                    return Err(ProjectRegistryError::InvalidTransition);
                };
                let current = EpochNumber::try_from(current.value())
                    .map_err(|_| ProjectRegistryError::InvalidTransition)?;
                (database.clone(), current)
            }
            ProjectRuntime::Unavailable {
                closing: Some(database),
                superseded_current: Some(current),
            } => (database.clone(), *current),
            ProjectRuntime::Superseded { .. } => return Ok(()),
            ProjectRuntime::Unavailable { .. } => {
                return Err(ProjectRegistryError::Unavailable {
                    project: project.clone(),
                });
            }
        };
        // The resource and actual successor survive cancellation, so a retry
        // drains the same native owner without reopening or reading the file.
        *owner = ProjectRuntime::Unavailable {
            closing: Some(database.clone()),
            superseded_current: Some(current),
        };
        tracing::trace!(target: "collaboration_service::project_native_close", "native project drain started");
        database.close().await;
        *owner = ProjectRuntime::Superseded {
            current: Some(current),
        };
        Ok(())
    }

    /// Read cached completed authority without waiting on an in-progress close.
    pub async fn superseded_current(&self, project: &ProjectId) -> Option<Option<EpochNumber>> {
        let runtime = self
            .projects
            .lock()
            .await
            .get(project)
            .and_then(|slot| slot.get())
            .cloned()?;
        let owner = runtime.try_read().ok()?;
        match &*owner {
            ProjectRuntime::Superseded { current } => Some(*current),
            _ => None,
        }
    }

    /// Inspect persisted authority without constructing a superseded writer or reader.
    pub async fn is_superseded(&self, project: &ProjectId) -> Result<bool, ProjectRegistryError> {
        if self.superseded_current(project).await.is_some() {
            return Ok(true);
        }
        if crate::project_store_paths::live_file(&self.projects_directory.join(project.as_str()))
            .await?
            .is_none()
        {
            return Ok(false);
        }
        let runtime = self.project_runtime(project).await?;
        Ok(matches!(
            *runtime.read().await,
            ProjectRuntime::Superseded { .. }
        ))
    }

    /// A failed project read remains isolated until its owner repairs and reopens it.
    pub async fn mark_unavailable(&self, project: &ProjectId) {
        self.unavailable.write().await.insert(project.clone());
    }

    /// Reading an unknown project must not create a database file.
    pub async fn existing_project(
        &self,
        project: &ProjectId,
    ) -> Result<Option<Arc<NativeProjectStore>>, ProjectRegistryError> {
        if self.rejected_project(project).await?.is_some() {
            return Ok(None);
        }
        if crate::project_store_paths::live_file(&self.projects_directory.join(project.as_str()))
            .await?
            .is_none()
        {
            return Ok(None);
        }
        self.open_project(project).await.map(Some)
    }

    /// Scope selection needs the rejection identity, never its retained ledger.
    pub async fn is_rejected_project(
        &self,
        project: &ProjectId,
    ) -> Result<bool, ProjectRegistryError> {
        if self.rejected.read().await.contains_key(project) {
            return Ok(true);
        }
        Ok(crate::project_store_paths::rejected_file(
            &self.projects_directory.join(project.as_str()),
        )
        .await?
        .is_some())
    }

    pub async fn rejected_project(
        &self,
        project: &ProjectId,
    ) -> Result<Option<Arc<ProjectLedgerReader>>, ProjectRegistryError> {
        if let Some(reader) = self.rejected.read().await.get(project) {
            return Ok(Some(reader.clone()));
        }
        let Some(path) = crate::project_store_paths::rejected_file(
            &self.projects_directory.join(project.as_str()),
        )
        .await?
        else {
            return Ok(None);
        };
        let mut readers = self.rejected.write().await;
        if let Some(reader) = readers.get(project) {
            return Ok(Some(reader.clone()));
        }
        let reader = Arc::new(
            ProjectLedgerReader::open(&path, project.clone(), BoardProjectSchema::new()?).await?,
        );
        readers.insert(project.clone(), reader.clone());
        Ok(Some(reader))
    }

    /// The Host stops and joins this project's runtime before its terminal library move.
    pub async fn orphan_project(&self, project: &ProjectId) -> Result<(), ProjectRegistryError> {
        let runtime = self.project_runtime(project).await?;
        let mut owner = runtime.write().await;
        let ProjectRuntime::Writable { database, .. } = &*owner else {
            return Err(ProjectRegistryError::InvalidTransition);
        };
        let database = database.clone();
        let path = database.path().with_extension("orphaned.sqlite");
        self.unavailable.write().await.insert(project.clone());
        *owner = ProjectRuntime::Unavailable {
            closing: Some(database.clone()),
            superseded_current: None,
        };
        database.move_aside(&path).await?;
        let reader = Arc::new(
            ProjectLedgerReader::open(&path, project.clone(), BoardProjectSchema::new()?).await?,
        );
        self.rejected.write().await.insert(project.clone(), reader);
        Ok(())
    }

    /// The Host composes only the writable owner admitted by the registry mode.
    pub async fn database(
        &self,
        project: &ProjectId,
    ) -> Result<Arc<ReplicatedDatabase<BoardProjectSchema>>, ProjectRegistryError> {
        let runtime = self.project_runtime(project).await?;
        let owner = runtime.read().await;
        match &*owner {
            ProjectRuntime::Writable { database, .. } => Ok(database.clone()),
            ProjectRuntime::Superseded { current } => Err(superseded_write_refusal(*current)),
            ProjectRuntime::SnapshotInstalling { .. } => Err(ProjectRegistryError::Installing {
                project: project.clone(),
            }),
            ProjectRuntime::Unavailable { .. } => Err(ProjectRegistryError::Unavailable {
                project: project.clone(),
            }),
        }
    }

    pub async fn project_ids(&self) -> Result<Vec<ProjectId>, ProjectRegistryError> {
        crate::project_store_paths::project_ids(&self.projects_directory).await
    }
    pub async fn close(self) -> Result<(), ProjectRegistryError> {
        for (project, slot) in self.projects.into_inner() {
            let slot = Arc::try_unwrap(slot).map_err(|_| ProjectRegistryError::StillOwned {
                project: project.clone(),
            })?;
            if let Some(runtime) = slot.into_inner() {
                let runtime = Arc::try_unwrap(runtime)
                    .map_err(|_| ProjectRegistryError::StillOwned {
                        project: project.clone(),
                    })?
                    .into_inner();
                match runtime {
                    ProjectRuntime::Writable { store, .. } => {
                        let store = Arc::try_unwrap(store).map_err(|_| {
                            ProjectRegistryError::StillOwned {
                                project: project.clone(),
                            }
                        })?;
                        store.close().await;
                    }
                    ProjectRuntime::SnapshotInstalling { database } => database.close().await,
                    ProjectRuntime::Superseded { .. } => {}
                    ProjectRuntime::Unavailable { closing, .. } => {
                        if let Some(database) = closing {
                            database.close().await;
                        }
                    }
                }
            }
        }
        for (project, reader) in self.rejected.into_inner() {
            let reader = Arc::try_unwrap(reader)
                .map_err(|_| ProjectRegistryError::StillOwned { project })?;
            reader.close().await;
        }
        Ok(())
    }
}

fn superseded_write_refusal(current: Option<EpochNumber>) -> ProjectRegistryError {
    let refusal = match current {
        Some(current) => match Epoch::new(current.get()) {
            Ok(current) => WriteRefusal::Superseded { current },
            Err(_) => return ProjectRegistryError::InvalidTransition,
        },
        None => WriteRefusal::NotWriter,
    };
    ProjectRegistryError::Storage(ProjectStorageError::WriteRefused(refusal))
}
