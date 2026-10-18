//! Both declared writes and Router intent writes enter the same locked admission gate.
use super::{ProjectAdmission, ProjectAdmissionError, granted_epoch};
use message_board::*;
use message_board_storage::{ProjectWriteTransaction, StoreSchemaState};

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
    pub observed_epoch: Option<EpochNumber>,
    pub hash: &'request CanonicalHash,
}

impl ProjectAdmission {
    pub(super) async fn begin_request(
        &self,
        input: RequestGateInput<'_>,
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
        let guard = store.lock_writer().await;
        // The file may have been orphaned while this request waited on its writer.
        if let Some(reply) = self.rejected_request(&input).await? {
            return Ok(RequestGate::Reply(reply));
        }
        if let StoreSchemaState::ReadOnly { refusal } = guard.schema_state() {
            if let Err(reply) = granted_epoch(self.leases.grant(input.project).await?) {
                return Ok(RequestGate::Reply(reply));
            }
            return Ok(RequestGate::Reply(RequestResult::SchemaNotReady(
                refusal.clone(),
            )));
        }
        let mut write = guard.begin().await?;
        let epoch = match granted_epoch(self.leases.grant(input.project).await?) {
            Ok(epoch) => epoch,
            Err(reply) => {
                write.rollback().await?;
                return Ok(RequestGate::Reply(reply));
            }
        };
        let high_water = write.high_water().await?;
        if high_water.epoch != epoch {
            write.rollback().await?;
            return Ok(RequestGate::Reply(RequestResult::NotWriter {
                current: Some(high_water.epoch),
            }));
        }
        if let Some(Record::Request(ledger)) = write.record(&input.request.clone().into()).await? {
            write.rollback().await?;
            return Ok(RequestGate::Reply(
                if &ledger.canonical_hash == input.hash {
                    RequestResult::Decided(ledger.decision)
                } else {
                    RequestResult::ConflictingRequest
                },
            ));
        }
        if input
            .observed_epoch
            .is_some_and(|observed| observed < epoch)
        {
            write.rollback().await?;
            return Ok(RequestGate::Reply(RequestResult::Unresolved {
                takeover: epoch,
            }));
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
        let Some(reader) = self.registry.rejected_project(input.project).await? else {
            return Ok(None);
        };
        if let Some(ledger) = reader.ledger(input.request).await? {
            return Ok(Some(if &ledger.canonical_hash == input.hash {
                RequestResult::Decided(ledger.decision)
            } else {
                RequestResult::ConflictingRequest
            }));
        }
        let reason = reader.rejection_reason().await?.ok_or_else(|| {
            ProjectAdmissionError::InvalidRecord("orphaned project has no rejection cause".into())
        })?;
        Ok(Some(RequestResult::ScopeUnavailable(
            ScopeRefusal::ProjectRejected {
                project: input.project.clone(),
                reason: Box::new(reason),
            },
        )))
    }
}
