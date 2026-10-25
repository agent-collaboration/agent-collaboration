//! Both declared writes and Router intent writes enter the same locked admission gate.
use super::{ProjectAdmission, ProjectAdmissionError};
use message_board::*;
use message_board_storage::ProjectWriteTransaction;

pub(super) enum RequestGate {
    Ready(Box<ReadyRequest>),
    Reply(RequestResult),
}
pub(super) struct ReadyRequest {
    pub write: ProjectWriteTransaction,
    pub epoch: EpochNumber,
    pub high_water: FeedCursor,
}
pub(super) struct RequestGateInput<'request> {
    pub project: &'request ProjectId,
    pub request: &'request RequestId,
    pub hash: &'request CanonicalHash,
}

impl ProjectAdmission {
    pub(super) async fn begin_request(
        &self,
        input: RequestGateInput<'_>,
        origin: ActorOrigin,
    ) -> Result<RequestGate, ProjectAdmissionError> {
        if let Some(reply) = self.rejected_request(&input).await? {
            return Ok(RequestGate::Reply(reply));
        }
        let Some(store) = self.registry.existing_project(input.project).await? else {
            return Ok(RequestGate::Reply(RequestResult::ScopeUnavailable(
                ScopeRefusal::ProjectNotFound {
                    project: input.project.clone(),
                },
            )));
        };
        let mut write = match store.begin_admission(origin).await {
            Ok(write) => write,
            Err(error) => {
                if let Some(reply) = self.rejected_request(&input).await? {
                    return Ok(RequestGate::Reply(reply));
                }
                if let Some(result) = super::write_gate::gate_result(&error)? {
                    return Ok(RequestGate::Reply(result));
                }
                return Err(error.into());
            }
        };
        let epoch = write.admitted_epoch()?;
        let high_water = write.high_water().await?;
        if let Some(Record::Request(ledger)) = write.record(&input.request.clone().into()).await? {
            let result = if &ledger.canonical_hash == input.hash {
                RequestResult::Decided(write.request_decision(&ledger).await?)
            } else {
                RequestResult::ConflictingRequest
            };
            write.rollback().await?;
            return Ok(RequestGate::Reply(result));
        }
        Ok(RequestGate::Ready(Box::new(ReadyRequest {
            write,
            epoch,
            high_water,
        })))
    }

    pub(super) async fn rejected_request(
        &self,
        input: &RequestGateInput<'_>,
    ) -> Result<Option<RequestResult>, ProjectAdmissionError> {
        use super::project_rejection::registration_rejection;
        let reason = if let Some(reader) = self.registry.rejected_project(input.project).await? {
            registration_rejection(reader.registration_outcome())
        } else if let Some(store) = self.registry.existing_project(input.project).await? {
            registration_rejection(&store.registration_outcome().await?)
        } else {
            None
        };
        Ok(reason.map(|reason| {
            RequestResult::ScopeUnavailable(ScopeRefusal::ProjectRejected {
                project: input.project.clone(),
                reason: ProjectRejection::Registration { reason },
            })
        }))
    }
}
