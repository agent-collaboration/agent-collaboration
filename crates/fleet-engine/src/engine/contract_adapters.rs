use super::attempt_statements::{CONTROL_WRITE, delete_attempt};
use super::port::{Engine, EngineHost};
use central_engine_model::{ItemFailureClass, RetirementThreshold};
use central_storage::{AlarmUpdate, GuardedStatement, StoreFailure};
use fleet_contracts::{
    AlarmRecompute, ControlGate, EngineAttemptControl, EngineSubject, FailureOutcome,
    SubjectVersion,
};
use workspace_model::WorkspaceId;

impl<Host: EngineHost> AlarmRecompute for Engine<'_, Host> {
    fn alarm_update(&self) -> AlarmUpdate {
        super::query_registry::alarm_update()
    }
}

impl<Host: EngineHost> EngineAttemptControl for Engine<'_, Host> {
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
