//! Keep one Host authority from source preparation through confirmed archive.
use crate::offline_run_plan::PlannedProject;
use crate::{
    OfflineHost, OfflineRunPlan, OwnerAuthentication, PreparedProjectBundle, ProjectBundleError,
    SourceArchiveOutcome, StagingPublisher, archive_confirmed_source,
};
use fleet_sync_protocol::{ReplicationPosition, RequestId as ImportId};
use message_board::{ProjectId, RequestId};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum OfflineRunError {
    #[error("offline run plan is invalid")]
    Plan(#[from] ProjectBundleError),
    #[error("cannot acquire the declared offline Host")]
    Host(#[from] crate::OfflineHostError),
    #[error("owner publication access is unavailable")]
    Owner(#[from] crate::PublicationError),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfflineRunReport {
    run_id: uuid::Uuid,
    projects: Vec<OfflineProjectOutcome>,
}

#[derive(Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum OfflineProjectOutcome {
    ReadyToClaim {
        project: ProjectId,
        bundle: PathBuf,
        claim_request: RequestId,
        canary_request: RequestId,
        import_id: ImportId,
        head: ReplicationPosition,
        archive: SourceArchiveOutcome,
    },
    Refused {
        project: ProjectId,
        stage: OfflineStage,
        reason: String,
        #[cfg(debug_assertions)]
        #[serde(skip_serializing_if = "Option::is_none")]
        diagnostic: Option<crate::staging_publication::PublicationDiagnostic>,
    },
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
enum OfflineStage {
    Preparation,
    Publication,
    Archive,
}

impl OfflineRunReport {
    pub fn all_ready(&self) -> bool {
        self.projects
            .iter()
            .all(|project| matches!(project, OfflineProjectOutcome::ReadyToClaim { .. }))
    }
}

impl OfflineRunPlan {
    pub async fn execute(
        &self,
        home: &Path,
        authentication: OwnerAuthentication,
    ) -> Result<OfflineRunReport, OfflineRunError> {
        self.validate()?;
        let mut host = self.acquire_host(home)?;
        let publisher = match authentication {
            OwnerAuthentication::OwnerAccess => {
                StagingPublisher::from_owner_access(self.target.clone()).await?
            }
            OwnerAuthentication::OwnerCredentialFile { path } => {
                StagingPublisher::from_owner_file(self.target.clone(), &path)?
            }
        };
        let mut projects = Vec::new();
        for project in &self.projects {
            projects.push(self.execute_project(&mut host, &publisher, project).await);
        }
        Ok(OfflineRunReport {
            run_id: self.run_id,
            projects,
        })
    }

    async fn execute_project(
        &self,
        host: &mut OfflineHost,
        publisher: &StagingPublisher,
        project: &PlannedProject,
    ) -> OfflineProjectOutcome {
        let refused = |stage, error: String| OfflineProjectOutcome::Refused {
            project: project.selected.project.clone(),
            stage,
            reason: error,
            #[cfg(debug_assertions)]
            diagnostic: None,
        };
        let bundle = match self.prepare_or_reopen(host, project).await {
            Ok(bundle) => bundle,
            Err(error) => return refused(OfflineStage::Preparation, error.to_string()),
        };
        let confirmed = match publisher.publish(host, &bundle).await {
            Ok(confirmed) => confirmed,
            Err(error) => {
                return OfflineProjectOutcome::Refused {
                    project: project.selected.project.clone(),
                    stage: OfflineStage::Publication,
                    reason: error.to_string(),
                    #[cfg(debug_assertions)]
                    diagnostic: error.diagnostic(),
                };
            }
        };
        let import_id = confirmed.import_id();
        let head = confirmed.head();
        let archive = match archive_confirmed_source(host, &bundle, confirmed) {
            Ok(archive) => archive,
            Err(error) => return refused(OfflineStage::Archive, error.to_string()),
        };
        OfflineProjectOutcome::ReadyToClaim {
            project: project.selected.project.clone(),
            bundle: bundle.directory().to_owned(),
            claim_request: project.claim_request.clone(),
            canary_request: project.canary_request.clone(),
            import_id,
            head,
            archive,
        }
    }

    async fn prepare_or_reopen(
        &self,
        host: &mut OfflineHost,
        project: &PlannedProject,
    ) -> Result<PreparedProjectBundle, ProjectBundleError> {
        let path = self.planned_bundle_path(project)?;
        if !path.try_exists()? {
            return PreparedProjectBundle::prepare(
                host,
                project.selected.clone(),
                self.preparation(project),
                path.parent().ok_or(ProjectBundleError::InvalidArtifact)?,
            )
            .await;
        }
        self.reopen_planned_bundle(project, host.collaboration_directory())
    }
}
