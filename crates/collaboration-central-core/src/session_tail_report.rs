//! The session keeps the verified historical writer separate from Fleet storage.
use crate::registry_contract::PendingContinuation;
use crate::registry_storage::session_dispatch;
use crate::session_reply::SessionReplyKind;
use crate::session_rules::SessionRecord;
use crate::*;
use collaboration_replication_protocol::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PendingTailReport {
    pub connection: ConnectionId,
    pub frame_id: FrameId,
    pub report: TailReportRow,
}

pub(crate) fn begin(
    tx: &mut dyn SqlTx,
    session: &SessionRecord,
    pending: PendingTailReport,
    ids: &mut IdBatch,
) -> Result<Vec<SessionEffect>, CentralError> {
    Ok(vec![session_dispatch::<RegistryKind>(
        tx,
        ids,
        (),
        RegistryCall::VerifyTail {
            project: pending.report.project.clone(),
            epoch: pending.report.epoch,
            installation: session.installation.clone(),
        },
        PendingContinuation::TailVerification(pending),
    )?])
}

pub(crate) fn verified(
    tx: &mut dyn SqlTx,
    pending: PendingTailReport,
    result: TailVerificationResult,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<SessionEffect>, CentralError> {
    if !crate::session_rules::current(tx, &pending.connection, now)? {
        return Ok(Vec::new());
    }
    match result {
        TailVerificationResult::Refused(refusal) => Ok(vec![SessionEffect::Send {
            to: pending.connection,
            frame: CentralFrame::Answer {
                frame_id: pending.frame_id,
                body: SessionAnswer::Refused(refusal.into()),
            },
        }]),
        TailVerificationResult::Verified { current_holder } => {
            let report = pending.report;
            Ok(vec![session_dispatch::<FleetKind>(
                tx,
                ids,
                (),
                FleetCall::StoreTail {
                    project: report.project,
                    epoch: report.epoch,
                    request_ids: report.request_ids,
                    digest: report.digest,
                    current_holder,
                },
                PendingContinuation::FrameAnswer {
                    connection: pending.connection,
                    frame_id: pending.frame_id,
                    expected: SessionReplyKind::TailReport,
                },
            )?])
        }
    }
}
