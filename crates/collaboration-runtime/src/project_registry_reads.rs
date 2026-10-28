//! Resolve immutable readers from the local writer or a completed central generation.
use super::{Arc, ProjectId, ProjectRegistryError, ProjectRuntime, ProjectStoreRegistry};

impl ProjectStoreRegistry {
    /// The write refusal path uses retained authority without refreshing a reader.
    pub async fn completed_project_authority(
        &self,
        project: &ProjectId,
    ) -> Result<
        Option<(
            message_board::CurrentWriter,
            Option<message_board::WriterObservation>,
        )>,
        ProjectRegistryError,
    > {
        let Some(reads) = self.central_reads.get() else {
            return Ok(None);
        };
        reads.completed_authority(project).await.map_err(|error| {
            tracing::warn!(%error, project = project.as_str(), "completed project authority unavailable");
            ProjectRegistryError::Unavailable {
                project: project.clone(),
            }
        })
    }

    /// Resolve reads through the current owner each time, including after a swap.
    pub async fn read_project(
        &self,
        project: &ProjectId,
    ) -> Result<Option<crate::ProjectReadHandle>, ProjectRegistryError> {
        if self.rejected_project(project).await?.is_some() {
            return Ok(None);
        }
        if crate::project_store_paths::live_file(&self.projects_directory.join(project.as_str()))
            .await?
            .is_none()
        {
            return self.central_read_project(project).await;
        }
        let runtime = self.project_runtime(project).await?;
        {
            let owner = runtime.read().await;
            let local = match &*owner {
                ProjectRuntime::Writable { store, .. } => !matches!(
                    store.writer_state().await,
                    Ok(fleet_sync_lease::WriterState::Superseded { .. }
                        | fleet_sync_lease::WriterState::Released { .. })
                ),
                _ => false,
            };
            if !local {
                drop(owner);
                return self
                    .central_read_project(project)
                    .await?
                    .map(Some)
                    .ok_or_else(|| ProjectRegistryError::Unavailable {
                        project: project.clone(),
                    });
            }
        }
        Ok(Some(crate::ProjectReadHandle::new(
            project.clone(),
            runtime,
            self.unavailable.clone(),
        )))
    }

    pub fn attach_central_reads(
        &self,
        source: Arc<dyn crate::ProjectReadCopySource>,
    ) -> Result<(), ProjectRegistryError> {
        self.central_reads
            .set(crate::central_project_reads::CentralProjectReads::new(
                source,
            ))
            .map_err(|_| ProjectRegistryError::InvalidTransition)
    }

    async fn central_read_project(
        &self,
        project: &ProjectId,
    ) -> Result<Option<crate::ProjectReadHandle>, ProjectRegistryError> {
        let Some(reads) = self.central_reads.get() else {
            return Ok(None);
        };
        let generation = reads.read(project).await.map_err(|error| {
            tracing::warn!(%error, project = project.as_str(), "central project read unavailable");
            ProjectRegistryError::Unavailable {
                project: project.clone(),
            }
        })?;
        Ok(Some(crate::ProjectReadHandle::central(
            project.clone(),
            generation,
            self.unavailable.clone(),
        )))
    }
}
