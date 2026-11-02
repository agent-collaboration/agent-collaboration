//! Native preparation stand-in: counts the effect boundary; central and SQLite stay real.
use agent_automation::{
    CessationEvidence, NativeEffectEvidence, PreparationEffect, RouteEffectEvidence,
    SubmissionEffect,
};
use collaboration_contracts::*;
use collaboration_protocol::{CodexGeneration, SessionRef};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

pub struct PreparationExecution {
    pub target: SessionRef,
    pub creations: AtomicUsize,
    pub inputs: Mutex<Vec<SchedulePreparationRequest>>,
}
impl PreparationExecution {
    pub fn evidence(&self, created: bool) -> RouteEffectEvidence<SessionRef, CodexGeneration> {
        RouteEffectEvidence::CodexAppServer(NativeEffectEvidence {
            permission_differences: Vec::new(),
            target: created.then(|| self.target.clone()),
            generation: None,
            client_user_message_id: None,
            native_turn_id: None,
            native_submission_id: None,
            allocation: if created {
                PreparationEffect::Accepted
            } else {
                PreparationEffect::NotRequested
            },
            resume: PreparationEffect::NotRequested,
            submission: SubmissionEffect::NotDispatched,
            cessation: CessationEvidence::NotApplicable,
        })
    }
}
fn unused<'a, T: Send + 'a>() -> DeliveryFuture<'a, T> {
    Box::pin(async { Err(DeliveryContractError::InvalidEvidence) })
}
impl ScheduledRunExecution for PreparationExecution {
    fn support(&self, _: &ScheduleDestination) -> DeliveryFuture<'_, ScheduleSupport> {
        Box::pin(async {
            Ok(ScheduleSupport::Supported {
                settlement: SettlementEvidence::TurnCompletion,
            })
        })
    }
    fn initial_evidence(
        &self,
        _: &ScheduleDestination,
    ) -> DeliveryFuture<'_, RouteEffectEvidence<SessionRef, CodexGeneration>> {
        Box::pin(async { Ok(self.evidence(false)) })
    }
    fn prepare_destination<'a>(
        &'a self,
        request: SchedulePreparationRequest,
        sink: &'a dyn PreparationEvidenceSink,
    ) -> DeliveryFuture<'a, SchedulePreparationOutcome> {
        Box::pin(async move {
            if !sink.record_intent(self.evidence(false)).await? {
                return Err(DeliveryContractError::EvidencePersistence);
            }
            self.inputs
                .lock()
                .map_err(|_| DeliveryContractError::InvalidEvidence)?
                .push(request);
            self.creations.fetch_add(1, Ordering::SeqCst);
            Ok(SchedulePreparationOutcome::Prepared(PreparedTarget {
                target: self.target.clone(),
                evidence: self.evidence(true),
            }))
        })
    }
    fn prepare_existing_target<'a>(
        &'a self,
        _: &SessionRef,
        _: &str,
        _: &'a dyn RunEvidenceSink,
    ) -> DeliveryFuture<'a, PreparedTarget> {
        unused()
    }
    fn prepare_fresh_session<'a>(
        &'a self,
        _: FreshSessionRequest,
        _: &'a dyn RunEvidenceSink,
    ) -> DeliveryFuture<'a, PreparedTarget> {
        unused()
    }
    fn submit_run<'a>(
        &'a self,
        _: ScheduledRunSubmission,
        _: &'a dyn RunEvidenceSink,
    ) -> DeliveryFuture<'a, RunSubmission> {
        unused()
    }
    fn observe_settlement(&self, _: RunObservationContext) -> DeliveryFuture<'_, RunSettlement> {
        unused()
    }
    fn summary_source(&self, _: RunObservationContext) -> DeliveryFuture<'_, RunSummarySource> {
        unused()
    }
    fn request_stop<'a>(
        &'a self,
        _: RunObservationContext,
        _: &'a dyn RunEvidenceSink,
    ) -> DeliveryFuture<'a, StopRequestOutcome> {
        unused()
    }
    fn reconcile_run(&self, _: RunObservationContext) -> DeliveryFuture<'_, RunReconciliation> {
        unused()
    }
}
