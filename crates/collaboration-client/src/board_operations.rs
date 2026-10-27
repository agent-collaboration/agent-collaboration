//! Typed board calls over the collaboration API; uncertain writes are never replayed.
use crate::{ClientError, CollaborationClient};
use collaboration_protocol::ThreadSubscriptionWaitFilter;
use message_board::{BoardError, BoardNextAction, Identity, ResourceIdentity, SubscriptionTarget};
#[derive(Debug, thiserror::Error)]
pub enum BoardClientError {
    #[error("{0}")]
    Rejected(Box<BoardError>),
    #[error("{message}")]
    OutcomeUnknown {
        resource: ResourceIdentity,
        message: &'static str,
        next_action: BoardNextAction,
    },
    #[error("wait result was lost after a possible handoff")]
    WaitOutcomeUnknown {
        actor: Box<Identity>,
        filter: ThreadSubscriptionWaitFilter,
        renewals: Vec<collaboration_protocol::SubscriptionRenewalEntry>,
    },
    #[error(transparent)]
    Connection(#[from] ClientError),
}

#[path = "board_fleet_calls.rs"]
mod board_fleet_calls;
#[path = "board_project_calls.rs"]
mod board_project_calls;
#[path = "board_read_calls.rs"]
mod board_read_calls;
#[path = "board_thread_calls.rs"]
mod board_thread_calls;
#[path = "board_view_calls.rs"]
mod board_view_calls;

impl CollaborationClient {
    async fn board_mutation_call<
        TRequest: serde::Serialize,
        TResult: serde::de::DeserializeOwned,
    >(
        &self,
        method: &str,
        request: TRequest,
        resource: ResourceIdentity,
    ) -> Result<TResult, BoardClientError> {
        let params = serde_json::to_value(request)
            .map_err(|_| ClientError::Protocol("invalid board request"))?;
        let result = match self.connection.call(method, params).await {
            Ok(result) => result,
            Err(ClientError::Rejected {
                code: -32050,
                data: Some(data),
            }) => {
                return Err(BoardClientError::Rejected(Box::new(
                    serde_json::from_value(data)
                        .map_err(|_| ClientError::Protocol("invalid board failure"))?,
                )));
            }
            Err(ClientError::Overloaded { message }) => {
                return Err(BoardClientError::Rejected(Box::new(
                    crate::admission_overload::board(message),
                )));
            }
            Err(error @ ClientError::Rejected { .. }) => return Err(error.into()),
            Err(_) => return Err(outcome_unknown(resource)),
        };
        serde_json::from_value(result).map_err(|_| outcome_unknown(resource))
    }
    async fn board_call<TRequest: serde::Serialize, TResult: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        request: TRequest,
    ) -> Result<TResult, BoardClientError> {
        let params = serde_json::to_value(request)
            .map_err(|_| ClientError::Protocol("invalid board request"))?;
        let result = match self.connection.call(method, params).await {
            Ok(result) => result,
            Err(ClientError::Rejected {
                code: -32050,
                data: Some(data),
            }) => {
                return Err(BoardClientError::Rejected(Box::new(
                    serde_json::from_value(data)
                        .map_err(|_| ClientError::Protocol("invalid board failure"))?,
                )));
            }
            Err(ClientError::Overloaded { message }) => {
                return Err(BoardClientError::Rejected(Box::new(
                    crate::admission_overload::board(message),
                )));
            }
            Err(error) => return Err(error.into()),
        };
        serde_json::from_value(result)
            .map_err(|_| ClientError::Protocol("invalid board result").into())
    }
}

fn outcome_unknown(resource: ResourceIdentity) -> BoardClientError {
    BoardClientError::OutcomeUnknown {
        resource,
        message: "Board write outcome is unknown. Inspect the affected resource before deciding whether to retry.",
        next_action: BoardNextAction::InspectResource,
    }
}

fn subscription_resource(scope: &SubscriptionTarget) -> ResourceIdentity {
    match scope {
        SubscriptionTarget::Thread { thread } => ResourceIdentity::Thread {
            thread_id: thread.thread.clone(),
        },
        SubscriptionTarget::Project { project_id } => ResourceIdentity::Project {
            project_id: project_id.clone(),
        },
    }
}
