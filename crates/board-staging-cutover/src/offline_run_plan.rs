//! Explicit machine and preserved source inventory for one offline helper lifetime.
use crate::{PreparedProjectBundle, ProjectBundleError, ProjectPreparation, SelectedWriterFile};
use collaboration_protocol::UuidIdentity;
use fleet_admission::FleetImportBundle;
use fleet_sync_protocol::PeerId;
use message_board::RequestId;
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use url::Url;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OfflineRunPlan {
    pub(crate) run_id: Uuid,
    pub(crate) router_root: PathBuf,
    pub(crate) service_id: UuidIdentity,
    pub(crate) peer_id: PeerId,
    #[serde(deserialize_with = "deserialize_target")]
    pub(crate) target: Url,
    pub(crate) bundle_parent: PathBuf,
    pub(crate) projects: Vec<PlannedProject>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PlannedProject {
    pub(crate) selected: SelectedWriterFile,
    pub(crate) fleet: FleetImportBundle,
    pub(crate) claim_request: RequestId,
    pub(crate) canary_request: RequestId,
}

impl OfflineRunPlan {
    pub fn read(path: &Path) -> Result<Self, ProjectBundleError> {
        let (plan, _): (Self, _) = crate::bundle_files::read_small(path)?;
        plan.validate()?;
        Ok(plan)
    }

    pub(crate) fn validate(&self) -> Result<(), ProjectBundleError> {
        if !runtime_primitives::uuid_v7(self.run_id)
            || !self.router_root.is_absolute()
            || !self.bundle_parent.is_absolute()
            || self.projects.is_empty()
        {
            return Err(ProjectBundleError::InvalidPreparation);
        }
        let mut projects = BTreeSet::new();
        let mut requests = BTreeSet::new();
        for project in &self.projects {
            if project.selected.peer != self.peer_id
                || !projects.insert(project.selected.project.clone())
                || !requests.insert(project.claim_request.clone())
                || !requests.insert(project.canary_request.clone())
            {
                return Err(ProjectBundleError::InvalidPreparation);
            }
            crate::prepared_project_bundle::validate_input(&self.preparation(project))?;
        }
        Ok(())
    }

    pub(crate) fn preparation(&self, project: &PlannedProject) -> ProjectPreparation {
        ProjectPreparation {
            run_id: self.run_id,
            target: self.target.clone(),
            fleet: project.fleet.clone(),
            claim_request: project.claim_request.clone(),
            canary_request: project.canary_request.clone(),
        }
    }

    pub(crate) fn planned_bundle_path(
        &self,
        project: &PlannedProject,
    ) -> Result<PathBuf, ProjectBundleError> {
        crate::bundle_files::owned_directory(&self.bundle_parent, true)?;
        Ok(std::fs::canonicalize(&self.bundle_parent)?.join(format!(
            "{}-{}",
            self.run_id,
            project.selected.project.as_str()
        )))
    }

    pub(crate) fn reopen_planned_bundle(
        &self,
        project: &PlannedProject,
        collaboration_directory: &Path,
    ) -> Result<PreparedProjectBundle, ProjectBundleError> {
        let bundle = PreparedProjectBundle::open(&self.planned_bundle_path(project)?)?;
        let manifest = bundle.manifest();
        let selected = &manifest.source.selected;
        if manifest.run_id != self.run_id
            || manifest.target_origin != self.target.origin().ascii_serialization()
            || manifest.fleet != project.fleet
            || manifest.claim_request != project.claim_request
            || manifest.canary_request != project.canary_request
            || selected.project != project.selected.project
            || selected.peer != project.selected.peer
            || selected.file_epoch != project.selected.file_epoch
            || manifest.source_directory
                != collaboration_directory
                    .join("projects")
                    .join(selected.project.as_str())
        {
            return Err(ProjectBundleError::InvalidArtifact);
        }
        Ok(bundle)
    }
}

pub enum OwnerAuthentication {
    OwnerAccess,
    OwnerCredentialFile { path: PathBuf },
}

fn deserialize_target<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Url, D::Error> {
    let value = String::deserialize(deserializer)?;
    Url::parse(&value).map_err(|_| serde::de::Error::custom("invalid central origin"))
}
