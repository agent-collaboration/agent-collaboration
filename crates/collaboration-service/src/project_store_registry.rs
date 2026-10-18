//! Own project file handles without serializing different projects' admissions.
use message_board::{ProjectId, ThreadRef};
use message_board_storage::{
    ProjectCommitGuard, ProjectLedgerReader, ProjectStorageError, ProjectStore,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::{Mutex, OnceCell, RwLock};

type ProjectSlot = Arc<OnceCell<Arc<ProjectStore>>>;

#[derive(Debug, thiserror::Error)]
pub enum ProjectRegistryError {
    #[error("project directory is unavailable: {0}")]
    Directory(#[from] std::io::Error),
    #[error(transparent)]
    Storage(#[from] ProjectStorageError),
    #[error("the project is orphaned after directory rejection")]
    ProjectRejected,
    #[error("the committed writer guard belongs to another project")]
    InvalidTransition,
    #[error("project storage is still owned during shutdown: {project:?}")]
    StillOwned { project: ProjectId },
    #[error("project storage is unavailable: {project:?}")]
    Unavailable { project: ProjectId },
}

pub struct ProjectStoreRegistry {
    projects_directory: PathBuf,
    cursor_key: OnceCell<message_board_storage::CursorSigningKey>,
    projects: Mutex<BTreeMap<ProjectId, ProjectSlot>>,
    activity: tokio::sync::broadcast::Sender<ProjectId>,
    rejected: RwLock<BTreeMap<ProjectId, Arc<ProjectLedgerReader>>>,
    unavailable: RwLock<BTreeSet<ProjectId>>,
}

impl ProjectStoreRegistry {
    pub(crate) async fn thread_project(
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
                    failure.get_or_insert(ProjectRegistryError::Storage(error));
                }
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(None),
        }
    }
    async fn open_project_handles(&self) -> Vec<(ProjectId, Arc<ProjectStore>)> {
        let unavailable = self.unavailable.read().await;
        let opened: Vec<_> = self
            .projects
            .lock()
            .await
            .iter()
            .filter_map(|(project, slot)| {
                slot.get()
                    .filter(|store| store.is_open() && !unavailable.contains(project))
                    .map(|store| (project.clone(), store.clone()))
            })
            .collect();
        drop(unavailable);
        opened
    }
    pub(crate) async fn post_projects(
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
                    failure.get_or_insert(ProjectRegistryError::Storage(error));
                }
            }
        }
        match failure {
            Some(error) if matches.is_empty() => Err(error),
            _ => Ok(matches),
        }
    }
    #[must_use]
    pub fn new(router_directory: &Path) -> Self {
        Self {
            projects_directory: router_directory.join("projects"),
            cursor_key: OnceCell::new(),
            projects: Mutex::new(BTreeMap::new()),
            activity: tokio::sync::broadcast::channel(256).0,
            rejected: RwLock::new(BTreeMap::new()),
            unavailable: RwLock::new(BTreeSet::new()),
        }
    }

    pub(crate) fn notify_activity(&self, project: &ProjectId) {
        let _ = self.activity.send(project.clone());
    }
    pub(crate) fn subscribe_activity(&self) -> tokio::sync::broadcast::Receiver<ProjectId> {
        self.activity.subscribe()
    }

    pub(crate) async fn cursor_key(
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
                    message_board_storage::CursorStore::open(&root.join("legacy-cursors.sqlite"))
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
    pub async fn open_project(
        &self,
        project: &ProjectId,
    ) -> Result<Arc<ProjectStore>, ProjectRegistryError> {
        if self.unavailable.read().await.contains(project) {
            return Err(ProjectRegistryError::Unavailable {
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
        let opened = slot
            .get_or_try_init(|| async {
                tokio::fs::create_dir_all(&self.projects_directory).await?;
                let path = self
                    .projects_directory
                    .join(format!("{}.sqlite", project.as_str()));
                Ok::<_, ProjectRegistryError>(Arc::new(
                    ProjectStore::open(&path, project.clone()).await?,
                ))
            })
            .await?;
        Ok(Arc::clone(opened))
    }

    /// A failed project read remains isolated until its owner repairs and reopens it.
    pub(crate) async fn mark_unavailable(&self, project: &ProjectId) {
        self.unavailable.write().await.insert(project.clone());
    }

    /// Reading an unknown project must not create a database file.
    pub async fn existing_project(
        &self,
        project: &ProjectId,
    ) -> Result<Option<Arc<ProjectStore>>, ProjectRegistryError> {
        if self.rejected_project(project).await?.is_some() {
            return Ok(None);
        }
        let path = self
            .projects_directory
            .join(format!("{}.sqlite", project.as_str()));
        if !tokio::fs::try_exists(path).await? {
            return Ok(None);
        }
        self.open_project(project).await.map(Some)
    }

    /// Scope selection needs the rejection identity, never its retained ledger.
    pub(crate) async fn is_rejected_project(
        &self,
        project: &ProjectId,
    ) -> Result<bool, ProjectRegistryError> {
        if self.rejected.read().await.contains_key(project) {
            return Ok(true);
        }
        Ok(tokio::fs::try_exists(
            self.projects_directory
                .join(format!("{}.orphaned.sqlite", project.as_str())),
        )
        .await?)
    }

    pub async fn rejected_project(
        &self,
        project: &ProjectId,
    ) -> Result<Option<Arc<ProjectLedgerReader>>, ProjectRegistryError> {
        if let Some(reader) = self.rejected.read().await.get(project) {
            return Ok(Some(reader.clone()));
        }
        let path = self
            .projects_directory
            .join(format!("{}.orphaned.sqlite", project.as_str()));
        if !tokio::fs::try_exists(&path).await? {
            return Ok(None);
        }
        let mut readers = self.rejected.write().await;
        if let Some(reader) = readers.get(project) {
            return Ok(Some(reader.clone()));
        }
        let reader = Arc::new(ProjectLedgerReader::open(&path, project.clone()).await?);
        readers.insert(project.clone(), reader.clone());
        Ok(Some(reader))
    }

    /// Caller retains the project mutex from voiding through this file transition.
    pub async fn orphan_project(
        &self,
        project: &ProjectId,
        commit: ProjectCommitGuard,
    ) -> Result<(), ProjectRegistryError> {
        if commit.project_id() != project {
            return Err(ProjectRegistryError::InvalidTransition);
        }
        let slot = self.projects.lock().await.get(project).cloned();
        let Some(store) = slot.as_ref().and_then(|slot| slot.get()) else {
            return Err(ProjectRegistryError::InvalidTransition);
        };
        let path = self
            .projects_directory
            .join(format!("{}.orphaned.sqlite", project.as_str()));
        store.move_aside(&commit, &path).await?;
        let reader = Arc::new(ProjectLedgerReader::open(&path, project.clone()).await?);
        self.rejected.write().await.insert(project.clone(), reader);
        drop(commit);
        Ok(())
    }

    pub async fn project_ids(&self) -> Result<Vec<ProjectId>, ProjectRegistryError> {
        let mut directory = match tokio::fs::read_dir(&self.projects_directory).await {
            Ok(directory) => directory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut ids = Vec::new();
        while let Some(entry) = directory.next_entry().await? {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.ends_with(".orphaned.sqlite") {
                continue;
            }
            if let Some(id) = name.strip_suffix(".sqlite")
                && let Ok(id) = ProjectId::try_from(id.to_owned())
            {
                ids.push(id);
            }
        }
        ids.sort();
        Ok(ids)
    }
    pub async fn close(self) -> Result<(), ProjectRegistryError> {
        for (project, slot) in self.projects.into_inner() {
            let slot = Arc::try_unwrap(slot).map_err(|_| ProjectRegistryError::StillOwned {
                project: project.clone(),
            })?;
            if let Some(store) = slot.into_inner() {
                let store =
                    Arc::try_unwrap(store).map_err(|_| ProjectRegistryError::StillOwned {
                        project: project.clone(),
                    })?;
                store.close().await;
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
