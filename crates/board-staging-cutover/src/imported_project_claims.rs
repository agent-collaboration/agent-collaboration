//! Resume the sealed run through ordinary public Claim after publication and archive.
use crate::offline_run_plan::PlannedProject;
use crate::{
    OfflineHost, OfflineRunPlan, OwnerAuthentication, PreparedProjectBundle, StagingPublisher,
};
use board_sync_protocol::{ProjectClaimRefusal, ProjectClaimReply, ProjectClaimRequest};
use collaboration_client::{BoardClientError, CollaborationClient};
use fleet_sync_protocol::{Epoch, ReplicationPosition};
use message_board::{BoardFailureKind, ProjectId, ProtocolVersion, RequestId};
use serde::Serialize;
use std::{path::Path, time::Duration};

#[derive(Debug, thiserror::Error)]
pub enum ClaimRunError {
    #[error("the sealed cutover run plan is invalid")]
    Plan(#[from] crate::ProjectBundleError),
    #[error("the declared Host identity is unavailable")]
    Host(#[from] crate::OfflineHostError),
    #[error("the declared Host enrollment is unavailable")]
    Enrollment,
    #[error("the active enrollment differs from the declared central target")]
    EnrollmentMismatch,
    #[error("the declared Host must apply its enrollment at restart before Claim")]
    RestartRequired,
    #[error("owner publication access is unavailable")]
    Owner(#[from] crate::PublicationError),
    #[error("the declared Host public API is unavailable")]
    ApiUnavailable,
    #[error("the connected Host differs from the declared service")]
    ServiceMismatch,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimRunReport {
    run_id: uuid::Uuid,
    projects: Vec<ClaimProjectOutcome>,
}

#[derive(Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum ClaimProjectOutcome {
    Claimed {
        project: ProjectId,
        request: RequestId,
        epoch: Epoch,
        start: ReplicationPosition,
    },
    Refused {
        project: ProjectId,
        request: RequestId,
        reason: ProjectClaimRefusal,
    },
    PrerequisiteFailed {
        project: ProjectId,
        request: RequestId,
        stage: ClaimPrerequisite,
        reason: String,
    },
    AuthorityChanged {
        project: ProjectId,
        request: RequestId,
        epoch: Epoch,
        start: ReplicationPosition,
    },
    OutcomeUnknown {
        project: ProjectId,
        request: RequestId,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum ClaimPrerequisite {
    Bundle,
    Backup,
    Publication,
}

impl ClaimRunReport {
    pub fn all_claimed(&self) -> bool {
        self.projects
            .iter()
            .all(|project| matches!(project, ClaimProjectOutcome::Claimed { .. }))
    }
}

impl OfflineRunPlan {
    pub(crate) fn inspect_enrolled_host(
        &self,
        home: &Path,
    ) -> Result<crate::HostInspection, ClaimRunError> {
        self.validate()?;
        let host = OfflineHost::inspect(&self.router_root, &self.service_id, self.peer_id, home)?;
        let root = host.router_root();
        let enrollment = codex_router_host::read_fleet_enrollment_document(
            &codex_router_host::router_fleet_enrollment_file(root),
            root,
        )
        .map_err(|_| ClaimRunError::Enrollment)?;
        let endpoint =
            url::Url::parse(&enrollment.endpoint).map_err(|_| ClaimRunError::Enrollment)?;
        if enrollment.peer != host.peer() || endpoint.origin() != self.target.origin() {
            return Err(ClaimRunError::EnrollmentMismatch);
        }
        if codex_router_host::fleet_enrollment_restart_required(root)
            .map_err(|_| ClaimRunError::Enrollment)?
        {
            return Err(ClaimRunError::RestartRequired);
        }
        Ok(host)
    }

    pub async fn claim_imported_projects(
        &self,
        home: &Path,
        authentication: OwnerAuthentication,
    ) -> Result<ClaimRunReport, ClaimRunError> {
        let host = self.inspect_enrolled_host(home)?;
        let root = host.router_root();
        let publisher = match authentication {
            OwnerAuthentication::OwnerAccess => {
                StagingPublisher::from_owner_access(self.target.clone()).await?
            }
            OwnerAuthentication::OwnerCredentialFile { path } => {
                StagingPublisher::from_owner_file(self.target.clone(), &path)?
            }
        };
        let directory = codex_router_host::router_collaboration_directory(root);
        let client = CollaborationClient::connect(
            &directory,
            "board-staging-cutover",
            env!("CARGO_PKG_VERSION"),
        )
        .await
        .map_err(|_| ClaimRunError::ApiUnavailable)?;
        if client.identity().service_id != *host.service_id() {
            return Err(ClaimRunError::ServiceMismatch);
        }
        let mut projects = Vec::new();
        for project in &self.projects {
            projects.push(
                self.claim_project(&directory, &publisher, &client, project)
                    .await,
            );
        }
        Ok(ClaimRunReport {
            run_id: self.run_id,
            projects,
        })
    }

    async fn claim_project(
        &self,
        directory: &Path,
        publisher: &StagingPublisher,
        client: &CollaborationClient,
        project: &PlannedProject,
    ) -> ClaimProjectOutcome {
        let failed = |stage, reason: String| ClaimProjectOutcome::PrerequisiteFailed {
            project: project.selected.project.clone(),
            request: project.claim_request.clone(),
            stage,
            reason,
        };
        let bundle = match self.reopen_planned_bundle(project, directory) {
            Ok(bundle) => bundle,
            Err(error) => return failed(ClaimPrerequisite::Bundle, error.to_string()),
        };
        if let Err(error) = require_preserved_backup(&bundle) {
            return failed(ClaimPrerequisite::Backup, error.to_string());
        }
        if let Err(error) = publisher.require_complete_import(&bundle).await {
            return failed(ClaimPrerequisite::Publication, error.to_string());
        }
        claim_frozen_project(client, &bundle).await
    }
}

#[derive(Debug, thiserror::Error)]
enum PreservedBackupError {
    #[error("the original source backup changed")]
    Changed,
    #[error("the original source backup is unavailable")]
    Unavailable,
}

fn require_preserved_backup(bundle: &PreparedProjectBundle) -> Result<(), PreservedBackupError> {
    match bundle
        .manifest()
        .source_fingerprint
        .matches(&bundle.manifest().backup_directory)
    {
        Ok(true) => Ok(()),
        Ok(false) => Err(PreservedBackupError::Changed),
        Err(_) => Err(PreservedBackupError::Unavailable),
    }
}

#[cfg(test)]
#[path = "imported_project_claims_test.rs"]
mod tests;

async fn claim_frozen_project(
    client: &CollaborationClient,
    bundle: &PreparedProjectBundle,
) -> ClaimProjectOutcome {
    let manifest = bundle.manifest();
    let project = manifest.source.selected.project.clone();
    let request = manifest.claim_request.clone();
    let call = ProjectClaimRequest {
        request_id: request.clone(),
        protocol_version: ProtocolVersion::CURRENT,
        project_id: project.clone(),
    };
    let answer = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            match client.project_claim(call.clone()).await {
                Ok(ProjectClaimReply::Pending { attempt }) if attempt == request => {
                    tokio::task::yield_now().await
                }
                Err(BoardClientError::Rejected(error))
                    if error.kind == BoardFailureKind::BoardUnavailable =>
                {
                    tokio::task::yield_now().await
                }
                other => return other,
            }
        }
    })
    .await;
    match answer {
        Ok(Ok(ProjectClaimReply::Claimed { epoch, start, .. })) => {
            if epoch.value() == 2 && start == manifest.header.manifest.position {
                ClaimProjectOutcome::Claimed {
                    project,
                    request,
                    epoch,
                    start,
                }
            } else {
                ClaimProjectOutcome::AuthorityChanged {
                    project,
                    request,
                    epoch,
                    start,
                }
            }
        }
        Ok(Ok(ProjectClaimReply::Refused(reason))) => ClaimProjectOutcome::Refused {
            project,
            request,
            reason,
        },
        Ok(Ok(ProjectClaimReply::Pending { .. })) | Ok(Err(_)) | Err(_) => {
            ClaimProjectOutcome::OutcomeUnknown { project, request }
        }
    }
}
