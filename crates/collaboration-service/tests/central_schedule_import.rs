//! The public JSONL carrier validates before dispatch and preserves uncertain Fleet commits.
use collaboration_client::{CentralCatalogFailure, ScheduleClientError};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, OperationId, ScheduleImportRequest,
};
use collaboration_runtime::{CentralAutomationError, CentralAutomationTransport};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
#[path = "support/central_catalog_fixture.rs"]
mod fixture;
use fixture::{CatalogFixture, ProofResult};

#[derive(Default)]
struct ImportEvidence {
    calls: AtomicUsize,
    dropped: AtomicBool,
    committed: Mutex<Option<serde_json::Value>>,
}
struct ObservedImport {
    inner: Arc<dyn CentralAutomationTransport>,
    evidence: Arc<ImportEvidence>,
    lose_first_reply: bool,
}
impl CentralAutomationTransport for ObservedImport {
    fn call(
        &self,
        request: AutomationCall,
    ) -> Pin<Box<dyn Future<Output = Result<AutomationAnswer, CentralAutomationError>> + Send + '_>>
    {
        Box::pin(async move {
            let importing = matches!(request, AutomationCall::SchedulePackageImport(_));
            if importing {
                self.evidence.calls.fetch_add(1, Ordering::SeqCst);
            }
            let answer = self.inner.call(request).await?;
            if importing
                && self.lose_first_reply
                && !self.evidence.dropped.swap(true, Ordering::SeqCst)
            {
                let value = serde_json::to_value(&answer)
                    .map_err(|_| CentralAutomationError::InvalidReply)?;
                *self
                    .evidence
                    .committed
                    .lock()
                    .map_err(|_| CentralAutomationError::InvalidReply)? = Some(value);
                return Err(CentralAutomationError::OutcomeUnknown);
            }
            Ok(answer)
        })
    }
}
fn package_request() -> ProofResult<ScheduleImportRequest> {
    let instruction = agent_automation::InstructionId::generate();
    let package = agent_automation::PortableSchedulePackage::<
        collaboration_protocol::SessionRef,
        collaboration_protocol::EndpointRef,
    > {
        schedule_id: agent_automation::ScheduleId::generate(),
        source_change_id: "source-change".into(),
        instruction: agent_automation::PortableInstruction {
            instruction_id: instruction.clone(),
            text: "Portable reply-loss proof".to_owned().try_into()?,
            source_revision_id: "source-revision".into(),
        },
        definition: agent_automation::ScheduleDefinition {
            instruction_id: instruction,
            timing: agent_automation::TimingRule::Interval { seconds: 600 },
            enabled: false,
            destination: agent_automation::ExecutionDestination::Unprepared,
            execution_timeout_seconds: None,
            model: Some("gpt-6-luna".into()),
            effort: Some("low".into()),
        },
        continuity: None,
    };
    Ok(ScheduleImportRequest {
        operation_id: OperationId::generate(),
        package_utf8: agent_automation::encode_schedule_package(&package, 1_048_576)?,
        overwrite: false,
    })
}

#[tokio::test]
async fn lost_committed_import_answer_requires_explicit_same_operation_replay() -> ProofResult {
    let evidence = Arc::new(ImportEvidence::default());
    let captured = evidence.clone();
    let fixture = CatalogFixture::open(
        move |inner| {
            Arc::new(ObservedImport {
                inner,
                evidence: captured,
                lose_first_reply: true,
            })
        },
        false,
    )
    .await?;
    let proof: ProofResult = async {
        let client = fixture.api.client("package-reply-loss").await?;
        if fixture.application.automation().automation_status().await.storage_available {
            return Err("reply-loss proof unexpectedly has a local definition store".into());
        }
        let request = package_request()?;
        // Act: the injected carrier loss happens only after the real Fleet commit answer.
        let failed = client.import_schedule(request.clone()).await;
        if !matches!(failed, Err(ScheduleClientError::Catalog(CentralCatalogFailure::OutcomeUnknown { operation_id, .. })) if operation_id == request.operation_id)
            || evidence.calls.load(Ordering::SeqCst) != 1 {
            return Err("unknown import was relabeled unsent, lost its operation ID, or automatically retried".into());
        }
        let committed = evidence.committed.lock().map_err(|_| "commit evidence poisoned")?.clone().ok_or("actual central commit missing")?;
        let replay = serde_json::to_value(client.import_schedule(request.clone()).await?)?;
        let mut public_commit = committed.get("value").cloned().ok_or("mutation commit missing")?;
        let schedule = public_commit.pointer_mut("/automation/value").and_then(serde_json::Value::as_object_mut).ok_or("committed Schedule missing")?;
        // The public catalog projection omits precisely the two machine-local occupancy fields.
        if schedule.remove("activeRunId") != Some(serde_json::Value::Null)
            || schedule.remove("waitingRunId") != Some(serde_json::Value::Null) {
            return Err("central import invented machine-local Run occupancy".into());
        }
        if replay != public_commit || evidence.calls.load(Ordering::SeqCst) != 2 {
            return Err("explicit same-ID replay did not return the original central mutation result".into());
        }
        let changed = client.import_schedule(ScheduleImportRequest { overwrite: true, ..request.clone() }).await;
        if !matches!(changed, Err(ScheduleClientError::Catalog(CentralCatalogFailure::Refused { operation_id: Some(actual), error, .. }))
            if actual == request.operation_id && matches!(&*error, collaboration_protocol::AutomationRefusal::OperationConflict { operation_id } if operation_id == &request.operation_id)) {
            return Err("changed import replay lost the catalog operation conflict".into());
        }
        let AutomationAnswer::List { automations, next } = fixture.transport.call(AutomationCall::List(
            collaboration_protocol::AutomationPageRequest { cursor: None, limit: 100.try_into()? },
        )).await? else { return Err("actual central catalog page missing".into()); };
        if automations.len() != 1 || next.is_some()
            || Some(&serde_json::to_value(&automations[0])?) != committed.pointer("/value/automation") {
            return Err("unknown import or explicit replay duplicated or changed central definition truth".into());
        }
        Ok(())
    }.await;
    let stopped = fixture.close().await;
    proof?;
    stopped
}

#[tokio::test]
async fn corrupt_jsonl_footer_is_refused_before_any_catalog_dispatch() -> ProofResult {
    let evidence = Arc::new(ImportEvidence::default());
    let captured = evidence.clone();
    let fixture = CatalogFixture::open(
        move |inner| {
            Arc::new(ObservedImport {
                inner,
                evidence: captured,
                lose_first_reply: false,
            })
        },
        false,
    )
    .await?;
    let proof: ProofResult = async {
        let client = fixture.api.client("package-integrity").await?;
        let mut request = package_request()?;
        request.package_utf8 = request.package_utf8.replace("Portable reply-loss proof", "Changed without updating the footer");
        let failed = client.import_schedule(request.clone()).await;
        if !matches!(failed, Err(ScheduleClientError::Rejected(error))
            if error.operation_id.as_ref() == Some(&request.operation_id)
                && error.field.as_deref() == Some("packageUtf8")
                && matches!(error.stage, collaboration_protocol::ScheduleFailureStage::Validation)
                && matches!(error.effects, collaboration_protocol::ScheduleEffects::Local { mutation: collaboration_protocol::LocalMutationState::None })) {
            return Err("corrupt package lost its typed pre-dispatch validation failure".into());
        }
        if evidence.calls.load(Ordering::SeqCst) != 0 {
            return Err("damaged JSONL was dispatched to the catalog".into());
        }
        Ok(())
    }.await;
    let stopped = fixture.close().await;
    proof?;
    stopped
}
