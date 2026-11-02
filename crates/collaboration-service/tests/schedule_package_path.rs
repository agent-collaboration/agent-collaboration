//! Real SDK/socket/Fleet package transfers work without a native backend or local definitions.
use collaboration_protocol::{
    InstructionCreateParams, InstructionText, OperationId, ScheduleCreateRequest,
    ScheduleImportRequest, ScheduleShowRequest,
};
use serde_json::json;
#[path = "support/central_catalog_fixture.rs"]
mod fixture;

#[tokio::test]
async fn sdk_exports_and_imports_disabled_schedule_with_replay() -> fixture::ProofResult {
    exercise_package(false).await
}
#[tokio::test]
async fn sdk_fresh_mode_export_overwrite_and_local_setup_preserve_mode() -> fixture::ProofResult {
    exercise_package(true).await
}
async fn exercise_package(fresh: bool) -> fixture::ProofResult {
    let fixture = fixture::CatalogFixture::open(|transport| transport, true).await?;
    let proof: fixture::ProofResult = async {
        if !fixture.application.automation().automation_status().await.storage_available {
            return Err("central package fixture local Run capability differs from its target role".into());
        }
        let client = fixture.api.client("package-test").await?;
        if fresh && !matches!(fixture.transport.call(collaboration_protocol::AutomationCall::PullAutomationBatches(
            collaboration_protocol::PullAutomationBatches { after: None, max_bytes: 1_048_576,
                work_protocols: vec![collaboration_protocol::MachineWorkProtocol::MachineWorkV1] },
        )).await?, collaboration_protocol::AutomationAnswer::Batches(_)) {
            return Err("fresh target fixture did not causally advertise machine-work support".into());
        }
        let instruction = client.create_instruction(InstructionCreateParams {
            operation_id: OperationId::generate(), text: InstructionText::try_from("Check the build".to_owned())?,
        }).await?;
        let endpoint = json!({"serviceId":fixture::SERVICE,"endpointId":"source-local"});
        let destination = if fresh { json!({"kind":"freshEachRun","endpoint":endpoint,"cwd":"/source-workspace"}) }
            else { json!({"kind":"unprepared"}) };
        let request: ScheduleCreateRequest = serde_json::from_value(json!({
            "operationId":OperationId::generate(),"definition":{
                "instructionId":instruction.instruction_id,"timing":{"kind":"interval","seconds":600},
                "enabled":false,"destination":destination,"executionTimeoutSeconds":null,"model":"gpt-6-luna","effort":"low"
            }
        }))?;
        let first = client.create_schedule(request).await?;
        let package = client.export_schedule(ScheduleShowRequest { schedule_id: first.schedule_id.clone() }).await?;
        let import = ScheduleImportRequest { operation_id: OperationId::generate(), package_utf8: package.package_utf8, overwrite: true };
        let result = client.import_schedule(import.clone()).await?;
        let replay = client.import_schedule(import).await?;
        if serde_json::to_value(&result)? != serde_json::to_value(replay)? || !result.already_sent.is_empty() {
            return Err("package replay changed central mutation evidence or invented sent work".into());
        }
        let collaboration_client::CatalogAutomationResult::Schedule(mut imported) = result.automation else { return Err("import did not return the central Schedule".into()); };
        if imported.schedule_id != first.schedule_id || imported.change_id == first.change_id || imported.definition.enabled {
            return Err("package import lost identity, overwrite revision or disabled state".into());
        }
        if fresh {
            if !matches!(imported.definition.destination, collaboration_protocol::ExecutionDestination::FreshEachRunUnprepared) {
                return Err("SDK import lost fresh mode or retained local binding".into());
            }
            let operation = OperationId::generate();
            let enable = client.enable_schedule(collaboration_protocol::ScheduleEnableRequest {
                operation_id: operation.clone(), schedule_id: imported.schedule_id.clone(),
            }).await;
            if !matches!(enable, Err(collaboration_client::ScheduleClientError::Catalog(collaboration_client::CentralCatalogFailure::Refused {
                operation_id: Some(actual), error, ..
            })) if actual == operation && matches!(&*error, collaboration_protocol::AutomationRefusal::InvalidRequest {
                field, reason: collaboration_protocol::AutomationRequestConstraint::UnpreparedDestination
            } if field == "enabled")) {
                return Err("unprepared fresh schedule lacked the central pre-effect enable refusal".into());
            }
            let mut definition = imported.definition.clone();
            definition.destination = collaboration_protocol::ExecutionDestination::FreshEachRun {
                endpoint: serde_json::from_value(json!({"serviceId":fixture::SERVICE,"endpointId":"destination-local"}))?,
                cwd: "/destination-workspace".into(),
            };
            let changed = client.update_schedule(collaboration_protocol::ScheduleUpdateRequest {
                operation_id: OperationId::generate(), schedule_id: imported.schedule_id.clone(), expected_change_id: imported.change_id.clone(), definition,
            }).await?;
            let collaboration_client::ScheduleMutationResponse::Central(changed) = changed else { return Err("central package update switched to Local".into()); };
            let collaboration_client::CatalogAutomationResult::Schedule(updated) = changed.automation else { return Err("updated fresh Schedule missing".into()); };
            imported = updated;
            if !matches!(imported.definition.destination, collaboration_protocol::ExecutionDestination::FreshEachRun { ref cwd, .. } if cwd == "/destination-workspace")
                || imported.definition.enabled {
                return Err("local setup changed mode or enabled execution".into());
            }
        }
        // Escaping can exceed the frame while the source string remains within its text limit.
        let oversized = client.import_schedule(ScheduleImportRequest {
            operation_id: OperationId::generate(), package_utf8: "\"".repeat(600_000), overwrite: true,
        }).await;
        if !matches!(oversized, Err(collaboration_client::ScheduleClientError::Rejected(error))
            if matches!(error.details, collaboration_protocol::ScheduleFailureDetails::FrameLimit { encoded_bytes, maximum_bytes } if encoded_bytes > maximum_bytes)
                && error.field.as_deref() == Some("packageUtf8")) {
            return Err("oversized import did not return typed encoded-frame feedback".into());
        }
        client.update_instruction(collaboration_protocol::InstructionUpdateParams {
            operation_id: OperationId::generate(), instruction_id: instruction.instruction_id, expected_revision_id: instruction.revision_id,
            text: InstructionText::try_from("\"".repeat(300_000))?,
        }).await?;
        let request = ScheduleShowRequest { schedule_id: first.schedule_id };
        let oversized = client.export_schedule(request.clone()).await;
        if !matches!(oversized, Err(collaboration_client::ScheduleClientError::Rejected(error))
            if matches!(error.details, collaboration_protocol::ScheduleFailureDetails::FrameLimit { encoded_bytes, maximum_bytes } if encoded_bytes > maximum_bytes)) {
            return Err("oversized export did not account for its escaped response envelope".into());
        }
        // Rejection leaves the SDK usable and never mutates central definition truth.
        let collaboration_client::ScheduleReadResponse::Schedule { value: inspected } = client.read_schedule(request.clone()).await? else { return Err("Schedule inspection missing".into()); };
        let collaboration_protocol::AutomationAnswer::Schedule(central) = fixture.transport.call(collaboration_protocol::AutomationCall::ScheduleShow(request)).await? else {
            return Err("central Schedule disappeared after frame rejection".into());
        };
        if inspected.change_id != imported.change_id || central.change_id != imported.change_id {
            return Err("package size rejection changed Schedule state".into());
        }
        Ok(())
    }.await;
    let stopped = fixture.close().await;
    proof?;
    stopped
}
