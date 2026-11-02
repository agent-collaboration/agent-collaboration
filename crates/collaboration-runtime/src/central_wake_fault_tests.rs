//! Queue intent and actual native acceptance are distinct facts at the debug wake boundary.
use super::WakeDeliverySender;
use crate::AutomationConfigurationHandle;
use agent_automation::{
    DeliveryStatus, NativeEffectEvidence, ProviderAcpEffectEvidence, ProviderSettlementEffect,
    RouteEffectEvidence, SubmissionEffect,
};
use automation_storage::{
    AutomationFaultEvidence, AutomationFaultObserver, AutomationFaultPoint, AutomationStore,
    CentralFiringRequest, CentralLocalWork,
};
use collaboration_contracts::{
    AttemptEvidenceSink, AttemptReconciliation, AttemptReconciliationContext,
    DeliveryContractError, DeliveryFuture, SessionMessageDelivery,
};
use collaboration_protocol::{
    AcceptedResumeEffect, AutomationContent, BatchId, CodexGeneration, DeliveryClientReceipt,
    DeliveryOutcome, DeliveryReceipt, FiringEntry, FiringId, FiringOutcome, MessageInputKind,
    MessageRepresentation, NativeSendAcceptance, NativeSendReceipt, OperationId,
    SessionReachability, SessionRef,
};
use serde_json::json;
use std::{future::Future, path::PathBuf, pin::Pin, sync::Arc};
use tokio::sync::Mutex;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Copy)]
enum DeliveryCase {
    ProviderRouterQueue,
    ProviderNativeSubmission,
    CodexNativeQueue,
}
struct NativeBoundaryDelivery(DeliveryCase);
impl SessionMessageDelivery for NativeBoundaryDelivery {
    fn deliver<'a>(
        &'a self,
        request: collaboration_contracts::layer_zero::DeliveryRequest,
        evidence: &'a dyn AttemptEvidenceSink,
    ) -> DeliveryFuture<'a, collaboration_contracts::NativeDeliveryResult> {
        Box::pin(async move {
            let generation: CodexGeneration = serde_json::from_value(json!({
                "serviceEpoch":"00000000-0000-4000-8000-000000000001","generation":1
            }))
            .map_err(|_| DeliveryContractError::InvalidEvidence)?;
            if matches!(self.0, DeliveryCase::CodexNativeQueue) {
                let mut native: NativeEffectEvidence<SessionRef, CodexGeneration> = serde_json::from_value(json!({
                    "target":request.target,"generation":generation,
                    "clientUserMessageId":request.payload.push_id.as_str(),"nativeTurnId":null,"nativeSubmissionId":null,
                    "allocation":"notRequested","resume":"notRequested","submission":"dispatching","cessation":"notApplicable"
                })).map_err(|_| DeliveryContractError::InvalidEvidence)?;
                evidence.record(native.clone().into()).await?;
                native.submission = SubmissionEffect::Accepted;
                native.native_submission_id = Some("fixture-native-queue".to_owned());
                evidence.record(native.into()).await?;
                let receipt = NativeSendReceipt {
                    target: request.target,
                    generation,
                    input_kind: MessageInputKind::Agent,
                    representation: MessageRepresentation::DeclaredAgentText,
                    client_user_message_id: request
                        .payload
                        .push_id
                        .as_str()
                        .to_owned()
                        .try_into()
                        .map_err(|_| DeliveryContractError::InvalidEvidence)?,
                    resume_effect: AcceptedResumeEffect::NotRequested,
                    permission_differences: Vec::new(),
                    acceptance: NativeSendAcceptance::QueueAccepted {
                        submission_id: "fixture-native-queue"
                            .to_owned()
                            .try_into()
                            .map_err(|_| DeliveryContractError::InvalidEvidence)?,
                    },
                };
                return Ok(collaboration_contracts::scripted_delivery_result(
                    receipt.into(),
                ));
            }
            let queued = matches!(self.0, DeliveryCase::ProviderRouterQueue);
            let operation_id: OperationId = request
                .attempt
                .as_str()
                .to_owned()
                .try_into()
                .map_err(|_| DeliveryContractError::InvalidEvidence)?;
            let mut provider = ProviderAcpEffectEvidence {
                target: request.target,
                generation,
                binding: "fixture-provider-binding"
                    .to_owned()
                    .try_into()
                    .map_err(|_| DeliveryContractError::InvalidEvidence)?,
                attempt_id: request.attempt,
                submission: if queued {
                    SubmissionEffect::RouterQueued
                } else {
                    SubmissionEffect::Dispatching
                },
                settlement: ProviderSettlementEffect::NotObserved,
            };
            evidence
                .record(RouteEffectEvidence::ProviderAcp(provider.clone()))
                .await?;
            if !queued {
                provider.submission = SubmissionEffect::Accepted;
                evidence
                    .record(RouteEffectEvidence::ProviderAcp(provider))
                    .await?;
            }
            Ok(collaboration_contracts::scripted_delivery_result(
                DeliveryReceipt {
                    outcome: if queued {
                        DeliveryOutcome::Queued
                    } else {
                        DeliveryOutcome::Started
                    },
                    reachability: Some(SessionReachability::ProviderAcp),
                    client: Some(DeliveryClientReceipt::ProviderAcp { operation_id }),
                },
            ))
        })
    }
    fn reconcile_attempt(
        &self,
        _: AttemptReconciliationContext,
    ) -> DeliveryFuture<'_, AttemptReconciliation> {
        Box::pin(async { Ok(AttemptReconciliation::StillUnknown) })
    }
}

struct NativePointRecorder {
    database: PathBuf,
    firing_id: FiringId,
    delivery_id: agent_automation::DeliveryId,
    calls: Mutex<Vec<Result<AutomationFaultEvidence, String>>>,
}
impl NativePointRecorder {
    async fn observe_before_outcome(
        &self,
        point: AutomationFaultPoint,
        evidence: AutomationFaultEvidence,
    ) -> TestResult<AutomationFaultEvidence> {
        if point != AutomationFaultPoint::NativeInputAcceptedBeforeOutcomeCommit {
            return Err("unexpected native observation point".into());
        }
        let mut store = AutomationStore::open(&self.database).await?;
        let delivery = store
            .read_delivery::<SessionRef, CodexGeneration, serde_json::Value>(&self.delivery_id)
            .await?;
        let receipt = store
            .read_central_firing(&self.firing_id)
            .await?
            .ok_or("durable firing receipt absent")?;
        store.close().await?;
        if delivery.status != DeliveryStatus::Dispatching
            || !matches!(receipt.outcome, FiringOutcome::Received)
        {
            return Err("native observation ran after durable outcome/report committed".into());
        }
        Ok(evidence)
    }
}
impl AutomationFaultObserver for NativePointRecorder {
    fn hold(
        &self,
        point: AutomationFaultPoint,
        evidence: AutomationFaultEvidence,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let observed = self
                .observe_before_outcome(point, evidence)
                .await
                .map_err(|error| error.to_string());
            self.calls.lock().await.push(observed);
        })
    }
}

#[tokio::test]
async fn provider_router_queue_does_not_claim_native_acceptance() -> TestResult {
    Box::pin(exercise_native_boundary(DeliveryCase::ProviderRouterQueue)).await
}
#[tokio::test]
async fn provider_native_submission_reaches_the_point_before_local_outcome() -> TestResult {
    Box::pin(exercise_native_boundary(
        DeliveryCase::ProviderNativeSubmission,
    ))
    .await
}
#[tokio::test]
async fn codex_native_queue_acceptance_reaches_the_point_before_local_outcome() -> TestResult {
    Box::pin(exercise_native_boundary(DeliveryCase::CodexNativeQueue)).await
}

async fn exercise_native_boundary(case: DeliveryCase) -> TestResult {
    // Arrange: real central admission and interaction handoff; only native delivery is a stand-in.
    let root = tempfile::tempdir()?;
    let database = root.path().join("automation.sqlite");
    let store = Arc::new(Mutex::new(AutomationStore::open(&database).await?));
    let interactions = Arc::new(
        interaction_storage::InteractionStore::open(root.path().join("interaction.sqlite")).await?,
    );
    let now_ms = chrono::Utc::now().timestamp_millis();
    let due_ms = now_ms.div_euclid(300_000) * 300_000;
    let due = chrono::DateTime::from_timestamp_millis(due_ms)
        .ok_or("due timestamp")?
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let expiry = chrono::DateTime::from_timestamp_millis(due_ms + 86_400_000)
        .ok_or("expiry timestamp")?
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let endpoint = if matches!(case, DeliveryCase::CodexNativeQueue) {
        "codex-local"
    } else {
        "claude-local"
    };
    let firing: FiringEntry = serde_json::from_value(json!({
        "firingId":uuid::Uuid::now_v7().to_string(),"automationId":uuid::Uuid::now_v7().to_string(),
        "dueAt":due_ms,"revision":uuid::Uuid::now_v7().to_string(),
        "content":{"contentId":uuid::Uuid::now_v7().to_string(),"revisionId":uuid::Uuid::now_v7().to_string(),"digest":"a".repeat(64)},
        "work":{"kind":"wake","createdBy":{"kind":"owner"},"timing":{"kind":"at","at":due},
            "anchorAt":due,"createdAt":due,"expiresAt":expiry,
            "target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":endpoint},"sessionId":"central-wake-recipient"},
            "origin":{"kind":"router"},"delivery":"queue","generationGuard":null}
    }))?;
    let batch_id = BatchId::generate();
    let receipt = store
        .lock()
        .await
        .admit_central_firing(&CentralFiringRequest {
            batch_id: batch_id.clone(),
            firing: firing.clone(),
            content: AutomationContent {
                reference: firing.content.clone(),
                text: "Native acceptance fixture".to_owned().try_into()?,
            },
            now_ms,
        })
        .await?;
    let CentralLocalWork::Wake { delivery_id } = receipt.local_work else {
        return Err("central wake did not admit a delivery".into());
    };
    crate::PushHandoffWorker::new(store.clone(), interactions.clone())
        .apply_pending()
        .await?;
    let recorder = Arc::new(NativePointRecorder {
        database,
        firing_id: firing.firing_id.clone(),
        delivery_id: delivery_id.clone(),
        calls: Mutex::new(Vec::new()),
    });
    store.lock().await.set_automation_faults(recorder.clone());
    let sender = WakeDeliverySender {
        interactions: interactions.clone(),
        delivery: Arc::new(NativeBoundaryDelivery(case)),
        configuration: AutomationConfigurationHandle::default(),
        machine_identity: collaboration_contracts::MachineIdentity::new(
            "00000000-0000-4000-8000-000000000001"
                .to_owned()
                .try_into()?,
            None,
        )?,
    };

    // Act: completion is the causal boundary for proving no false native observation.
    Box::pin(sender.dispatch(store.clone(), delivery_id.clone())).await?;

    // Assert: real outcome commits as before, with a native point only for reached Accepted evidence.
    let delivered = store
        .lock()
        .await
        .read_delivery::<SessionRef, CodexGeneration, serde_json::Value>(&delivery_id)
        .await?;
    if delivered.status != DeliveryStatus::Accepted {
        return Err("queue or native delivery behavior changed".into());
    }
    let calls = recorder
        .calls
        .lock()
        .await
        .clone()
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    let expected = usize::from(!matches!(case, DeliveryCase::ProviderRouterQueue));
    if calls.len() != expected {
        return Err(format!(
            "native point count: expected {expected}, observed {}",
            calls.len()
        )
        .into());
    }
    if let Some(evidence) = calls.first() {
        if evidence.batch_id.as_ref() != Some(&batch_id)
            || evidence.firing_id.as_ref() != Some(&firing.firing_id)
            || evidence.delivery_attempt_id.as_ref()
                != delivered
                    .attempt
                    .as_ref()
                    .map(|attempt| &attempt.attempt_id)
            || evidence.native_turn_id.is_some()
        {
            return Err("native point lost reached identities or fabricated a turn".into());
        }
        if matches!(case, DeliveryCase::ProviderNativeSubmission) {
            let expected_operation: OperationId = delivered
                .attempt
                .as_ref()
                .ok_or("attempt")?
                .attempt_id
                .as_str()
                .to_owned()
                .try_into()?;
            if evidence.operation_id.as_ref() != Some(&expected_operation) {
                return Err("provider point did not carry the reached operation".into());
            }
        } else if evidence.operation_id.is_some() {
            return Err("Codex native point fabricated a provider operation".into());
        }
    }
    drop(sender);
    Arc::try_unwrap(store)
        .map_err(|_| "automation store still shared")?
        .into_inner()
        .close()
        .await?;
    Arc::try_unwrap(interactions)
        .map_err(|_| "interaction store still shared")?
        .close()
        .await?;
    Ok(())
}
