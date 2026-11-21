//! Shared requests and replies for Central's calls to an owning workspace.
use central_storage::StoreFailure;
use fleet_rpc::{CallFailure, Rpc, RpcClient, Transport};
use serde::{Deserialize, Serialize};
use std::future::Future;
use workspace_model::{
    ActorIdentityId, ItemRef, RefusalReason, RequestId, Timestamp, WorkspaceAnswer, WorkspaceId,
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
    Done(Box<WorkspaceAnswer>),
    ItemNotFound {
        workspace_id: WorkspaceId,
        item: ItemRef,
    },
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
    pub fn unavailable_reply(&self, failure: StoreFailure) -> WorkspaceResponse {
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
    ReadRequestStatus(Result<RetainedWorkspaceStatus, StoreFailure>),
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
/// use fleet_contracts::WorkspaceServiceRpc;
/// struct IncompleteService;
/// impl WorkspaceServiceRpc for IncompleteService {}
/// ```
///
/// ```compile_fail,E0271
/// use fleet_contracts::{ReadWorkspaceRequestStatus, WorkspaceServiceRpc};
/// struct WrongReply;
/// impl WorkspaceServiceRpc for WrongReply {
///     async fn read_request_status(&self, _: ReadWorkspaceRequestStatus) -> u8 { 0 }
/// }
/// ```
pub trait WorkspaceServiceRpc: Send + Sync {
    fn read_request_status(
        &self,
        request: ReadWorkspaceRequestStatus,
    ) -> impl Future<Output = Result<RetainedWorkspaceStatus, StoreFailure>> + Send;
}

/// Typed client for the owning workspace service.
pub type WorkspaceServiceClient<T> = RpcClient<WorkspaceServiceProtocol, T>;

impl<T: Transport<WorkspaceServiceProtocol>> WorkspaceServiceRpc for WorkspaceServiceClient<T> {
    async fn read_request_status(
        &self,
        request: ReadWorkspaceRequestStatus,
    ) -> Result<RetainedWorkspaceStatus, StoreFailure> {
        match self
            .exchange_message(WorkspaceRequest::ReadRequestStatus(request))
            .await
            .map_err(|failure| match failure {
                CallFailure::NeverReached => StoreFailure::Unavailable,
                CallFailure::AnswerLost => StoreFailure::AnswerLost,
            })? {
            WorkspaceResponse::ReadRequestStatus(reply) => reply,
        }
    }
}

#[doc(hidden)]
pub async fn dispatch_workspace_request(
    service: &impl WorkspaceServiceRpc,
    message: WorkspaceRequest,
) -> WorkspaceResponse {
    match message {
        WorkspaceRequest::ReadRequestStatus(request) => {
            WorkspaceResponse::ReadRequestStatus(service.read_request_status(request).await)
        }
    }
}
