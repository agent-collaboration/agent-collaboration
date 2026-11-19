//! Composes central mail with the interim engine and request entry.
use crate::WorkspaceRuntime;
use crate::engine::attempt_statements::{CONTROL_WRITE, delete_attempt};
use crate::engine::port::Engine;
use central_engine_model::{ItemFailureClass, RetirementThreshold};
use central_storage::{AlarmUpdate, GuardedStatement, StoreFailure};
use fleet_contracts::{
    AlarmRecompute, ControlGate, EngineAttemptControl, EngineSubject, FailureOutcome,
    SubjectVersion, WorkspaceCaller,
};
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

impl<Host: CentralStore, Stores: WorkspaceStores> AlarmRecompute
    for Engine<'_, WorkspaceRuntime<Host, Stores>>
{
    fn alarm_update(&self) -> AlarmUpdate {
        crate::engine::query_registry::alarm_update()
    }
}

impl<Host: CentralStore, Stores: WorkspaceStores> EngineAttemptControl
    for Engine<'_, WorkspaceRuntime<Host, Stores>>
{
    async fn control_gate(
        &self,
        subject: &EngineSubject,
        threshold: RetirementThreshold,
    ) -> ControlGate {
        Engine::control_gate(self, subject, threshold).await
    }
    fn observe_engine_store_down(&self, subject: &EngineSubject, failure: StoreFailure) {
        Engine::observe_engine_store_down(self, subject, failure);
    }
    async fn record_failure_naming(
        &self,
        subject: &EngineSubject,
        version: &SubjectVersion,
        class: ItemFailureClass,
        threshold: RetirementThreshold,
        workspaces: &[WorkspaceId],
    ) -> FailureOutcome {
        Engine::record_failure_naming(
            self,
            subject,
            CONTROL_WRITE,
            version,
            class,
            threshold,
            workspaces,
        )
        .await
    }
    fn attempt_cleanup_statements(&self, subject: &EngineSubject) -> GuardedStatement {
        delete_attempt(subject)
    }
}
