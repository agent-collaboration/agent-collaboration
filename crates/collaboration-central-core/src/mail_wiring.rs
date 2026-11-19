//! Composes central mail with the interim engine and request entry.
use crate::WorkspaceRuntime;
use central_storage::StoreFailure;
use fleet_contracts::{AlarmRecompute, EngineAttemptControl, WorkspaceCaller};
use fleet_engine::Engine;
use fleet_mail::{CentralMail, DrainResult};
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use workspace_model::central_mail as mail;
use workspace_model::{SessionAddress, WorkspaceId};

impl<Host: CentralStore, Stores: WorkspaceStores> WorkspaceRuntime<Host, Stores> {
    pub(crate) fn mail(
        &self,
    ) -> CentralMail<'_, Host, Stores, impl EngineAttemptControl + AlarmRecompute + '_> {
        CentralMail::new(&self.fleet_runtime, Engine::new(self))
    }

    pub async fn request_central(
        &self,
        caller: &WorkspaceCaller,
        request: mail::CentralMailRequest,
    ) -> Result<mail::CentralMailResult, StoreFailure> {
        // The request path answers as on main; the due batch that follows checks
        // the organization stamp once for this event before any unit (P3).
        let result = self.mail().execute_central(caller, request).await;
        // Due work and publication cannot replace this event's durable answer.
        let _ = self.after_event().await;
        match result {
            Err(StoreFailure::Unavailable | StoreFailure::AnswerLost) => {
                Ok(mail::CentralMailResult::CentralUnavailable)
            }
            result => result,
        }
    }

    pub async fn queued_for_session(
        &self,
        session: &SessionAddress,
    ) -> Result<mail::SessionQueue, StoreFailure> {
        self.mail().queued_for_session(session).await
    }

    pub async fn drain_workspace(
        &self,
        workspace: &WorkspaceId,
    ) -> Result<DrainResult, StoreFailure> {
        self.mail().drain_workspace(workspace).await
    }
}
