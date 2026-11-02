//! Real central Bind, durable Router receipt, lost reply and native-effect boundary controls.
use automation_storage::{
    AutomationStore, ExternalAdmission, PreparationFailureDisposition, PreparationFailureRecord,
    PreparationIntent, StoredOperationState,
};
use collaboration_protocol::*;
use collaboration_runtime::{CentralAutomationError, CentralAutomationTransport};
use collaboration_service::{ApplicationFixture, CentralCatalogFailure, ScheduleCommandFailure};
use serde_json::json;
use sqlx::{Connection, Row};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
#[path = "support/central_catalog_fixture.rs"]
mod fixture;
#[path = "support/central_preparation_execution.rs"]
mod native;
use fixture::{CatalogFixture, ProofResult};

struct LoseFirstBindAnswer {
    central: Arc<dyn CentralAutomationTransport>,
    path: PathBuf,
    requests: Mutex<Vec<String>>,
    unavailable_reads: AtomicBool,
}
impl CentralAutomationTransport for LoseFirstBindAnswer {
    fn call(
        &self,
        request: AutomationCall,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<AutomationAnswer, CentralAutomationError>>
                + Send
                + '_,
        >,
    > {
        Box::pin(async move {
            let AutomationCall::ScheduleBindPrepared(bind) = &request else {
                if self.unavailable_reads.load(Ordering::SeqCst) {
                    return Err(CentralAutomationError::Unavailable);
                }
                return self.central.call(request).await;
            };
            let encoded =
                serde_json::to_string(bind).map_err(|_| CentralAutomationError::InvalidWork)?;
            let mut database = sqlx::SqliteConnection::connect_with(
                &sqlx::sqlite::SqliteConnectOptions::new().filename(&self.path),
            )
            .await
            .map_err(automation_storage::StorageError::from)?;
            let row=sqlx::query("SELECT operation_status,central_bind_request_json,effect_evidence_json,final_result_json FROM operation_receipts WHERE operation_id=?")
                .bind(bind.operation_id.as_str()).fetch_one(&mut database).await.map_err(automation_storage::StorageError::from)?;
            let evidence: serde_json::Value = serde_json::from_str(
                &row.try_get::<String, _>("effect_evidence_json")
                    .map_err(automation_storage::StorageError::from)?,
            )
            .map_err(|_| CentralAutomationError::InvalidWork)?;
            if row
                .try_get::<String, _>("operation_status")
                .map_err(automation_storage::StorageError::from)?
                != "inProgress"
                || row
                    .try_get::<String, _>("central_bind_request_json")
                    .map_err(automation_storage::StorageError::from)?
                    != encoded
                || row
                    .try_get::<Option<String>, _>("final_result_json")
                    .map_err(automation_storage::StorageError::from)?
                    .is_some()
                || evidence.pointer("/target")
                    != Some(
                        &serde_json::to_value(&bind.bound_thread.target)
                            .map_err(|_| CentralAutomationError::InvalidWork)?,
                    )
            {
                return Err(CentralAutomationError::InvalidWork);
            }
            database
                .close()
                .await
                .map_err(automation_storage::StorageError::from)?;
            let attempt = {
                let mut calls = self
                    .requests
                    .lock()
                    .map_err(|_| CentralAutomationError::InvalidWork)?;
                calls.push(encoded);
                calls.len()
            };
            let answer = self.central.call(request).await?;
            if attempt == 1 {
                Err(CentralAutomationError::OutcomeUnknown)
            } else {
                Ok(answer)
            }
        })
    }
}
async fn setup() -> ProofResult<(
    CatalogFixture,
    ScheduleSnapshot,
    Arc<native::PreparationExecution>,
    Arc<LoseFirstBindAnswer>,
)> {
    let fixture = CatalogFixture::open(|transport| transport, true).await?;
    let client = fixture.api.client("prepare-fixture").await?;
    if !matches!(
        fixture
            .transport
            .call(AutomationCall::PullAutomationBatches(
                PullAutomationBatches {
                    after: None,
                    max_bytes: 1_048_576,
                    work_protocols: vec![MachineWorkProtocol::MachineWorkV1],
                }
            ))
            .await?,
        AutomationAnswer::Batches(_)
    ) {
        return Err("fixture did not advertise MachineWorkV1".into());
    }
    let instruction = client
        .create_instruction(InstructionCreateParams {
            operation_id: OperationId::generate(),
            text: "original central instruction".to_owned().try_into()?,
        })
        .await?;
    let created=client.create_schedule(serde_json::from_value(json!({"operationId":OperationId::generate(),"definition":{
        "instructionId":instruction.instruction_id,"timing":{"kind":"interval","seconds":600},"enabled":false,
        "destination":{"kind":"unprepared"},"executionTimeoutSeconds":null,"model":"gpt-6-luna","effort":"low"
    }}))?).await?;
    let schedule: ScheduleSnapshot = match fixture
        .transport
        .call(AutomationCall::ScheduleShow(ScheduleShowRequest {
            schedule_id: created.schedule_id,
        }))
        .await?
    {
        AutomationAnswer::Schedule(value) => value,
        _ => return Err("missing real central Schedule".into()),
    };
    let native = Arc::new(native::PreparationExecution {
        target: serde_json::from_value(
            json!({"endpoint":{"serviceId":fixture::SERVICE,"endpointId":"codex-local"},"sessionId":"prepared-native-target"}),
        )?,
        creations: AtomicUsize::new(0),
        inputs: Mutex::new(Vec::new()),
    });
    let transport = Arc::new(LoseFirstBindAnswer {
        central: fixture.transport.clone(),
        path: fixture.api.directory().join("automation.sqlite"),
        requests: Mutex::new(Vec::new()),
        unavailable_reads: AtomicBool::new(false),
    });
    Ok((fixture, schedule, native, transport))
}
fn request(schedule_id: ScheduleId) -> ProofResult<SchedulePrepareRequest> {
    Ok(serde_json::from_value(
        json!({"operationId":OperationId::generate(),"scheduleId":schedule_id,
        "destination":{"kind":"fresh","endpoint":{"serviceId":fixture::SERVICE,"endpointId":"codex-local"},"cwd":"/isolated-prepare"}}),
    )?)
}
fn application(
    store: Arc<tokio::sync::Mutex<AutomationStore>>,
    native: Arc<native::PreparationExecution>,
    transport: Arc<LoseFirstBindAnswer>,
) -> ProofResult<collaboration_service::CollaborationApplication> {
    Ok(ApplicationFixture::new(fixture::SERVICE, fixture::SERVICE)?
        .with_automation_store(store)
        .with_scheduled_run_execution(native)
        .with_catalog(Some(transport))
        .build())
}
fn require_unknown(result: Result<ScheduleSnapshot, ScheduleCommandFailure>) -> ProofResult {
    if matches!(result,Err(ScheduleCommandFailure::Catalog(error)) if matches!(*error,CentralCatalogFailure::OutcomeUnknown {..}))
    {
        Ok(())
    } else {
        Err("lost real Bind answer did not remain a central outcomeUnknown".into())
    }
}
#[tokio::test]
async fn lost_bind_answer_reopens_and_replays_exact_request_without_native_creation() -> ProofResult
{
    let (fixture, schedule, native, transport) = setup().await?;
    let proof:ProofResult=async {
        let request=request(schedule.schedule_id.clone())?;
        let app=application(fixture.store.as_ref().ok_or("missing store")?.clone(),native.clone(),transport.clone())?;
        require_unknown(app.automation().schedule_prepare(request.clone()).await)?;
        if native.creations.load(Ordering::SeqCst)!=1 { return Err("native preparation did not run exactly once".into()); }
        let inputs_match = {
            let original_inputs=native.inputs.lock().map_err(|_| "inputs poisoned")?;
            original_inputs.first().is_some_and(|input| input.instruction_text=="original central instruction" && input.expected_change_id==schedule.change_id)
        };
        if !inputs_match { return Err("native inputs did not come from central".into()); }
        let first=transport.requests.lock().map_err(|_| "calls poisoned")?.first().cloned().ok_or("Bind never reached real central")?;
        drop(app);
        let reopened=Arc::new(tokio::sync::Mutex::new(AutomationStore::open(&fixture.api.directory().join("automation.sqlite")).await?));
        if !matches!(reopened.lock().await.read_operation(&request.operation_id).await?.state,StoredOperationState::InProgress {..}) { return Err("lost Bind answer finalized the operation".into()); }
        let app=application(reopened.clone(),native.clone(),transport.clone())?;
        let bound=app.automation().schedule_prepare(request.clone()).await?;
        let replay=app.automation().schedule_prepare(request.clone()).await?;
        let inspected=app.automation().operation_show(OperationShowRequest {operation_id:request.operation_id.clone()},
            collaboration_service::collaboration_application::API_RESULT_BUDGET).await
            .map_err(|error|format!("central preparation receipt was not inspectable: {error:?}"))?;
        if !matches!(inspected.state,OperationState::Succeeded {outcome:OperationSuccess::Schedule {result,..}} if serde_json::to_value(&result).ok()==serde_json::to_value(&bound).ok()) {
            return Err("operation inspection lost the exact central preparation result".into());
        }
        if native.creations.load(Ordering::SeqCst)!=1 || serde_json::to_value(&bound)?!=serde_json::to_value(replay)? { return Err("retry repeated native creation or changed retained result".into()); }
        if !matches!(&bound.definition.destination,ExecutionDestination::OwnedThread {target,..} if *target==native.target) || bound.change_id==schedule.change_id { return Err("actual central binding was not returned".into()); }
        let calls=transport.requests.lock().map_err(|_| "calls poisoned")?.clone();
        if calls != vec![first.clone(),first] { return Err("retry rebuilt or repeated the exact central Bind request".into()); }
        if !matches!(reopened.lock().await.read_operation(&request.operation_id).await?.state,StoredOperationState::Succeeded {..}) { return Err("real central answer did not finalize the operation".into()); }
        drop(app); Arc::try_unwrap(reopened).map_err(|_| "reopened store still owned")?.into_inner().close().await?;
        Ok(())
    }.await;
    drop(native);
    drop(transport);
    let closed = fixture.close().await;
    proof?;
    closed
}
#[tokio::test]
async fn changed_request_same_operation_is_refused_without_native_or_bind_replay() -> ProofResult {
    let (fixture, schedule, native, transport) = setup().await?;
    let proof:ProofResult=async {
        let mut request=request(schedule.schedule_id)?;
        let app=application(fixture.store.as_ref().ok_or("missing store")?.clone(),native.clone(),transport.clone())?;
        require_unknown(app.automation().schedule_prepare(request.clone()).await)?;
        let DestinationPreparation::Fresh {cwd,..}=&mut request.destination else {return Err("wrong fixture destination".into());};
        *cwd="/changed-request".into();
        let result=app.automation().schedule_prepare(request).await;
        if !matches!(result,Err(ScheduleCommandFailure::Local(error)) if matches!(error.failure().kind,ScheduleFailureKind::OperationConflict)) {return Err("same operation admitted changed request".into());}
        if native.creations.load(Ordering::SeqCst)!=1 || transport.requests.lock().map_err(|_|"calls poisoned")?.len()!=1 {return Err("conflicting request dispatched an effect".into());}
        drop(app);Ok(())
    }.await;
    drop(native);
    drop(transport);
    let closed = fixture.close().await;
    proof?;
    closed
}
#[tokio::test]
async fn null_legacy_preparation_remains_uncertain_without_new_effects() -> ProofResult {
    let (fixture, schedule, native, transport) = setup().await?;
    let proof:ProofResult=async {
        let request=request(schedule.schedule_id.clone())?;
        let store=fixture.store.as_ref().ok_or("missing store")?;
        let evidence=native.evidence(true);
        store.lock().await.admit_schedule_preparation::<_,serde_json::Value>(&ExternalAdmission {operation_id:request.operation_id.clone(),schedule_id:request.schedule_id.clone(),canonical_request:serde_json::to_vec(&(&request.schedule_id,&request.destination))?,evidence:evidence.clone(),now_ms:0}).await?;
        store.lock().await.record_preparation_intent(&PreparationIntent {operation_id:request.operation_id.clone(),schedule_id:request.schedule_id.clone(),evidence:evidence.clone()}).await?;
        let legacy=serde_json::to_value(ScheduleFailure {kind:ScheduleFailureKind::OutcomeUnknown,stage:ScheduleFailureStage::Preparation,message:"legacy uncertain native effect".into(),operation_id:Some(request.operation_id.clone()),schedule_id:Some(request.schedule_id.clone()),current_change_id:None,field:None,constraint:None,details:ScheduleFailureDetails::None,effects:ScheduleEffects::Route {evidence:Box::new(collaboration_runtime::schedule_preparation_evidence_sink::project_evidence(&evidence)?)},next_action:ScheduleNextAction::InspectOperation})?;
        store.lock().await.record_preparation_failure(PreparationFailureRecord {operation_id:request.operation_id.clone(),schedule_id:request.schedule_id.clone(),disposition:PreparationFailureDisposition::Uncertain,evidence,error:legacy.clone()}).await?;
        let app=application(store.clone(),native.clone(),transport.clone())?;
        let result=app.automation().schedule_prepare(request.clone()).await;
        if !matches!(result,Err(ScheduleCommandFailure::Local(error)) if serde_json::to_value(error.failure()).ok()==Some(legacy.clone())) {return Err("NULL legacy row lost its retained uncertain answer".into());}
        let record=store.lock().await.read_operation(&request.operation_id).await?;
        if !matches!(record.state,StoredOperationState::Uncertain {failure:Some(failure),..} if failure==legacy) || native.creations.load(Ordering::SeqCst)!=0 || !transport.requests.lock().map_err(|_|"calls poisoned")?.is_empty() {return Err("NULL legacy row or effect boundary changed".into());}
        drop(app);Ok(())
    }.await;
    drop(native);
    drop(transport);
    let closed = fixture.close().await;
    proof?;
    closed
}

#[tokio::test]
async fn unavailable_preparation_read_retains_caller_operation_identity_without_effects()
-> ProofResult {
    let (fixture, schedule, native, transport) = setup().await?;
    let proof:ProofResult=async {
        transport.unavailable_reads.store(true,Ordering::SeqCst);
        let request=request(schedule.schedule_id)?;
        let app=application(fixture.store.as_ref().ok_or("missing store")?.clone(),native.clone(),transport.clone())?;
        let result=app.automation().schedule_prepare(request.clone()).await;
        if !matches!(result,Err(ScheduleCommandFailure::Catalog(error)) if matches!(*error,CentralCatalogFailure::CentralUnreachable {operation_id:Some(ref actual)} if *actual==request.operation_id)) {
            return Err("preparation read outage lost caller identity or became unknown mutation".into());
        }
        if native.creations.load(Ordering::SeqCst)!=0 || !transport.requests.lock().map_err(|_|"calls poisoned")?.is_empty()
            || !matches!(fixture.store.as_ref().ok_or("missing store")?.lock().await.read_operation(&request.operation_id).await,Err(automation_storage::StorageError::OperationNotFound)) {
            return Err("preparation read outage admitted an operation or native effect".into());
        }
        drop(app);Ok(())
    }.await;
    drop(native);
    drop(transport);
    let closed = fixture.close().await;
    proof?;
    closed
}
