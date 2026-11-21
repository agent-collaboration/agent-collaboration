//! Shared requests and replies for Central's calls to an owning workspace.
use fleet_rpc::{CallError, CallFailure, Rpc, RpcClient, Transport};
use serde::{Deserialize, Serialize};
use std::future::Future;
use workspace_model::{
    ActorIdentityId, ChangePosition, ItemRef, ReadScope, RefusalReason, RequestId, Timestamp,
    WorkspaceId,
};

/// Protocol selected by the owning workspace service client.
pub struct WorkspaceServiceProtocol;

impl Rpc for WorkspaceServiceProtocol {
    type Target = WorkspaceId;
    type Request = WorkspaceRequest;
    type Reply = WorkspaceResponse;
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadWorkspaceRequestStatus {
    pub request_id: RequestId,
    pub actor: ActorIdentityId,
    pub cutoff: Timestamp,
}

/// Decisions retained by the owning workspace, or the absence of a visible receipt.
///
/// A transient request outcome cannot be returned as a retained answer.
/// ```compile_fail,E0308
/// use fleet_contracts::RetainedWorkspaceStatus;
/// use workspace_model::WorkspaceResult;
/// let _ = RetainedWorkspaceStatus::Done(Box::new(WorkspaceResult::OutcomeUnknown));
/// ```
///
/// A list page cannot be used as a retained write answer.
/// ```compile_fail,E0308
/// use fleet_contracts::RetainedWorkspaceStatus;
/// use workspace_model::WorkspaceAnswer;
/// let _ = RetainedWorkspaceStatus::Done(Box::new(WorkspaceAnswer::Page {
///     items: vec![], next_cursor: None,
/// }));
/// ```
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RetainedWorkspaceStatus {
    /// No visible retained receipt matches the query.
    NotFound,
    Refused(RefusalReason),
    Done(Box<RetainedWriteAnswer>),
    ItemNotFound {
        workspace_id: WorkspaceId,
        item: ItemRef,
    },
}

/// Successful decisions that the workspace write gate can retain.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RetainedWriteAnswer {
    Created {
        item: ItemRef,
        position: ChangePosition,
    },
    Updated {
        item: ItemRef,
        position: ChangePosition,
    },
    Removed {
        item: ItemRef,
        position: ChangePosition,
    },
    Marked {
        scope: ReadScope,
        through: ChangePosition,
    },
    Unchanged {
        item: ItemRef,
    },
}

/// Failure reported by the workspace that owns the retained receipt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceStatusFailure {
    #[error("the store is unavailable")]
    Unavailable,
    #[error("the store holds corrupt data")]
    Corrupt,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "call",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum WorkspaceRequest {
    ReadRequestStatus(ReadWorkspaceRequestStatus),
}

impl WorkspaceRequest {
    /// Encode a service-open failure as the reply for this request.
    #[doc(hidden)]
    pub fn unavailable_reply(&self, failure: WorkspaceStatusFailure) -> WorkspaceResponse {
        match self {
            Self::ReadRequestStatus(_) => WorkspaceResponse::ReadRequestStatus(Err(failure)),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "reply",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum WorkspaceResponse {
    ReadRequestStatus(Result<RetainedWorkspaceStatus, WorkspaceStatusFailure>),
}

/// Read-only status on the owning workspace.
///
/// Clients for a different protocol do not implement this service.
/// ```compile_fail,E0599
/// use fleet_contracts::{ReadWorkspaceRequestStatus, WorkspaceServiceRpc};
/// use fleet_rpc::{Rpc, RpcClient, Transport};
/// struct OtherProtocol;
/// impl Rpc for OtherProtocol {
///     type Target = u8;
///     type Request = ();
///     type Reply = ();
/// }
/// fn wrong_client<T: Transport<OtherProtocol>>(
///     client: &RpcClient<OtherProtocol, T>, request: ReadWorkspaceRequestStatus,
/// ) {
///     let _ = client.read_request_status(request);
/// }
/// ```
///
/// Both the handler and its reply type are required.
/// ```compile_fail,E0046
/// use fleet_contracts::{WorkspaceServiceRpc, WorkspaceStatusFailure};
/// struct IncompleteService;
/// impl WorkspaceServiceRpc for IncompleteService {
///     type Failure = WorkspaceStatusFailure;
/// }
/// ```
///
/// ```compile_fail,E0271
/// use fleet_contracts::{ReadWorkspaceRequestStatus, WorkspaceServiceRpc};
/// use fleet_contracts::WorkspaceStatusFailure;
/// struct WrongReply;
/// impl WorkspaceServiceRpc for WrongReply {
///     type Failure = WorkspaceStatusFailure;
///     async fn read_request_status(&self, _: ReadWorkspaceRequestStatus) -> u8 { 0 }
/// }
/// ```
pub trait WorkspaceServiceRpc: Send + Sync {
    type Failure;

    fn read_request_status(
        &self,
        request: ReadWorkspaceRequestStatus,
    ) -> impl Future<Output = Result<RetainedWorkspaceStatus, Self::Failure>> + Send;
}

/// Typed client for the owning workspace service.
pub type WorkspaceServiceClient<T> = RpcClient<WorkspaceServiceProtocol, T>;

impl<T: Transport<WorkspaceServiceProtocol>> WorkspaceServiceRpc for WorkspaceServiceClient<T> {
    type Failure = CallError<WorkspaceStatusFailure>;

    async fn read_request_status(
        &self,
        request: ReadWorkspaceRequestStatus,
    ) -> Result<RetainedWorkspaceStatus, Self::Failure> {
        match self
            .exchange_message(WorkspaceRequest::ReadRequestStatus(request))
            .await
            .map_err(|failure| match failure {
                CallFailure::NeverReached => CallError::NeverReached,
                CallFailure::AnswerLost => CallError::AnswerLost,
            })? {
            WorkspaceResponse::ReadRequestStatus(reply) => reply.map_err(CallError::Service),
        }
    }
}

#[doc(hidden)]
pub async fn dispatch_workspace_request(
    service: &impl WorkspaceServiceRpc<Failure = WorkspaceStatusFailure>,
    message: WorkspaceRequest,
) -> WorkspaceResponse {
    match message {
        WorkspaceRequest::ReadRequestStatus(request) => {
            WorkspaceResponse::ReadRequestStatus(service.read_request_status(request).await)
        }
    }
}
