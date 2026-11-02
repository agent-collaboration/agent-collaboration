//! Automation calls borrow the Host's authenticated machine session.
use crate::{CentralAutomationError, CentralAutomationTransport};
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, BoardConsumerRequest, BoardPayload,
};
use collaboration_protocol::{AutomationAnswer, AutomationCall};
use durable_steps::CallFailure;
use fleet_sync_protocol::{OperationAnswer, OperationRequest};
use fleet_sync_tokio_runner::SessionRequests;
use std::{future::Future, pin::Pin};

pub struct SessionAutomationTransport {
    requests: SessionRequests<BoardConsumerProtocol>,
}

impl SessionAutomationTransport {
    pub fn new(requests: SessionRequests<BoardConsumerProtocol>) -> Self {
        Self { requests }
    }
}

impl CentralAutomationTransport for SessionAutomationTransport {
    fn call(
        &self,
        request: AutomationCall,
    ) -> Pin<Box<dyn Future<Output = Result<AutomationAnswer, CentralAutomationError>> + Send + '_>>
    {
        Box::pin(async move {
            let payload =
                BoardPayload::encode(&request).map_err(|_| CentralAutomationError::InvalidWork)?;
            let answer = self
                .requests
                .request(OperationRequest::Consumer(
                    BoardConsumerRequest::Automation(payload),
                ))
                .await
                .map_err(|failure| match failure {
                    CallFailure::AnswerLost => CentralAutomationError::OutcomeUnknown,
                    _ => CentralAutomationError::Unavailable,
                })?;
            match answer {
                OperationAnswer::Consumer(BoardConsumerAnswer::Automation(payload)) => payload
                    .decode()
                    .map_err(|_| CentralAutomationError::InvalidReply),
                _ => Err(CentralAutomationError::InvalidReply),
            }
        })
    }
}
