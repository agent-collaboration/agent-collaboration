//! Remote creates bypass directory persistence and use the Host's existing session.
use super::FleetClient;
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerRequest, BoardPayload, FleetRequestEnvelope, RemoteCreateCall,
};
use collaboration_contracts::{RemoteCreateFleet, RemoteCreateFleetError, RemoteCreateFleetFuture};
use collaboration_mail_protocol::{RemoteCreateAdmission, RemoteCreateRequest, RemoteCreateStatus};
use fleet_sync_protocol::{OperationAnswer, OperationRequest};
use message_board::RequestId;

impl RemoteCreateFleet for FleetClient {
    fn request_remote_create(
        &self,
        request: RemoteCreateRequest,
    ) -> RemoteCreateFleetFuture<'_, RemoteCreateAdmission> {
        Box::pin(async move {
            let id = request.request_id.clone();
            let admission: RemoteCreateAdmission = self
                .remote_create_call(RemoteCreateCall::Create(Box::new(request)))
                .await?
                .decode()
                .map_err(|_| RemoteCreateFleetError::InvalidReply)?;
            if matches!(&admission, RemoteCreateAdmission::Accepted { request_id } | RemoteCreateAdmission::Replayed { request_id, .. } if request_id != &id)
            {
                return Err(RemoteCreateFleetError::InvalidReply);
            }
            Ok(admission)
        })
    }
    fn remote_create_status(
        &self,
        request: RequestId,
    ) -> RemoteCreateFleetFuture<'_, RemoteCreateStatus> {
        Box::pin(async move {
            self.remote_create_call(RemoteCreateCall::Status {
                request_id: request,
            })
            .await?
            .decode()
            .map_err(|_| RemoteCreateFleetError::InvalidReply)
        })
    }
}

impl FleetClient {
    async fn remote_create_call(
        &self,
        call: RemoteCreateCall,
    ) -> Result<BoardPayload, RemoteCreateFleetError> {
        let session = self
            .session
            .as_ref()
            .ok_or(RemoteCreateFleetError::NotSent)?;
        let request_id = call.request_id().clone();
        let request = BoardConsumerRequest::Fleet(FleetRequestEnvelope {
            request_id: request_id.clone(),
            operation: BoardPayload::encode(&call)
                .map_err(|_| RemoteCreateFleetError::InvalidRequest)?,
        });
        let answer = session
            .request(OperationRequest::Consumer(request))
            .await
            .map_err(|failure| match failure {
                durable_steps::CallFailure::NotSent { .. } => RemoteCreateFleetError::NotSent,
                durable_steps::CallFailure::AnswerLost => RemoteCreateFleetError::AnswerLost,
                durable_steps::CallFailure::Retryable { .. } => RemoteCreateFleetError::Retryable,
            })?;
        match answer {
            OperationAnswer::Consumer(BoardConsumerAnswer::Fleet {
                request_id: answered,
                decision,
            }) if answered == request_id => Ok(decision),
            _ => Err(RemoteCreateFleetError::InvalidReply),
        }
    }
}
