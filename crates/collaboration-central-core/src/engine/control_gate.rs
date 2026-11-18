use super::port::{Engine, EngineHost};
use super::{
    attempt_records::{AttemptGuard, EngineSubject},
    attempt_statements::{CONTROL_WRITE, delete_observed_attempt},
    attempt_store::load_attempt,
    effects::{ApplyFailure, classify_batch},
    failures::FailureOutcome,
    store_adapter::SubjectVersion,
};
use fleet_runtime_ports::CentralStore as _;
use central_engine_model::{AttemptState, RetirementThreshold};
use central_storage::{AlarmUpdate, StatementBatch, StoreFailure};

/// The control record a run saw at its gate. Its success clears only that
/// record, so a failure written after the gate survives (R3/R4).
pub(crate) enum ObservedControl {
    Absent,
    Present(AttemptGuard),
}

pub(crate) enum ControlGate {
    Run(ObservedControl),
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
            Ok(None) => return ControlGate::Run(ObservedControl::Absent),
            Ok(Some(stored)) => stored,
            Err(StoreFailure::AnswerLost) => return ControlGate::Uncertain,
            Err(failure) => return ControlGate::CentralDown(failure),
        };
        match &stored.record {
            Ok(record) => match record.state {
                AttemptState::Retired { .. } => ControlGate::Skip,
                AttemptState::Failing { next_try_at } => match self.now() {
                    Ok(now) if next_try_at <= now => {
                        ControlGate::Run(ObservedControl::Present(stored.guard.clone()))
                    }
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

    /// Clears the record this run observed at its gate, and nothing when it
    /// observed none: a newer failure written meanwhile survives (C3).
    pub(crate) async fn clear_control(
        &self,
        subject: &EngineSubject,
        observed: &ObservedControl,
    ) -> Result<(), ApplyFailure> {
        let guard = match observed {
            ObservedControl::Absent => return Ok(()),
            ObservedControl::Present(guard) => guard,
        };
        classify_batch(
            self.host()
                .run_batch(StatementBatch {
                    statements: vec![delete_observed_attempt(subject, guard)],
                    alarm: AlarmUpdate::Unchanged,
                })
                .await,
        )
    }
}
