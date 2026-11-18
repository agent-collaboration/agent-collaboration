//! Each workspace's own store, reached by a host-supplied call.
use central_storage::{StoreCall, StoreFailure, StoreReply};
use std::future::Future;
use workspace_ids::WorkspaceId;

pub trait WorkspaceStores: Clone + Send + Sync + 'static {
    fn call_workspace(
        &self,
        workspace: &WorkspaceId,
        call: StoreCall,
    ) -> impl Future<Output = Result<StoreReply, StoreFailure>> + Send;
}
