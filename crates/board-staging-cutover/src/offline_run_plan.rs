//! Explicit machine and preserved source inventory for one offline helper lifetime.
use crate::offline_host::RootMode;
use crate::{HostInspection, OfflineHost, OfflineHostError};
use crate::{PreparedProjectBundle, ProjectBundleError, ProjectPreparation, SelectedWriterFile};
use collaboration_protocol::UuidIdentity;
use fleet_admission::FleetImportBundle;
use fleet_sync_protocol::PeerId;
use message_board::RequestId;
use serde::{Deserialize, Serialize};
use std::os::unix::fs::MetadataExt;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use url::Url;
use uuid::Uuid;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OfflineRunPlan {
    pub(crate) run_id: Uuid,
    pub(crate) router_root: PathBuf,
    pub(crate) service_id: UuidIdentity,
    pub(crate) peer_id: PeerId,
    #[serde(
        deserialize_with = "deserialize_target",
        serialize_with = "serialize_target"
    )]
    pub(crate) target: Url,
    pub(crate) bundle_parent: PathBuf,
    pub(crate) projects: Vec<PlannedProject>,
    #[serde(skip)]
    pub(crate) admission: RunAdmission,
}

#[derive(Default)]
pub(crate) enum RunAdmission {
    #[default]
    Isolated,
    Production {
        confirmation: message_board::CanonicalHash,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CutoverPlanPreview {
    router_root: PathBuf,
    #[serde(serialize_with = "serialize_target")]
    target: Url,
    production: bool,
    projects: Vec<ProjectPlanPreview>,
    digest: message_board::CanonicalHash,
}

impl CutoverPlanPreview {
    pub fn digest(&self) -> &message_board::CanonicalHash {
        &self.digest
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectPlanPreview {
    project: message_board::ProjectId,
    epoch: fleet_sync_protocol::Epoch,
    bundle: PathBuf,
    backup: PathBuf,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PlannedProject {
    pub(crate) selected: SelectedWriterFile,
    pub(crate) fleet: FleetImportBundle,
    pub(crate) claim_request: RequestId,
    pub(crate) canary_request: RequestId,
}

impl OfflineRunPlan {
    pub fn router_root(&self) -> &Path {
        &self.router_root
    }

    pub fn read(path: &Path) -> Result<Self, ProjectBundleError> {
        let (plan, _): (Self, _) = crate::bundle_files::read_small(path)?;
        plan.validate()?;
        Ok(plan)
    }

    /// Production intent is supplied separately from the preserved run file.
    pub fn with_production_confirmation(
        mut self,
        target: &Url,
        confirmation: message_board::CanonicalHash,
    ) -> Result<Self, OfflineHostError> {
        self.require_target(target)?;
        self.admission = RunAdmission::Production { confirmation };
        Ok(self)
    }

    /// Reads files only: no lock artifact, writable database, credential or HTTP access.
    pub fn dry_run(
        &self,
        home: &Path,
        production_target: Option<&Url>,
    ) -> Result<CutoverPlanPreview, OfflineHostError> {
        self.validate()
            .map_err(|_| OfflineHostError::PlanUnavailable)?;
        let mode = if let Some(target) = production_target {
            self.require_target(target)?;
            RootMode::Production
        } else {
            RootMode::Isolated
        };
        let host = OfflineHost::inspect_mode(
            &self.router_root,
            &self.service_id,
            self.peer_id,
            home,
            mode,
        )?;
        self.preview_for_host(&host, mode)
            .map_err(|_| OfflineHostError::PlanUnavailable)
    }

    pub(crate) fn inspect_host(&self, home: &Path) -> Result<HostInspection, OfflineHostError> {
        let host = OfflineHost::inspect_mode(
            &self.router_root,
            &self.service_id,
            self.peer_id,
            home,
            self.root_mode(),
        )?;
        self.require_confirmation(&host)?;
        Ok(host)
    }

    pub(crate) fn acquire_host(&self, home: &Path) -> Result<OfflineHost, OfflineHostError> {
        // A wrong digest cannot create even an inert coordination file.
        self.inspect_host(home)?;
        let host = OfflineHost::acquire_mode(
            &self.router_root,
            &self.service_id,
            self.peer_id,
            home,
            self.root_mode(),
        )?;
        // The same state must still be current after excluding a running Host.
        let inspected = OfflineHost::inspect_mode(
            host.router_root(),
            &self.service_id,
            self.peer_id,
            home,
            self.root_mode(),
        )?;
        self.require_confirmation(&inspected)?;
        Ok(host)
    }

    fn root_mode(&self) -> RootMode {
        match self.admission {
            RunAdmission::Isolated => RootMode::Isolated,
            RunAdmission::Production { .. } => RootMode::Production,
        }
    }

    fn require_target(&self, target: &Url) -> Result<(), OfflineHostError> {
        if target != &self.target {
            return Err(OfflineHostError::TargetMismatch);
        }
        Ok(())
    }

    fn require_confirmation(&self, host: &HostInspection) -> Result<(), OfflineHostError> {
        if let RunAdmission::Production { confirmation } = &self.admission {
            let current = self
                .preview_for_host(host, RootMode::Production)
                .map_err(|_| OfflineHostError::PlanUnavailable)?;
            if confirmation != current.digest() {
                return Err(OfflineHostError::ConfirmationMismatch);
            }
        }
        Ok(())
    }

    fn preview_for_host(
        &self,
        host: &HostInspection,
        mode: RootMode,
    ) -> Result<CutoverPlanPreview, ProjectBundleError> {
        let root = host.router_root();
        let root_identity = std::fs::metadata(root)?;
        let collaboration = codex_router_host::router_collaboration_directory(root);
        let mut projects = Vec::new();
        let mut fingerprints = Vec::new();
        // Also bind newly added or removed projects outside the selected list.
        fingerprints.push(capture_if_present(&collaboration.join("projects"))?);
        for project in &self.projects {
            let bundle = self.planned_bundle_path(project)?;
            let backup = crate::bundle_files::backup_directory(
                root,
                self.run_id,
                &project.selected.project,
            )?;
            fingerprints.push(capture_if_present(&bundle)?);
            fingerprints.push(capture_if_present(&backup)?);
            projects.push(ProjectPlanPreview {
                project: project.selected.project.clone(),
                epoch: project.selected.file_epoch,
                bundle,
                backup,
            });
        }
        let production = matches!(mode, RootMode::Production);
        let digest = crate::bundle_files::bytes_digest(&serde_json::to_vec(&(
            "cutover-plan-v1",
            self,
            host,
            root_identity.dev(),
            root_identity.ino(),
            production,
            &projects,
            fingerprints,
        ))?)?;
        Ok(CutoverPlanPreview {
            router_root: root.to_owned(),
            target: self.target.clone(),
            production,
            projects,
            digest,
        })
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

fn capture_if_present(
    path: &Path,
) -> Result<Option<crate::SourceDirectoryFingerprint>, ProjectBundleError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => crate::SourceDirectoryFingerprint::capture(path).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
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

fn serialize_target<S: serde::Serializer>(target: &Url, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(target.as_str())
}
