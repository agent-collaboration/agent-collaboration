//! Completes the original workspace event after its request finishes.
use crate::WorkspaceRuntime;
use central_storage::StoreFailure;
use fleet_contracts::WorkspaceCaller;
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use workspace_model as m;

impl<Host: CentralStore, Stores: WorkspaceStores> WorkspaceRuntime<Host, Stores> {
    pub async fn request(
        &self,
        caller: &WorkspaceCaller,
        request: m::WorkspaceRequest,
    ) -> Result<m::WorkspaceReply, StoreFailure> {
        // Admission answers for the organization as on main; the due batch that
        // follows checks the stamp once for this event before any unit (P3).
        let result = fleet_workspace_requests::execute(
            &self.fleet_runtime,
            &self.workspace_directory(),
            &self.mail(),
            caller,
            request,
        )
        .await;
        let _ = self.after_event().await;
        match result {
            Err(StoreFailure::Unavailable) => Ok(m::WorkspaceReply {
                result: m::WorkspaceResult::CentralUnavailable,
                actors: vec![],
            }),
            Err(StoreFailure::AnswerLost) => Ok(m::WorkspaceReply {
                result: m::WorkspaceResult::CentralUnavailable,
                actors: vec![],
            }),
            result => result,
        }
    }
}
