use super::port::{Engine, EngineHost};
use super::{
    attempt_records::EngineSubject,
    attempt_statements::{CONTROL_WRITE, delete_attempt},
    attempt_store::load_attempt,
    effects::{ApplyFailure, classify_batch},
    failures::FailureOutcome,
    store_adapter::SubjectVersion,
};
use crate::ObjectHost as _;
use central_engine_model::{AttemptState, RetirementThreshold};
use central_storage::{AlarmUpdate, StatementBatch, StoreFailure};

pub(crate) enum ControlGate {
    Run,
    Skip,
    Uncertain,
    CentralDown(StoreFailure),
}

impl<R: EngineHost> Engine<'_, R> {
    pub(crate) async fn control_gate(
        &self,
        subject: &EngineSubject,
        threshold: RetirementThreshold,
    ) -> ControlGate {
        let stored = match load_attempt(self.host(), subject).await {
            Ok(None) => return ControlGate::Run,
            Ok(Some(stored)) => stored,
            Err(StoreFailure::AnswerLost) => return ControlGate::Uncertain,
            Err(failure) => return ControlGate::CentralDown(failure),
        };
        match &stored.record {
            Ok(record) => match record.state {
                AttemptState::Retired { .. } => ControlGate::Skip,
                AttemptState::Failing { next_try_at } => match self.now() {
                    Ok(now) if next_try_at <= now => ControlGate::Run,
                    Ok(_) => ControlGate::Skip,
                    Err(failure) => ControlGate::CentralDown(failure),
                },
            },
            Err(corruption) => match self
                .write_failure(
                    subject,
                    CONTROL_WRITE,
                    &SubjectVersion(vec![]),
                    corruption.failure_class(),
                    threshold,
                    Some(&stored),
                )
                .await
            {
                FailureOutcome::Recorded(_) | FailureOutcome::Superseded => ControlGate::Skip,
                FailureOutcome::Uncertain => ControlGate::Uncertain,
                FailureOutcome::StoreDown(failure) => ControlGate::CentralDown(failure),
            },
        }
    }

    pub(crate) async fn clear_control(&self, subject: &EngineSubject) -> Result<(), ApplyFailure> {
        classify_batch(
            self.host()
                .run_batch(StatementBatch {
                    statements: vec![delete_attempt(subject)],
                    alarm: AlarmUpdate::Unchanged,
                })
                .await,
        )
    }
}
