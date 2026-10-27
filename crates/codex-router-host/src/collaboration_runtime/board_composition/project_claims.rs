//! The holder's claim uses the machine session and publishes only a verified, renewed seed.
use super::project_writers::ProjectWriters;
use board_sync_protocol::{ProjectClaimRefusal, ProjectClaimReply, ProjectClaimRequest};
use collaboration_contracts::ProjectClaim;
use collaboration_contracts::ProjectClaimError;
use collaboration_contracts::ProjectClaimFuture;
use fleet_sync_protocol::{
    DatabaseId, HolderChangeAnswer, HolderChangeRefusal, LeaseAnswer, LeaseRequest,
    OperationAnswer, OperationRequest,
};
use std::io;

impl ProjectClaim for ProjectWriters {
    fn claim<'runtime>(
        &'runtime self,
        request: &'runtime ProjectClaimRequest,
    ) -> ProjectClaimFuture<'runtime> {
        Box::pin(async move {
            self.claim_project(request)
                .await
                .map_err(|error| ProjectClaimError::Runtime {
                    message: error.to_string(),
                })
        })
    }
}
impl ProjectWriters {
    async fn claim_project(&self, request: &ProjectClaimRequest) -> io::Result<ProjectClaimReply> {
        if *self.stopped.borrow() || !*self.welcomed.borrow() {
            return Err(io::Error::other("machine session is not ready"));
        }
        let project = &request.project_id;
        let attempt =
            fleet_sync_protocol::RequestId::try_from(request.request_id.as_str().to_owned())
                .map_err(io::Error::other)?;
        let database =
            DatabaseId::try_from(project.as_str().to_owned()).map_err(io::Error::other)?;
        let answer = self
            .requests
            .request(OperationRequest::Lease(LeaseRequest::Claim {
                attempt,
                database,
            }))
            .await
            .map_err(|failure| io::Error::other(format!("claim session request: {failure:?}")))?;
        let OperationAnswer::Lease(LeaseAnswer::Claim(answer)) = answer else {
            return Err(io::Error::other("claim returned another operation answer"));
        };
        let (epoch, expires_at, start) = match answer {
            HolderChangeAnswer::Granted {
                epoch,
                expires_at,
                start,
            } => (epoch, expires_at, start),
            HolderChangeAnswer::Pending { .. } => {
                return Ok(ProjectClaimReply::Pending {
                    attempt: request.request_id.clone(),
                });
            }
            HolderChangeAnswer::Refused(reason) => {
                return Ok(ProjectClaimReply::Refused(claim_refusal(reason)?));
            }
            other => return Err(io::Error::other(format!("claim did not grant: {other:?}"))),
        };
        let grant = self
            .confirmed_snapshot_grant(project, epoch)
            .await?
            .filter(|grant| grant.start == start)
            .ok_or_else(|| {
                io::Error::other("claim receipt no longer names current holder authority")
            })?;
        self.prepare_snapshot_install(project, grant).await?;
        match self.finish_snapshot_install(project, epoch, start).await? {
            super::project_installation::SnapshotAdmission::Ready => {
                Ok(ProjectClaimReply::Claimed {
                    epoch,
                    expires_at,
                    start,
                })
            }
            super::project_installation::SnapshotAdmission::NoLongerOwned => Err(io::Error::other(
                "claim install is no longer the current project owner",
            )),
            super::project_installation::SnapshotAdmission::Stopped => Err(io::Error::other(
                "machine session stopped during installation",
            )),
            super::project_installation::SnapshotAdmission::Pending => {
                Ok(ProjectClaimReply::Pending {
                    attempt: request.request_id.clone(),
                })
            }
        }
    }
}

fn claim_refusal(reason: HolderChangeRefusal) -> io::Result<ProjectClaimRefusal> {
    Ok(match reason {
        HolderChangeRefusal::AlreadyHeld { holder } => ProjectClaimRefusal::AlreadyHeld { holder },
        HolderChangeRefusal::TransferInProgress { transfer } => {
            ProjectClaimRefusal::TransferInProgress { transfer }
        }
        HolderChangeRefusal::SchemaTooNew { store, supported } => {
            ProjectClaimRefusal::SchemaTooNew { store, supported }
        }
        HolderChangeRefusal::NotFound => ProjectClaimRefusal::NotFound,
        HolderChangeRefusal::Retired => ProjectClaimRefusal::Retired,
        HolderChangeRefusal::GateRefused { reason } if reason.code.as_str() == "board_archived" => {
            ProjectClaimRefusal::Archived
        }
        other => {
            return Err(io::Error::other(format!(
                "unexpected claim refusal: {other:?}"
            )));
        }
    })
}

#[cfg(test)]
#[allow(clippy::panic_in_result_fn)]
mod refusal_tests {
    use super::*;
    type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
    #[test]
    fn claim_refusals_preserve_frozen_native_details_and_name_only_the_archive_gate() -> TestResult
    {
        let id = "019f0000-0000-7000-8000-000000009710";
        let holder = fleet_sync_protocol::PeerId::try_from(id.to_owned())?;
        let transfer = fleet_sync_protocol::TransferId::try_from(id.to_owned())?;
        let schema =
            fleet_sync_protocol::SchemaIdentity::new(7, "expected-native-fingerprint".into())?;
        for (input, expected) in [
            (
                HolderChangeRefusal::AlreadyHeld { holder },
                ProjectClaimRefusal::AlreadyHeld { holder },
            ),
            (
                HolderChangeRefusal::TransferInProgress { transfer },
                ProjectClaimRefusal::TransferInProgress { transfer },
            ),
            (
                HolderChangeRefusal::SchemaTooNew {
                    store: schema.clone(),
                    supported: None,
                },
                ProjectClaimRefusal::SchemaTooNew {
                    store: schema,
                    supported: None,
                },
            ),
            (HolderChangeRefusal::NotFound, ProjectClaimRefusal::NotFound),
            (HolderChangeRefusal::Retired, ProjectClaimRefusal::Retired),
            (
                HolderChangeRefusal::GateRefused {
                    reason: fleet_sync_protocol::GateReason {
                        code: fleet_sync_protocol::GateReasonCode::new("board_archived")?,
                        detail: "actual archive gate".into(),
                    },
                },
                ProjectClaimRefusal::Archived,
            ),
        ] {
            assert_eq!(claim_refusal(input)?, expected);
        }
        assert!(claim_refusal(HolderChangeRefusal::Unavailable).is_err());
        assert!(
            claim_refusal(HolderChangeRefusal::GateRefused {
                reason: fleet_sync_protocol::GateReason {
                    code: fleet_sync_protocol::GateReasonCode::new("unrelated_gate")?,
                    detail: "not archived".into()
                }
            })
            .is_err()
        );
        Ok(())
    }
}
