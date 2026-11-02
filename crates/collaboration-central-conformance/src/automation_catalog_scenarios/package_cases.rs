//! Portable package imports commit through one real Fleet catalog transaction.
use super::schedule_cases::{operation, require_mutation};
use super::{call, raw_call, same_answer};
use crate::{
    AutomationClockControl, ConformanceClient, ConformanceResult,
    native_board_session::NativeBoardSession,
};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, InstructionShowParams, ScheduleId, ScheduleShowRequest,
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

pub(super) async fn run(
    client: &ConformanceClient,
    first: &mut NativeBoardSession,
    second: &mut NativeBoardSession,
    source_service: &str,
) -> ConformanceResult<()> {
    let package = portable_package_fixture(source_service);
    let original_id: ScheduleId = field(&package, "/scheduleId")?;
    let instruction_id = field(&package, "/instruction/instructionId")?;
    let create = request(package.clone(), false)?;
    let answer = raw_call(first, create.clone()).await?;
    let imported = require_mutation(answer.clone())?;
    if imported.schedule_id != original_id
        || imported.definition.instruction_id != instruction_id
        || imported.definition.enabled
        || imported.next_due_at.is_some()
        || imported.change_id.as_str() == "source-change"
    {
        return Err("package import lost IDs, allocated revision or disabled state".into());
    }
    let AutomationAnswer::Instruction(document) = call(
        second,
        AutomationCall::InstructionShow(InstructionShowParams {
            instruction_id: instruction_id.clone(),
        }),
    )
    .await?
    else {
        return Err("import did not create the preserved instruction identity".into());
    };
    if document.text.as_str() != "Portable instructions"
        || document.revision_id.as_str() == "source-revision"
    {
        return Err("import changed instruction bytes or reused source revision".into());
    }
    let continuity = json!({"kind":"importedSummary","text":"Portable continuity",
        "sourceRunId":"source-run","sourceTarget":field::<Value>(&package,"/continuity/sourceTarget")?,
        "importOperationId":field::<Value>(&create,"/value/operationId")?});
    if serde_json::to_value(&imported.imported_continuity)? != continuity {
        return Err("package continuity lost its source or import operation".into());
    }
    same_answer(raw_call(second, create.clone()).await?, answer.clone())?;
    let exists = raw_call(second, request(package.clone(), false)?).await?;
    if serde_json::to_value(exists)?
        != json!({"kind":"refused","value":{
            "kind":"scheduleImportExists","scheduleId":original_id
        }})
    {
        return Err("identical package without overwrite did not refuse explicitly".into());
    }
    let mut replacement = package.clone();
    replace(&mut replacement, "/definition/effort", json!("high"))?;
    replace(
        &mut replacement,
        "/continuity/text",
        json!("Replacement continuity"),
    )?;
    client
        .automation_clock(AutomationClockControl::Advance {
            now: collaboration_central_core::RegistryTime("2026-10-31T10:00:02.456Z".parse()?),
        })
        .await?;
    let overwrite = request(replacement, true)?;
    let overwrite_operation = field(&overwrite, "/value/operationId")?;
    let overwritten = require_mutation(raw_call(second, overwrite).await?)?;
    if overwritten.change_id == imported.change_id
        || overwritten.created_at != imported.created_at
        || overwritten.anchor_at == imported.anchor_at
        || overwritten.updated_at == imported.updated_at
        || overwritten.definition.enabled
        || overwritten.definition.effort.as_deref() != Some("high")
    {
        return Err(
            "overwrite lost creation identity, new anchor/revision or disabled state".into(),
        );
    }
    if !matches!(&overwritten.imported_continuity, collaboration_protocol::ImportedContinuity::ImportedSummary {
        text, import_operation_id, ..
    } if text == "Replacement continuity" && *import_operation_id == overwrite_operation)
    {
        return Err("overwrite retained stale imported continuity or operation provenance".into());
    }
    same_answer(raw_call(first, create.clone()).await?, answer)?;
    for pointer in [
        "/value/package/sourceChangeId",
        "/value/package/instruction/sourceRevisionId",
    ] {
        let mut conflict = create.clone();
        replace(&mut conflict, pointer, json!("different-source-id"))?;
        if !matches!(
            raw_call(first, conflict).await?,
            AutomationAnswer::Refused(
                collaboration_protocol::AutomationRefusal::OperationConflict { .. }
            )
        ) {
            return Err("package replay digest omitted source provenance".into());
        }
    }
    let mut conflict = package.clone();
    let refused_id = durable_steps::Uuid::now_v7().to_string();
    replace(&mut conflict, "/scheduleId", json!(refused_id))?;
    replace(
        &mut conflict,
        "/instruction/text",
        json!("Must not replace shared instructions"),
    )?;
    let refused = raw_call(first, request(conflict, true)?).await?;
    if serde_json::to_value(refused)?
        != json!({"kind":"refused","value":{
            "kind":"instructionImportConflict","instructionId":instruction_id,"scheduleIds":[original_id]
        }})
    {
        return Err("overwrite bypassed instruction content conflict or lost affected IDs".into());
    }
    missing_schedule(first, &refused_id).await?;
    same_answer(
        call(
            second,
            AutomationCall::InstructionShow(InstructionShowParams {
                instruction_id: instruction_id.clone(),
            }),
        )
        .await?,
        AutomationAnswer::Instruction(document),
    )?;
    for (pointer, limit) in [
        (
            "/value/package/instruction/text",
            collaboration_protocol::limits::LimitName::InstructionMaxBytes,
        ),
        (
            "/value/package/continuity/text",
            collaboration_protocol::limits::LimitName::InstructionMaxBytes,
        ),
        (
            "/value/package/sourceChangeId",
            collaboration_protocol::limits::LimitName::TextMaxBytes,
        ),
        (
            "/value/package/instruction/sourceRevisionId",
            collaboration_protocol::limits::LimitName::TextMaxBytes,
        ),
        (
            "/value/package/continuity/sourceRunId",
            collaboration_protocol::limits::LimitName::TextMaxBytes,
        ),
    ] {
        let maximum = usize::try_from(limit.definition().maximum)?;
        for text in [String::new(), "🦀".repeat(maximum / 4 + 1)] {
            let mut invalid = create.clone();
            replace(&mut invalid, pointer, json!(text))?;
            super::raw_limits::limited(first, invalid, limit, text.len() as u64).await?;
        }
    }
    let mut off_grid = portable_package_fixture(source_service);
    let off_grid_id: String = field(&off_grid, "/scheduleId")?;
    let off_grid_instruction = field(&off_grid, "/instruction/instructionId")?;
    replace(
        &mut off_grid,
        "/definition/timing",
        json!({"kind":"at","at":"2026-10-31T10:07:00Z"}),
    )?;
    let refused = raw_call(first, request(off_grid, false)?).await?;
    if !matches!(refused, AutomationAnswer::Refused(collaboration_protocol::AutomationRefusal::Validation {
        error: collaboration_protocol::AutomationValidationError::OffGrid { earlier, later, .. }
    }) if earlier == "2026-10-31T10:05:00Z" && later == "2026-10-31T10:10:00Z")
    {
        return Err("off-grid package was rounded or lost its nearest grid marks".into());
    }
    missing_schedule(first, &off_grid_id).await?;
    if !matches!(
        call(
            first,
            AutomationCall::InstructionShow(InstructionShowParams {
                instruction_id: off_grid_instruction
            })
        )
        .await?,
        AutomationAnswer::Refused(collaboration_protocol::AutomationRefusal::NotFound)
    ) {
        return Err("refused off-grid import leaked an instruction write".into());
    }
    let mut fresh = portable_package_fixture(source_service);
    replace(
        &mut fresh,
        "/definition/destination",
        json!({"kind":"freshEachRunUnprepared"}),
    )?;
    replace(&mut fresh, "/definition/model", json!("fixture-model"))?;
    replace(&mut fresh, "/continuity", Value::Null)?;
    let fresh = require_mutation(raw_call(second, request(fresh, false)?).await?)?;
    if fresh.definition.enabled
        || !matches!(
            fresh.imported_continuity,
            collaboration_protocol::ImportedContinuity::None
        )
        || !matches!(
            fresh.definition.destination,
            collaboration_protocol::ExecutionDestination::FreshEachRunUnprepared
        )
    {
        return Err("fresh-each-run package lost its unprepared mode or absent continuity".into());
    }
    let mut changed_mode = package.clone();
    replace(
        &mut changed_mode,
        "/definition/destination",
        json!({"kind":"freshEachRunUnprepared"}),
    )?;
    replace(
        &mut changed_mode,
        "/definition/model",
        json!("fixture-model"),
    )?;
    if !matches!(
        raw_call(first, request(changed_mode, true)?).await?,
        AutomationAnswer::Refused(collaboration_protocol::AutomationRefusal::InvalidRequest {
            reason: collaboration_protocol::AutomationRequestConstraint::ExecutionModeFixed,
            ..
        })
    ) {
        return Err("package overwrite changed the fixed execution mode".into());
    }
    let mut prepared = portable_package_fixture(source_service);
    let bound_target: Value = field(&prepared, "/continuity/sourceTarget")?;
    let prepared_id: String = field(&prepared, "/scheduleId")?;
    replace(
        &mut prepared,
        "/definition/destination",
        json!({
            "kind":"ownedThread","target":bound_target,"cwd":"/workspace"
        }),
    )?;
    if !matches!(
        raw_call(first, request(prepared, false)?).await?,
        AutomationAnswer::Refused(collaboration_protocol::AutomationRefusal::InvalidRequest {
            reason: collaboration_protocol::AutomationRequestConstraint::UnpreparedDestination,
            ..
        })
    ) {
        return Err("package import accepted a live native destination binding".into());
    }
    missing_schedule(first, &prepared_id).await?;
    println!(
        "SCHEDULE_PACKAGE_IMPORT_RESULT {}",
        json!({
            "idsPreserved":true,"alwaysDisabled":true,"overwriteExplicit":true,"contentConflictAtomic":true,
            "receiptStableAfterOverwrite":true,"sourceIdsInDigestOnly":true,"offGridNearestMarks":true
        })
    );
    Ok(())
}

fn portable_package_fixture(source_service: &str) -> Value {
    let instruction = durable_steps::Uuid::now_v7().to_string();
    json!({
        "scheduleId":durable_steps::Uuid::now_v7().to_string(),"sourceChangeId":"source-change",
        "instruction":{"instructionId":instruction,"text":"Portable instructions","sourceRevisionId":"source-revision"},
        "definition":{"instructionId":instruction,"timing":{"kind":"interval","seconds":600},
            "enabled":true,"destination":{"kind":"unprepared"},"executionTimeoutSeconds":null,"model":null,"effort":"low"},
        "continuity":{"text":"Portable continuity","sourceRunId":"source-run","sourceTarget":{
            "endpoint":{"serviceId":source_service,"endpointId":"codex-local"},"sessionId":"summary-source"}}
    })
}
fn request(package: Value, overwrite: bool) -> ConformanceResult<Value> {
    Ok(
        json!({"kind":"schedulePackageImport","value":{"operationId":operation()?,"package":package,"overwrite":overwrite}}),
    )
}
fn field<T: DeserializeOwned>(value: &Value, pointer: &str) -> ConformanceResult<T> {
    Ok(serde_json::from_value(
        value
            .pointer(pointer)
            .ok_or("package fixture field missing")?
            .clone(),
    )?)
}
fn replace(value: &mut Value, pointer: &str, replacement: Value) -> ConformanceResult<()> {
    *value
        .pointer_mut(pointer)
        .ok_or("package fixture field missing")? = replacement;
    Ok(())
}
async fn missing_schedule(machine: &mut NativeBoardSession, id: &str) -> ConformanceResult<()> {
    if !matches!(
        call(
            machine,
            AutomationCall::ScheduleShow(ScheduleShowRequest {
                schedule_id: id.to_owned().try_into()?
            })
        )
        .await?,
        AutomationAnswer::Refused(collaboration_protocol::AutomationRefusal::NotFound)
    ) {
        return Err("refused package leaked a schedule write".into());
    }
    Ok(())
}
