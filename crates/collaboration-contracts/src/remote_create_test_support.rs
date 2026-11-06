//! Scripted C1 stand-in at the requester/Fleet client port; not a central handler.
use crate::{RemoteCreateFleet, RemoteCreateFleetError, RemoteCreateFleetFuture};
use collaboration_mail_protocol::{RemoteCreateAdmission, RemoteCreateRequest, RemoteCreateStatus};
use message_board::RequestId;
use std::{collections::VecDeque, sync::Mutex};

pub enum RemoteCreateStep {
    Reply(RemoteCreateAdmission),
    Accepted,
    Replayed(RemoteCreateStatus),
    Fail(RemoteCreateFleetError),
}
pub struct ScriptedRemoteCreateFleet {
    steps: Mutex<VecDeque<RemoteCreateStep>>,
    statuses: Mutex<VecDeque<RemoteCreateStatus>>,
    requests: Mutex<Vec<RemoteCreateRequest>>,
    queries: Mutex<Vec<RequestId>>,
}
impl ScriptedRemoteCreateFleet {
    #[must_use]
    pub fn new(steps: Vec<RemoteCreateStep>, statuses: Vec<RemoteCreateStatus>) -> Self {
        Self {
            steps: Mutex::new(steps.into()),
            statuses: Mutex::new(statuses.into()),
            requests: Mutex::new(Vec::new()),
            queries: Mutex::new(Vec::new()),
        }
    }
    pub fn requests(&self) -> Result<Vec<RemoteCreateRequest>, RemoteCreateFleetError> {
        self.requests
            .lock()
            .map(|requests| requests.clone())
            .map_err(|_| RemoteCreateFleetError::InvalidReply)
    }
    pub fn queries(&self) -> Result<Vec<RequestId>, RemoteCreateFleetError> {
        self.queries
            .lock()
            .map(|queries| queries.clone())
            .map_err(|_| RemoteCreateFleetError::InvalidReply)
    }
}
impl RemoteCreateFleet for ScriptedRemoteCreateFleet {
    fn request_remote_create(
        &self,
        request: RemoteCreateRequest,
    ) -> RemoteCreateFleetFuture<'_, RemoteCreateAdmission> {
        Box::pin(async move {
            self.requests
                .lock()
                .map_err(|_| RemoteCreateFleetError::InvalidReply)?
                .push(request.clone());
            let step = self
                .steps
                .lock()
                .map_err(|_| RemoteCreateFleetError::InvalidReply)?
                .pop_front()
                .ok_or(RemoteCreateFleetError::InvalidReply)?;
            match step {
                RemoteCreateStep::Reply(answer) => Ok(answer),
                RemoteCreateStep::Accepted => Ok(RemoteCreateAdmission::Accepted {
                    request_id: request.request_id,
                }),
                RemoteCreateStep::Replayed(status) => Ok(RemoteCreateAdmission::Replayed {
                    request_id: request.request_id,
                    status,
                }),
                RemoteCreateStep::Fail(error) => Err(error),
            }
        })
    }
    fn remote_create_status(
        &self,
        request: RequestId,
    ) -> RemoteCreateFleetFuture<'_, RemoteCreateStatus> {
        Box::pin(async move {
            self.queries
                .lock()
                .map_err(|_| RemoteCreateFleetError::InvalidReply)?
                .push(request);
            self.statuses
                .lock()
                .map_err(|_| RemoteCreateFleetError::InvalidReply)?
                .pop_front()
                .ok_or(RemoteCreateFleetError::InvalidReply)
        })
    }
}
