//! Fleet supplies a pure read; Registry compares the authenticated carrier mapping.
use super::{CoreHost, PlatformIo, SharedTaskCaller, no_route};
use crate::fleet_creation_calls::ReadProjectCreationDecision;
use durable_steps::{CallFailure, CallId};
use fleet_sync_protocol::{GateAnswer, GateReason, GateReasonCode, RegistrationAdmission};
use message_board::{ProjectId, StoredDecision};

impl<Io: PlatformIo> CoreHost<Io> {
    pub(super) async fn admit_registration(
        &self,
        request: RegistrationAdmission,
        identity: CallId,
    ) -> Result<GateAnswer, CallFailure> {
        let SharedTaskCaller::Registry { peer_directory, .. } = &self.caller else {
            return Err(no_route());
        };
        let project = ProjectId::try_from(request.database.to_string()).map_err(|_| no_route())?;
        let decision = self
            .registry_fleet_call(ReadProjectCreationDecision, project, identity)
            .await?
            .ok_or_else(no_route)?;
        match decision.decision {
            StoredDecision::Accepted { .. } => {
                let installation = peer_directory(request.writer).await?;
                if installation == decision.carrier_installation {
                    Ok(GateAnswer::Admitted)
                } else {
                    Ok(GateAnswer::Refused {
                        reason: GateReason {
                            code: GateReasonCode::new("creatorMismatch")
                                .map_err(|_| CallFailure::AnswerLost)?,
                            detail: "The project was created by another installation.".to_owned(),
                        },
                    })
                }
            }
            StoredDecision::Rejected { reasons } => {
                let first = reasons.first().ok_or(CallFailure::AnswerLost)?;
                Ok(GateAnswer::Refused {
                    reason: crate::registration_refusal::render(first)?,
                })
            }
        }
    }
}
