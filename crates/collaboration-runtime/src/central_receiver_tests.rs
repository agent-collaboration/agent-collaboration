#![expect(
    clippy::panic_in_result_fn,
    reason = "Assertions reject wrong behavior; Result propagates real fixture and storage failures."
)]
//! S4 conformance stand-in: real receipt storage stays on the receiving side of the seam.
use super::central_automation_receiver::CentralAutomationReceiver;
use crate::{CentralAutomationClock, CentralAutomationError, CentralAutomationTransport};
use automation_storage::AutomationStore;
use collaboration_protocol::{
    AutomationAnswer, AutomationBatch, AutomationBatches, AutomationCall, AutomationContent,
    BatchId, CodexGeneration, ContentReference, EndpointRef, ExecutionDestination, FiringEntry,
    FiringOutcome, FiringWork, FrozenAutomationBatch, MachineWorkProtocol, OperationId, SessionRef,
    UuidIdentity, WorkEntry,
};
use sha2::{Digest, Sha256};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::sync::Mutex;
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
struct FixedClock;
impl CentralAutomationClock for FixedClock {
    fn now_ms(&self) -> i64 {
        400_000
    }
    fn wait_until(&self, _: i64) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(std::future::pending())
    }
}
struct CustodyStandIn {
    batch: FrozenAutomationBatch,
    contents: Vec<AutomationContent>,
    acknowledged: AtomicUsize,
}
impl CentralAutomationTransport for CustodyStandIn {
    fn call(
        &self,
        request: AutomationCall,
    ) -> Pin<Box<dyn Future<Output = Result<AutomationAnswer, CentralAutomationError>> + Send + '_>>
    {
        Box::pin(async move {
            Ok(match request {
                AutomationCall::PullAutomationBatches(pull) => {
                    assert_eq!(
                        pull.work_protocols,
                        vec![MachineWorkProtocol::MachineWorkV1]
                    );
                    AutomationAnswer::Batches(AutomationBatches {
                        batches: vec![self.batch.clone()],
                        next: None,
                    })
                }
                AutomationCall::FetchContents(_) => {
                    AutomationAnswer::Contents(self.contents.clone())
                }
                AutomationCall::AckAutomationBatches(ack) => {
                    let first = ack
                        .batches
                        .first()
                        .ok_or(CentralAutomationError::InvalidReply)?;
                    assert_eq!(first.batch_id, self.batch.batch_id);
                    assert_eq!(first.digest, self.batch.digest);
                    self.acknowledged.fetch_add(1, Ordering::SeqCst);
                    AutomationAnswer::Acknowledged
                }
                _ => return Err(CentralAutomationError::InvalidReply),
            })
        })
    }
}
fn fixture() -> TestResult<(UuidIdentity, CustodyStandIn)> {
    let service_id: UuidIdentity = "00000000-0000-4000-8000-000000000002"
        .to_owned()
        .try_into()?;
    let text = "Frozen source revision";
    let reference: ContentReference = serde_json::from_value(
        serde_json::json!({"contentId":uuid::Uuid::now_v7().to_string(),"revisionId":uuid::Uuid::now_v7().to_string(),"digest":format!("{:x}",Sha256::digest(text.as_bytes()))}),
    )?;
    let mut entries = Vec::new();
    for _ in 0..3 {
        let firing: FiringEntry = serde_json::from_value(serde_json::json!({
            "firingId":uuid::Uuid::now_v7().to_string(),"automationId":uuid::Uuid::now_v7().to_string(),"dueAt":300000,"revision":uuid::Uuid::now_v7().to_string(),"content":reference,
            "work":{"kind":"schedule","definition":{"instructionId":uuid::Uuid::now_v7().to_string(),"timing":{"kind":"interval","seconds":600},"enabled":true,
                "destination":{"kind":"freshEachRun","endpoint":{"serviceId":service_id,"endpointId":"claude-local"},"cwd":"/work/receiver"},"executionTimeoutSeconds":120,"model":"claude-haiku-5-5","effort":"low"}}
        }))?;
        entries.push(WorkEntry::Firing(firing));
    }
    // Different schedules can reference one frozen content revision only when they share its instruction.
    if let WorkEntry::Firing(first) = entries.first().ok_or("entry absent")? {
        let FiringWork::Schedule { definition: first } = &first.work else {
            return Err("wrong fixture".into());
        };
        let instruction_id = first.instruction_id.clone();
        for entry in &mut entries {
            if let WorkEntry::Firing(FiringEntry {
                work: FiringWork::Schedule { definition },
                ..
            }) = entry
            {
                definition.instruction_id = instruction_id.clone();
            }
        }
    }
    let batch = AutomationBatch {
        batch_id: BatchId::generate(),
        service_id: service_id.clone(),
        entries: entries.try_into()?,
    };
    let bytes = serde_json::to_vec(&batch)?;
    let frozen = FrozenAutomationBatch {
        batch_id: batch.batch_id,
        digest: format!("{:x}", Sha256::digest(&bytes)).try_into()?,
        frozen_bytes: bytes,
    };
    Ok((
        service_id,
        CustodyStandIn {
            batch: frozen,
            contents: vec![AutomationContent {
                reference,
                text: text.to_owned().try_into()?,
            }],
            acknowledged: AtomicUsize::new(0),
        },
    ))
}
#[tokio::test]
async fn interrupted_after_two_entries_reopens_and_acknowledges_three_receipts_once() -> TestResult
{
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    let (service, transport) = fixture()?;
    let transport = Arc::new(transport);
    let store = Arc::new(Mutex::new(AutomationStore::open(&path).await?));
    let faults = Arc::new(crate::DebugAutomationFaultController::default());
    let arm = faults
        .arm(
            automation_storage::AutomationFaultPoint::EntryAdmissionCommitted,
            crate::AutomationFaultSelector {
                completed_entry_ordinal: Some(2),
                ..Default::default()
            },
        )
        .await?;
    store.lock().await.set_automation_faults(faults.clone());
    let receiver = Arc::new(CentralAutomationReceiver::new(
        store.clone(),
        transport.clone(),
        Arc::new(FixedClock),
        service.clone(),
    ));
    let running = receiver.clone();
    let task = tokio::spawn(async move { running.receive_pending().await });
    tokio::time::timeout(std::time::Duration::from_secs(3), faults.wait_held(&arm)).await??;
    assert_eq!(transport.acknowledged.load(Ordering::SeqCst), 0);
    task.abort();
    assert!(task.await.is_err_and(|error| error.is_cancelled()));
    drop(receiver);
    let store = Arc::try_unwrap(store)
        .map_err(|_| "store still owned")?
        .into_inner();
    store.close().await?;
    let reopened = Arc::new(Mutex::new(AutomationStore::open(&path).await?));
    let receiver = CentralAutomationReceiver::new(
        reopened.clone(),
        transport.clone(),
        Arc::new(FixedClock),
        service,
    );
    receiver.receive_pending().await?;
    receiver.receive_pending().await?;
    assert_eq!(transport.acknowledged.load(Ordering::SeqCst), 2);
    let mut connection = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(&path),
    )
    .await?;
    use sqlx::Connection;
    let counts:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM workflow_runs),(SELECT count(*) FROM firing_receipts),(SELECT count(*) FROM central_report_outbox)").fetch_one(&mut connection).await?;
    assert_eq!(counts, (3, 3, 3));
    connection.close().await?;
    Ok(())
}

#[tokio::test]
async fn central_run_starts_with_the_frozen_revision_after_the_local_head_changes() -> TestResult {
    use crate::scheduled_route_fixture::{
        FakeRouteKind, FakeScheduledExecution, FakeSettlementPlan, FakeSubmissionPlan,
    };
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    let (service, transport) = fixture()?;
    let batch: AutomationBatch = serde_json::from_slice(&transport.batch.frozen_bytes)?;
    let WorkEntry::Firing(mut firing) = batch
        .entries
        .entries()
        .first()
        .ok_or("entry absent")?
        .clone()
    else {
        return Err("wrong fixture".into());
    };
    let target: SessionRef = serde_json::from_value(
        serde_json::json!({"endpoint":{"serviceId":service,"endpointId":"claude-local"},"sessionId":"central-worker"}),
    )?;
    let FiringWork::Schedule { definition } = &mut firing.work else {
        return Err("wrong fixture".into());
    };
    definition.destination = ExecutionDestination::OwnedThread {
        target: target.clone(),
        cwd: directory.path().to_string_lossy().into_owned(),
    };
    let instruction_id = definition.instruction_id.clone();
    let frozen_revision = firing.content.revision_id.clone();
    let mut store = AutomationStore::open(&path).await?;
    let receipt = store
        .admit_central_firing(&automation_storage::CentralFiringRequest {
            batch_id: batch.batch_id,
            firing: firing.clone(),
            content: transport.contents.first().ok_or("content absent")?.clone(),
            now_ms: 400_000,
        })
        .await?;
    store
        .update_instruction(&automation_storage::InstructionUpdate {
            operation_id: OperationId::generate(),
            instruction_id,
            expected_revision_id: frozen_revision.clone(),
            text: "Later edit must not replace captured work"
                .to_owned()
                .try_into()?,
            now_ms: 400_001,
        })
        .await?;
    let automation_storage::CentralLocalWork::Schedule { run_id } = receipt.local_work else {
        return Err("run absent".into());
    };
    let store = Arc::new(Mutex::new(store));
    let interactions = Arc::new(
        interaction_storage::InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await?,
    );
    let execution = Arc::new(FakeScheduledExecution {
        kind: FakeRouteKind::Provider,
        target: target.clone(),
        interactions: interactions.clone(),
        observed_push_ids: Arc::new(std::sync::Mutex::new(Vec::new())),
        generation: serde_json::from_value(
            serde_json::json!({"serviceEpoch":service,"generation":1}),
        )?,
        attempt_id: agent_automation::AttemptId::generate(),
        submissions: AtomicUsize::new(1),
        submission_plan: FakeSubmissionPlan::Accept,
        settlement_plan: FakeSettlementPlan::Pending,
    });
    let worker = crate::scheduled_run_worker::ScheduledRunWorker {
        store: store.clone(),
        interactions,
        execution: execution.clone(),
        backend: None,
        configuration: crate::AutomationConfigurationHandle::default(),
        machine_identity: collaboration_contracts::MachineIdentity::new(
            service.clone(),
            Some("Central receiver fixture"),
        )?,
    };
    Box::pin(worker.step(run_id.clone())).await?;
    Box::pin(worker.step(run_id.clone())).await?;
    let record=store.lock().await.read_run::<SessionRef,EndpointRef,CodexGeneration,crate::stored_run_receipt::StoredRunReceipt>(&run_id).await?;
    assert_eq!(record.phase, agent_automation::RunPhase::Executing);
    let inputs = record.inputs.ok_or("captured input absent")?;
    assert_eq!(inputs.instruction_revision_id, frozen_revision);
    assert_eq!(inputs.instruction_text.as_str(), "Frozen source revision");
    let receipt = store
        .lock()
        .await
        .read_central_firing(&firing.firing_id)
        .await?
        .ok_or("receipt absent")?;
    assert_eq!(u64::from(receipt.report_seq), 2);
    assert!(
        matches!(receipt.outcome,FiringOutcome::RunStarted { run_id:actual } if actual==run_id)
    );
    assert_eq!(execution.submissions.load(Ordering::SeqCst), 2);
    let push_id = execution
        .observed_push_ids
        .lock()
        .map_err(|_| "push observer poisoned")?
        .last()
        .cloned()
        .ok_or("submitted push absent")?;
    let actual_push = worker
        .interactions
        .get_push_record(&push_id)
        .await?
        .ok_or("submitted push record absent")?;
    assert_eq!(actual_push.body.as_deref(), Some("Frozen source revision"));
    Ok(())
}
