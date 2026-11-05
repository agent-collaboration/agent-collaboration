//! The online canary phase proves one frozen public write and its idempotent readback.
use crate::canary_record::{CanaryFailure, CanaryStage};
use crate::{ClaimRunError, OfflineRunPlan};
use collaboration_client::CollaborationClient;
use message_board::{ProjectId, RequestId};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum CanaryRunError {
    #[error("the declared online Host prerequisites are unavailable")]
    Host(#[from] ClaimRunError),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanaryRunReport {
    run_id: uuid::Uuid,
    projects: Vec<ProjectCanaryOutcome>,
}
#[derive(Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum ProjectCanaryOutcome {
    Verified {
        project: ProjectId,
        request: RequestId,
        #[serde(flatten)]
        proof: crate::canary_record::CanaryProof,
    },
    Unverified {
        project: ProjectId,
        request: RequestId,
        stage: CanaryStage,
        reason: String,
    },
    OutcomeUnknown {
        project: ProjectId,
        request: RequestId,
        stage: CanaryStage,
    },
}

impl CanaryRunReport {
    pub fn all_verified(&self) -> bool {
        self.projects
            .iter()
            .all(|project| matches!(project, ProjectCanaryOutcome::Verified { .. }))
    }
}

impl OfflineRunPlan {
    pub async fn run_canaries(&self, home: &Path) -> Result<CanaryRunReport, CanaryRunError> {
        // Inspection validates the protected root, exact enrollment origin/peer and restart marker.
        // The running Host retains its singleton authority for these ordinary public calls.
        let host = self.inspect_enrolled_host(home)?;
        let directory = codex_router_host::router_collaboration_directory(host.router_root());
        let client = CollaborationClient::connect(
            &directory,
            "board-staging-cutover-canary",
            env!("CARGO_PKG_VERSION"),
        )
        .await
        .map_err(|_| ClaimRunError::ApiUnavailable)?;
        if client.identity().service_id != *host.service_id() {
            return Err(ClaimRunError::ServiceMismatch.into());
        }
        let mut projects = Vec::new();
        for project in &self.projects {
            let checked = async {
                let bundle = self
                    .reopen_planned_bundle(project, &directory)
                    .map_err(|_| {
                        CanaryFailure::invalid(
                            CanaryStage::Bundle,
                            "the frozen run bundle is unavailable or differs",
                        )
                    })?;
                if !bundle
                    .manifest()
                    .source_fingerprint
                    .matches(&bundle.manifest().backup_directory)
                    .map_err(|_| {
                        CanaryFailure::invalid(
                            CanaryStage::Backup,
                            "the original backup is unavailable",
                        )
                    })?
                {
                    return Err(CanaryFailure::invalid(
                        CanaryStage::Backup,
                        "the original backup changed",
                    ));
                }
                crate::canary_record::run_canary(&client, &bundle).await
            }
            .await;
            // Keep the original identity even after a lost write answer; reruns never mint a canary.
            let request = project.canary_request.clone();
            let project = project.selected.project.clone();
            projects.push(match checked {
                Ok(proof) => ProjectCanaryOutcome::Verified {
                    project,
                    request,
                    proof,
                },
                Err(CanaryFailure::Invalid { stage, reason }) => ProjectCanaryOutcome::Unverified {
                    project,
                    request,
                    stage,
                    reason: reason.to_owned(),
                },
                Err(CanaryFailure::Unknown { stage }) => ProjectCanaryOutcome::OutcomeUnknown {
                    project,
                    request,
                    stage,
                },
            });
        }
        Ok(CanaryRunReport {
            run_id: self.run_id,
            projects,
        })
    }
}
