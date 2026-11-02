//! Central admission requires MachineWorkV1; target preparation owns native capability checks.
use collaboration_client::{CentralCatalogFailure, ScheduleClientError};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationRefusal, InstructionCreateParams,
    LocalMutationState, MachineWorkProtocol, OperationId, PullAutomationBatches,
    ScheduleCreateRequest, ScheduleEffects, ScheduleFailureKind,
};
use serde_json::json;
#[path = "support/central_catalog_fixture.rs"]
mod fixture;

#[tokio::test]
async fn enabled_schedule_requires_machine_work_readiness_and_target_preparation_checks_native()
-> fixture::ProofResult {
    let fixture = fixture::CatalogFixture::open(|transport| transport, true).await?;
    let proof: fixture::ProofResult = async {
        let client = fixture.api.client("schedule-activation").await?;
        let instruction = client.create_instruction(InstructionCreateParams {
            operation_id: OperationId::generate(), text: "Check".to_owned().try_into()?,
        }).await?;
        let mut request: ScheduleCreateRequest = serde_json::from_value(json!({
            "operationId":OperationId::generate(),"definition":{
                "instructionId":instruction.instruction_id,
                "timing":{"kind":"interval","seconds":600},"enabled":true,
                "destination":{"kind":"freshEachRun","endpoint":{"serviceId":fixture::SERVICE,"endpointId":"codex-local"},"cwd":"/fixture"},
                "executionTimeoutSeconds":null,"model":"gpt-6-luna","effort":"low"
            }
        }))?;
        let rejected = client.create_schedule(request.clone()).await;
        if !matches!(rejected, Err(ScheduleClientError::Catalog(CentralCatalogFailure::Refused {
            operation_id:Some(operation), error, ..
        })) if operation == request.operation_id && matches!(&*error, AutomationRefusal::TargetNeedsUpgrade {service_id} if String::from(service_id.clone()) == fixture::SERVICE)) {
            return Err("enabled Schedule lacked the exact target-readiness refusal".into());
        }
        let empty = client.list_schedules(serde_json::from_value(json!({"cursor":null,"limit":50}))?).await?;
        if !empty.records.is_empty() || empty.next_cursor.is_some() {
            return Err("refused creation changed central Schedule inventory".into());
        }
        if !matches!(fixture.transport.call(AutomationCall::PullAutomationBatches(PullAutomationBatches {
            after: None, max_bytes: 1_048_576,
            work_protocols: vec![MachineWorkProtocol::MachineWorkV1],
        })).await?, AutomationAnswer::Batches(_)) {
            return Err("activation fixture did not advertise MachineWorkV1".into());
        }
        request.operation_id = OperationId::generate();
        let enabled = client.create_schedule(request.clone()).await?;
        if !enabled.definition.enabled {
            return Err("ready central target lost requested activation".into());
        }
        // Catalog readiness is distinct from native capability. The target refuses an
        // actual reuse-thread preparation before admitting any operation or native effect.
        request.operation_id = OperationId::generate();
        request.definition.enabled = false;
        request.definition.destination = collaboration_protocol::ExecutionDestination::Unprepared;
        let unprepared = client.create_schedule(request).await?;
        let operation = OperationId::generate();
        let failure = client.prepare_schedule(serde_json::from_value(json!({
            "operationId":operation,"scheduleId":unprepared.schedule_id,
            "destination":{"kind":"fresh","endpoint":{"serviceId":fixture::SERVICE,"endpointId":"codex-local"},"cwd":"/fixture"}
        }))?).await;
        if !matches!(failure, Err(ScheduleClientError::Rejected(failure)) if
            matches!(failure.kind, ScheduleFailureKind::UnsupportedCapability)
            && failure.operation_id.as_ref() == Some(&operation)
            && matches!(failure.effects, ScheduleEffects::Local {mutation:LocalMutationState::None})) {
            return Err("native capability refusal lost caller identity or pre-effect evidence".into());
        }
        let store = fixture.store.as_ref().ok_or("missing Router store")?;
        if !matches!(store.lock().await.read_operation(&operation).await, Err(automation_storage::StorageError::OperationNotFound)) {
            return Err("pre-effect refusal admitted a preparation operation".into());
        }
        let AutomationAnswer::Schedule(current) = fixture.transport.call(AutomationCall::ScheduleShow(
            collaboration_protocol::ScheduleShowRequest {schedule_id:unprepared.schedule_id.clone()}
        )).await? else {return Err("refused preparation lost central Schedule".into());};
        if current.change_id != unprepared.change_id || !matches!(current.definition.destination, collaboration_protocol::ExecutionDestination::Unprepared) {
            return Err("refused native preparation changed central definition".into());
        }
        for id in [enabled.schedule_id, unprepared.schedule_id] {
            let firings = client.list_firings(collaboration_protocol::FiringListRequest {
                automation_id:id.as_str().to_owned().try_into()?, cursor:None, limit:50.try_into()?,
            }).await?;
            if !firings.records.is_empty() || firings.next_cursor.is_some() {
                return Err("admission or refused preparation invented a Firing".into());
            }
        }
        Ok(())
    }.await;
    let stopped = fixture.close().await;
    proof?;
    stopped
}
