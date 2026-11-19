//! Mail drain operations invoked while applying workspace requests.
use central_storage::StoreFailure;
use std::future::Future;
use workspace_model::{Timestamp, WorkspaceId};

pub trait WorkspaceDrain: Send + Sync {
    fn preset_workspace_drain(
        &self,
        workspace: &WorkspaceId,
        now: Timestamp,
    ) -> impl Future<Output = Result<(), StoreFailure>> + Send;

    fn drain_after_request(
        &self,
        workspace: &WorkspaceId,
    ) -> impl Future<Output = Result<(), StoreFailure>> + Send;
}
