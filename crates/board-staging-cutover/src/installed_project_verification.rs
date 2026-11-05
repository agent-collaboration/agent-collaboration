//! Compare actual installed files under the offline Host authority with the sealed source.
use crate::offline_run_plan::PlannedProject;
use crate::{
    NativeProjectSnapshot, NativeSourcePreflight, OfflineHost, OfflineRunPlan, SelectedWriterFile,
};
use fleet_sync_protocol::Epoch;
use message_board::ProjectId;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum InstalledVerificationError {
    #[error("the sealed cutover run plan is invalid")]
    Plan(#[from] crate::ProjectBundleError),
    #[error("cannot acquire the declared Host for installed-content verification")]
    Host(#[from] crate::OfflineHostError),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledVerificationReport {
    run_id: uuid::Uuid,
    projects: Vec<InstalledProjectComparison>,
}

#[derive(Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum InstalledProjectComparison {
    Verified {
        project: ProjectId,
        installed: Box<NativeSourcePreflight>,
    },
    VerifiedRecovery {
        project: ProjectId,
        installed: Box<NativeSourcePreflight>,
        recovery: crate::intent_recovery_verification::VerifiedIntentRecovery,
    },
    Changed {
        project: ProjectId,
        source: Box<NativeSourcePreflight>,
        installed: Box<NativeSourcePreflight>,
        reason: String,
    },
    Unavailable {
        project: ProjectId,
        stage: VerificationStage,
        reason: String,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum VerificationStage {
    Bundle,
    Backup,
    InstalledSource,
    Close,
}

impl InstalledVerificationReport {
    pub fn all_verified(&self) -> bool {
        self.projects.iter().all(|project| {
            matches!(
                project,
                InstalledProjectComparison::Verified { .. }
                    | InstalledProjectComparison::VerifiedRecovery { .. }
            )
        })
    }
}

impl OfflineRunPlan {
    pub async fn verify_installed_projects(
        &self,
        home: &Path,
    ) -> Result<InstalledVerificationReport, InstalledVerificationError> {
        self.validate()?;
        let mut host =
            OfflineHost::acquire(&self.router_root, &self.service_id, self.peer_id, home)?;
        let mut projects = Vec::new();
        for project in &self.projects {
            projects.push(self.compare_installed_project(&mut host, project).await);
        }
        Ok(InstalledVerificationReport {
            run_id: self.run_id,
            projects,
        })
    }

    async fn compare_installed_project(
        &self,
        host: &mut OfflineHost,
        project: &PlannedProject,
    ) -> InstalledProjectComparison {
        let failed = |stage, reason: String| InstalledProjectComparison::Unavailable {
            project: project.selected.project.clone(),
            stage,
            reason,
        };
        let bundle = match self.reopen_planned_bundle(project, host.collaboration_directory()) {
            Ok(bundle) => bundle,
            Err(error) => return failed(VerificationStage::Bundle, error.to_string()),
        };
        match bundle
            .manifest()
            .source_fingerprint
            .matches(&bundle.manifest().backup_directory)
        {
            Ok(true) => {}
            Ok(false) => {
                return failed(
                    VerificationStage::Backup,
                    "the original source backup changed".to_owned(),
                );
            }
            Err(_) => {
                return failed(
                    VerificationStage::Backup,
                    "the original source backup is unavailable".to_owned(),
                );
            }
        }
        let epoch = match Epoch::new(2) {
            Ok(epoch) => epoch,
            Err(_) => {
                return failed(
                    VerificationStage::InstalledSource,
                    "target epoch is unavailable".to_owned(),
                );
            }
        };
        let selected = SelectedWriterFile {
            project: project.selected.project.clone(),
            peer: host.peer(),
            file_epoch: epoch,
        };
        let machine_store = host.collaboration_directory().join("interaction.sqlite");
        let mut installed = match NativeProjectSnapshot::open(host, selected).await {
            Ok(installed) => installed,
            Err(error) => return failed(VerificationStage::InstalledSource, error.to_string()),
        };
        let source = &bundle.manifest().source;
        let actual = installed.preflight().clone();
        let exact = installed.verify_replicated_tables(&source.manifest).is_ok()
            && actual.application_epoch == source.application_epoch
            && actual.high_water == source.high_water
            && actual.line_two == source.line_two
            && actual.latest_record_at == source.latest_record_at
            && actual.project_scope == source.project_scope;
        let recovery = if exact {
            None
        } else {
            let service = String::from(self.service_id.clone()).try_into();
            Some(match service {
                Ok(service) => {
                    crate::intent_recovery_verification::verify_intent_recovery(
                        &bundle,
                        &mut installed,
                        &service,
                        self.peer_id,
                        &machine_store,
                    )
                    .await
                }
                Err(_) => Err(crate::intent_recovery_rows::RecoveryError::Invalid(
                    "declared service",
                )),
            })
        };
        if let Err(error) = installed.close().await {
            return failed(VerificationStage::Close, error.to_string());
        }
        if exact {
            InstalledProjectComparison::Verified {
                project: project.selected.project.clone(),
                installed: Box::new(actual),
            }
        } else if let Some(Ok(recovery)) = recovery {
            InstalledProjectComparison::VerifiedRecovery {
                project: project.selected.project.clone(),
                installed: Box::new(actual),
                recovery,
            }
        } else {
            let reason = match recovery {
                Some(Err(error)) => error.to_string(),
                _ => "content differs from the source".to_owned(),
            };
            InstalledProjectComparison::Changed {
                project: project.selected.project.clone(),
                source: Box::new(source.clone()),
                installed: Box::new(actual),
                reason,
            }
        }
    }
}
