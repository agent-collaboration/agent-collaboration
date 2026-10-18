use crate::CentralError;
use crate::*;
use collaboration_replication_protocol::SessionAnswer;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SessionReplyKind {
    Claim,
    Piece {
        project: collaboration_replication_protocol::ProjectId,
    },
    Renew,
    Release,
    FirstLease,
    TailReport,
    Fleet,
    Read,
    Snapshot {
        project: collaboration_replication_protocol::ProjectId,
    },
    ArchiveSeen,
}
impl SessionReplyKind {
    pub(crate) fn of_call(call: &crate::session_rules::SessionRoute) -> Result<Self, CentralError> {
        Ok(match call {
            crate::session_rules::SessionRoute::Registry(RegistryCall::Claim { .. }) => Self::Claim,
            crate::session_rules::SessionRoute::Copy {
                project,
                call: CopyCall::ApplyPiece { .. },
            } => Self::Piece {
                project: project.clone(),
            },
            crate::session_rules::SessionRoute::Registry(RegistryCall::Renew { .. }) => Self::Renew,
            crate::session_rules::SessionRoute::Registry(RegistryCall::Release { .. }) => {
                Self::Release
            }
            crate::session_rules::SessionRoute::Registry(RegistryCall::ReserveFirstEpoch {
                ..
            }) => Self::FirstLease,
            crate::session_rules::SessionRoute::Fleet(FleetCall::StoreTail { .. }) => {
                Self::TailReport
            }
            crate::session_rules::SessionRoute::Fleet(FleetCall::Fleet { .. }) => Self::Fleet,
            crate::session_rules::SessionRoute::Fleet(FleetCall::Read { .. }) => Self::Read,
            crate::session_rules::SessionRoute::Copy {
                project,
                call: CopyCall::SnapshotPage { .. },
            } => Self::Snapshot {
                project: project.clone(),
            },
            crate::session_rules::SessionRoute::Fleet(FleetCall::ArchiveSeen { .. }) => {
                Self::ArchiveSeen
            }
            _ => return Err(CentralError::ReplyContract),
        })
    }
    pub(crate) fn answer(self, result: SessionReply) -> Result<SessionAnswer, CentralError> {
        match (self, result) {
            (Self::Claim, SessionReply::Registry(RegistryResult::Claim(answer))) => Ok(answer),
            (
                Self::Piece { project } | Self::Snapshot { project },
                SessionReply::Copy(CopyResult::Unavailable(reason)),
            ) => Ok(SessionAnswer::Refused(match reason {
                collaboration_replication_protocol::CopyUnavailableReason::StorageFull => {
                    collaboration_replication_protocol::SessionRefusal::CentralCopyFull
                }
                reason => collaboration_replication_protocol::SessionRefusal::CopyUnavailable {
                    project,
                    reason,
                },
            })),
            (Self::Piece { .. }, SessionReply::Copy(CopyResult::ApplyPiece(a)))
            | (Self::Renew, SessionReply::Registry(RegistryResult::Renew(a)))
            | (Self::Release, SessionReply::Registry(RegistryResult::Release(a)))
            | (Self::FirstLease, SessionReply::Registry(RegistryResult::FirstLease(a)))
            | (Self::TailReport, SessionReply::Fleet(FleetResult::TailReport(a)))
            | (Self::Fleet, SessionReply::Fleet(FleetResult::Fleet(a)))
            | (Self::Read, SessionReply::Fleet(FleetResult::Read(a)))
            | (Self::Snapshot { .. }, SessionReply::Copy(CopyResult::SnapshotPage(a)))
            | (Self::ArchiveSeen, SessionReply::Fleet(FleetResult::ArchiveSeen(a))) => Ok(a),
            _ => Err(CentralError::ReplyContract),
        }
    }
}
