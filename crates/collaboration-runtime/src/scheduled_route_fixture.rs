//! A fake execution route drives provider and peer runs through the application's stored
//! inspection.
use crate::scheduled_run_worker::ScheduledRunWorker;
use agent_automation::{
    AttemptId, ClaudeCodePeerEffectEvidence, ContinuityInput, InstructionText, OperationId,
    PeerWriteEffect, ProviderAcpEffectEvidence, ProviderBindingReference, ProviderSettlementEffect,
    ScheduleDefinition, SubmissionEffect, TimingRule,
};
use agent_automation::{ExecutionDestination, RouteEffectEvidence, RunId};
use automation_storage::AutomationStore;
use automation_storage::ScheduleCreate;
use collaboration_contracts::DeliveryContractError;
use collaboration_contracts::DeliveryFuture;
use collaboration_contracts::{RunObservationContext, ScheduledRunExecution};
use collaboration_protocol::RunExecution;
use collaboration_protocol::{CodexGeneration, EndpointRef, PushId, SessionRef};
use std::sync::Arc;
use tokio::sync::Mutex;

use collaboration_contracts::FreshSessionRequest;
use collaboration_contracts::PreparationEvidenceSink;
use collaboration_contracts::PreparedTarget;
use collaboration_contracts::RunAcceptance;
use collaboration_contracts::RunEvidenceDisposition;
use collaboration_contracts::RunEvidenceSink;
use collaboration_contracts::RunReconciliation;
use collaboration_contracts::RunSettlement;
use collaboration_contracts::RunSubmission;
use collaboration_contracts::RunSummarySource;
use collaboration_contracts::ScheduleCapability;
use collaboration_contracts::ScheduleDestination;
use collaboration_contracts::SchedulePreparationFailure;
use collaboration_contracts::SchedulePreparationOutcome;
use collaboration_contracts::SchedulePreparationRequest;
use collaboration_contracts::ScheduleSupport;
use collaboration_contracts::ScheduledRunPayload;
use collaboration_contracts::ScheduledRunSubmission;
use collaboration_contracts::SettlementEvidence;
use collaboration_contracts::StopRequestOutcome;
use collaboration_protocol::{
    DeliveryClientReceipt, DeliveryNextAction, DeliveryOutcome, DeliveryReceipt, DeliveryRejection,
    DeliveryRejectionReason, PushDeliveryState, PushKind, RouterLink, SessionReachability,
    parse_push_line_header,
};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

pub type TestResult<TValue> = Result<TValue, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Copy)]
pub enum FakeRouteKind {
    Provider,
    Peer,
}

#[derive(Clone, Copy)]
pub enum FakeSubmissionPlan {
    Accept,
    Reject,
    RejectOnceThenAccept,
    Unknown,
    CrashAfterDispatchIntent,
}

#[derive(Clone, Copy)]
pub enum FakeSettlementPlan {
    Completed,
    Failed,
    Interrupted,
    Pending,
    WrittenWithoutCompletion,
}

pub struct FakeScheduledExecution {
    pub kind: FakeRouteKind,
    pub target: SessionRef,
    pub interactions: Arc<interaction_storage::InteractionStore>,
    pub observed_push_ids: Arc<std::sync::Mutex<Vec<PushId>>>,
    pub generation: CodexGeneration,
    pub attempt_id: AttemptId,
    pub submissions: AtomicUsize,
    pub submission_plan: FakeSubmissionPlan,
    pub settlement_plan: FakeSettlementPlan,
}

impl FakeScheduledExecution {
    fn prepared_evidence(
        &self,
    ) -> Result<RouteEffectEvidence<SessionRef, CodexGeneration>, DeliveryContractError> {
        Ok(match self.kind {
            FakeRouteKind::Provider => {
                RouteEffectEvidence::ProviderAcp(ProviderAcpEffectEvidence {
                    target: self.target.clone(),
                    generation: self.generation.clone(),
                    binding: ProviderBindingReference::try_from("fixture-binding".to_owned())
                        .map_err(|_| DeliveryContractError::InvalidEvidence)?,
                    attempt_id: self.attempt_id.clone(),
                    submission: SubmissionEffect::NotDispatched,
                    settlement: ProviderSettlementEffect::NotObserved,
                })
            }
            FakeRouteKind::Peer => {
                RouteEffectEvidence::ClaudeCodePeer(ClaudeCodePeerEffectEvidence {
                    session_id: String::from(self.target.session_id.clone())
                        .try_into()
                        .map_err(|_| DeliveryContractError::InvalidEvidence)?,
                    process_id: 42_u32
                        .try_into()
                        .map_err(|_| DeliveryContractError::InvalidEvidence)?,
                    write: PeerWriteEffect::NotDispatched,
                })
            }
        })
    }
}

impl ScheduledRunExecution for FakeScheduledExecution {
    fn prepare_destination<'a>(
        &'a self,
        request: SchedulePreparationRequest,
        sink: &'a dyn PreparationEvidenceSink,
    ) -> DeliveryFuture<'a, SchedulePreparationOutcome> {
        Box::pin(async move {
            let mut evidence = self.prepared_evidence()?;
            if matches!(
                request.destination,
                collaboration_protocol::DestinationPreparation::Fork { .. }
            ) {
                return Ok(SchedulePreparationOutcome::Failed(
                    SchedulePreparationFailure {
                        kind: collaboration_protocol::ScheduleFailureKind::UnsupportedCapability,
                        explanation: "fork is Codex-only".into(),
                        evidence,
                        uncertain: false,
                    },
                ));
            }
            if let RouteEffectEvidence::ProviderAcp(provider) = &mut evidence {
                provider.submission = SubmissionEffect::NotDispatched;
            }
            if !sink.record_intent(evidence.clone()).await? {
                return Err(DeliveryContractError::EvidencePersistence);
            }
            Ok(SchedulePreparationOutcome::Prepared(PreparedTarget {
                target: self.target.clone(),
                evidence,
            }))
        })
    }

    fn initial_evidence(
        &self,
        _: &ScheduleDestination,
    ) -> DeliveryFuture<'_, RouteEffectEvidence<SessionRef, CodexGeneration>> {
        Box::pin(async move {
            let mut evidence = self.prepared_evidence()?;
            match &mut evidence {
                RouteEffectEvidence::ProviderAcp(provider) => {
                    provider.submission = SubmissionEffect::NotDispatched;
                }
                RouteEffectEvidence::ClaudeCodePeer(peer) => {
                    peer.write = PeerWriteEffect::NotDispatched;
                }
                RouteEffectEvidence::CodexAppServer(_) => {}
            }
            Ok(evidence)
        })
    }

    fn support(&self, destination: &ScheduleDestination) -> DeliveryFuture<'_, ScheduleSupport> {
        let destination = destination.clone();
        Box::pin(async move {
            if matches!(destination, ScheduleDestination::Fresh { .. }) {
                return Ok(ScheduleSupport::Unsupported {
                    missing: vec![ScheduleCapability::CreateSession],
                });
            }
            if matches!(destination, ScheduleDestination::Fork { .. }) {
                return Ok(ScheduleSupport::Unsupported {
                    missing: vec![ScheduleCapability::ForkSession],
                });
            }
            Ok(ScheduleSupport::Supported {
                settlement: match self.kind {
                    FakeRouteKind::Provider => SettlementEvidence::OperationSettlement,
                    FakeRouteKind::Peer => SettlementEvidence::WriteOnly,
                },
            })
        })
    }

    fn prepare_existing_target<'a>(
        &'a self,
        target: &SessionRef,
        _declared_cwd: &str,
        sink: &'a dyn RunEvidenceSink,
    ) -> DeliveryFuture<'a, PreparedTarget> {
        let target = target.clone();
        Box::pin(async move {
            if target != self.target {
                return Err(DeliveryContractError::InvalidEvidence);
            }
            let evidence = self.prepared_evidence()?;
            if matches!(
                sink.record(evidence.clone()).await?,
                RunEvidenceDisposition::AdmissionRefused
            ) {
                return Err(DeliveryContractError::EvidencePersistence);
            }
            Ok(PreparedTarget { target, evidence })
        })
    }

    fn prepare_fresh_session<'a>(
        &'a self,
        _: FreshSessionRequest,
        sink: &'a dyn RunEvidenceSink,
    ) -> DeliveryFuture<'a, PreparedTarget> {
        self.prepare_existing_target(&self.target, "", sink)
    }

    fn submit_run<'a>(
        &'a self,
        run: ScheduledRunSubmission,
        sink: &'a dyn RunEvidenceSink,
    ) -> DeliveryFuture<'a, RunSubmission> {
        Box::pin(async move {
            if run.target != self.target {
                return Err(DeliveryContractError::InvalidEvidence);
            }
            let ScheduledRunPayload::Existing { prepared } = run.payload else {
                return Err(DeliveryContractError::InvalidEvidence);
            };
            let prepared_link_matches_id = prepared
                .line
                .as_str()
                .split_whitespace()
                .last()
                .and_then(|link| RouterLink::parse(link).ok())
                .is_some_and(|link| link.push_id() == &prepared.push_id);
            if !prepared_link_matches_id {
                return Err(DeliveryContractError::InvalidEvidence);
            }
            if prepared.push_id.as_str() == run.run_id.as_str()
                || parse_push_line_header(prepared.line.as_str())
                    .is_none_or(|header| header.kind != PushKind::ScheduleRun)
            {
                return Err(DeliveryContractError::InvalidEvidence);
            }
            let stored_schedule_push = self
                .interactions
                .get_push_record(&prepared.push_id)
                .await
                .map_err(|_| DeliveryContractError::EvidencePersistence)?
                .ok_or(DeliveryContractError::InvalidEvidence)?;
            if stored_schedule_push.kind != PushKind::ScheduleRun
                || stored_schedule_push.target != run.target
                || stored_schedule_push.delivery_state != PushDeliveryState::Attempted
                || stored_schedule_push.origin_router_ref.is_none()
                || stored_schedule_push.body.as_deref()
                    != Some(run.inputs.instruction_text.as_str())
            {
                return Err(DeliveryContractError::InvalidEvidence);
            }
            let previous_push_id = self
                .observed_push_ids
                .lock()
                .map_err(|_| DeliveryContractError::InvalidEvidence)?
                .last()
                .cloned();
            if let Some(previous_push_id) = previous_push_id
                && previous_push_id != prepared.push_id
                && self
                    .interactions
                    .get_push_record(&previous_push_id)
                    .await
                    .map_err(|_| DeliveryContractError::EvidencePersistence)?
                    .is_some()
            {
                return Err(DeliveryContractError::InvalidEvidence);
            }
            self.observed_push_ids
                .lock()
                .map_err(|_| DeliveryContractError::InvalidEvidence)?
                .push(prepared.push_id.clone());
            let submission_attempt = self.submissions.fetch_add(1, Ordering::SeqCst);
            if submission_attempt == 0 {
                return Ok(RunSubmission::NotStartedBusy);
            }
            let mut evidence = run.recorded;
            match &mut evidence {
                RouteEffectEvidence::ProviderAcp(provider) => {
                    provider.submission = SubmissionEffect::Dispatching;
                }
                RouteEffectEvidence::ClaudeCodePeer(peer) => {
                    peer.write = PeerWriteEffect::Dispatching;
                }
                RouteEffectEvidence::CodexAppServer(_) => {}
            }
            let timing = match sink.record(evidence.clone()).await? {
                RunEvidenceDisposition::Recorded {
                    timing: Some(timing),
                } => timing,
                _ => return Err(DeliveryContractError::InvalidEvidence),
            };
            match (self.submission_plan, submission_attempt) {
                (FakeSubmissionPlan::Reject, _) | (FakeSubmissionPlan::RejectOnceThenAccept, 1) => {
                    if let RouteEffectEvidence::ProviderAcp(provider) = &mut evidence {
                        provider.submission = SubmissionEffect::Rejected;
                    }
                    sink.record(evidence).await?;
                    return Ok(RunSubmission::Rejected(DeliveryRejection {
                        reason: DeliveryRejectionReason::Busy,
                        next_action: DeliveryNextAction::RetryLater,
                        client_code: None,
                        detail: Some("fixture rejection".into()),
                        claims: None,
                        native_failure: None,
                    }));
                }
                (FakeSubmissionPlan::Unknown, _) => {
                    if let RouteEffectEvidence::ProviderAcp(provider) = &mut evidence {
                        provider.submission = SubmissionEffect::Unknown;
                    }
                    sink.record(evidence).await?;
                    return Ok(RunSubmission::Unknown);
                }
                (FakeSubmissionPlan::CrashAfterDispatchIntent, _) => {
                    return Err(DeliveryContractError::EvidencePersistence);
                }
                (FakeSubmissionPlan::Accept, _) | (FakeSubmissionPlan::RejectOnceThenAccept, _) => {
                }
            }
            let started_at = crate::observation_timestamp::timestamp(timing.dispatch_started_at_ms)
                .map_err(|_| DeliveryContractError::InvalidEvidence)?;
            let deadline_at = crate::observation_timestamp::timestamp(timing.deadline_at_ms)
                .map_err(|_| DeliveryContractError::InvalidEvidence)?;
            let timeout = timing
                .effective_timeout_seconds
                .try_into()
                .map_err(|_| DeliveryContractError::InvalidEvidence)?;
            let (execution, receipt) = match &mut evidence {
                RouteEffectEvidence::ProviderAcp(provider) => {
                    provider.submission = SubmissionEffect::Accepted;
                    let operation_id = OperationId::try_from(self.attempt_id.as_str().to_owned())
                        .map_err(|_| DeliveryContractError::InvalidEvidence)?;
                    (
                        RunExecution::ProviderAcp {
                            target: self.target.clone(),
                            operation_id: operation_id.clone(),
                            started_at,
                            deadline_at,
                            effective_timeout_seconds: timeout,
                        },
                        DeliveryReceipt {
                            outcome: DeliveryOutcome::Started,
                            reachability: Some(SessionReachability::ProviderAcp),
                            client: Some(DeliveryClientReceipt::ProviderAcp { operation_id }),
                        },
                    )
                }
                RouteEffectEvidence::ClaudeCodePeer(peer) => {
                    peer.write = PeerWriteEffect::Written;
                    (
                        RunExecution::ClaudeCodePeer {
                            target: self.target.clone(),
                            written_at: started_at,
                        },
                        DeliveryReceipt {
                            outcome: DeliveryOutcome::PeerMessageWritten,
                            reachability: Some(SessionReachability::ClaudeCodePeer),
                            client: Some(DeliveryClientReceipt::ClaudeCodePeer),
                        },
                    )
                }
                RouteEffectEvidence::CodexAppServer(_) => {
                    return Err(DeliveryContractError::InvalidEvidence);
                }
            };
            sink.record(evidence).await?;
            Ok(RunSubmission::Started(Box::new(RunAcceptance {
                execution,
                receipt,
            })))
        })
    }

    fn observe_settlement(&self, _: RunObservationContext) -> DeliveryFuture<'_, RunSettlement> {
        Box::pin(async {
            Ok(match self.settlement_plan {
                FakeSettlementPlan::Completed => RunSettlement::Completed {
                    summary_source: None,
                },
                FakeSettlementPlan::Failed => RunSettlement::Failed {
                    reason: "fixture operation failed".into(),
                },
                FakeSettlementPlan::Interrupted => RunSettlement::Interrupted,
                FakeSettlementPlan::Pending => RunSettlement::Pending,
                FakeSettlementPlan::WrittenWithoutCompletion => {
                    RunSettlement::WrittenWithoutCompletion
                }
            })
        })
    }

    fn summary_source(&self, _: RunObservationContext) -> DeliveryFuture<'_, RunSummarySource> {
        Box::pin(async { Err(DeliveryContractError::InvalidEvidence) })
    }

    fn request_stop<'a>(
        &'a self,
        _: RunObservationContext,
        _: &'a dyn RunEvidenceSink,
    ) -> DeliveryFuture<'a, StopRequestOutcome> {
        Box::pin(async { Ok(StopRequestOutcome::Unsupported) })
    }

    fn reconcile_run(&self, _: RunObservationContext) -> DeliveryFuture<'_, RunReconciliation> {
        Box::pin(async { Ok(RunReconciliation::StillUnknown) })
    }
}

#[path = "scheduled_run_route_fixture.rs"]
pub mod fixture;
pub use fixture::{apply_push_handoff, provider_worker_fixture};
