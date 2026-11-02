//! Portable import crosses real public APIs and the single Fleet catalog transaction.
use super::central_catalog_fixture::{CatalogHosts, ProofResult, connected_transport};
use agent_automation::{
    ExecutionDestination, PortableInstruction, PortableSchedulePackage, ScheduleDefinition,
};
use collaboration_client::{CentralCatalogFailure, CollaborationClient, ScheduleClientError};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationRefusal, EndpointRef, InstructionId,
    InstructionShowParams, OperationId, ScheduleId, ScheduleImportRequest, ScheduleShowRequest,
    SessionRef,
};
use collaboration_runtime::CentralAutomationTransport;

fn portable_package() -> ProofResult<PortableSchedulePackage<SessionRef, EndpointRef>> {
    let instruction_id = InstructionId::generate();
    Ok(PortableSchedulePackage {
        schedule_id: ScheduleId::generate(),
        source_change_id: "source-change".into(),
        instruction: PortableInstruction {
            instruction_id: instruction_id.clone(),
            text: "Portable central instructions".to_owned().try_into()?,
            source_revision_id: "source-revision".into(),
        },
        definition: ScheduleDefinition {
            instruction_id,
            timing: serde_json::from_value(serde_json::json!({"kind":"interval","seconds":600}))?,
            enabled: true,
            destination: ExecutionDestination::Unprepared,
            execution_timeout_seconds: None,
            model: Some("gpt-6-luna".into()),
            effort: Some("low".into()),
        },
        continuity: None,
    })
}

#[tokio::test]
async fn public_schedule_import_is_central_atomic_and_replays_across_hosts() -> ProofResult {
    let hosts = CatalogHosts::open().await?;
    let proof: ProofResult = async {
        drop(connected_transport(&hosts.first).await?);
        let transport = connected_transport(&hosts.second).await?;
        let first = CollaborationClient::connect(&hosts.first_root, "package-import-a", "1").await?;
        let second = CollaborationClient::connect(&hosts.second_root, "package-import-b", "1").await?;
        let package = portable_package()?;
        let request = ScheduleImportRequest { operation_id: OperationId::generate(),
            package_utf8: agent_automation::encode_schedule_package(&package, 1_048_576)?, overwrite: false };
        // Act: the normal JSONL package surface must reach the central transaction.
        let imported = first.import_schedule(request.clone()).await?;
        let AutomationAnswer::Schedule(schedule) = transport.call(AutomationCall::ScheduleShow(
            ScheduleShowRequest { schedule_id: package.schedule_id.clone() })).await? else {
            return Err("public package import wrote locally instead of reaching central".into());
        };
        let value = serde_json::to_value(&imported)?;
        let mut expected_schedule=serde_json::to_value(&schedule)?;
        let expected_object=expected_schedule.as_object_mut().ok_or("central Schedule must be an object")?;
        if expected_object.remove("activeRunId").is_none()||expected_object.remove("waitingRunId").is_none(){return Err("shared central Schedule placeholders missing".into());}
        if value != serde_json::json!({"automation":{"kind":"schedule","value":expected_schedule},"alreadySent":[]})
            || schedule.definition.enabled || schedule.next_due_at.is_some()
            || schedule.change_id.as_str() == package.source_change_id {
            return Err("public import lost central mutation evidence, new revision or disabled state".into());
        }
        let replay = second.import_schedule(request.clone()).await?;
        if serde_json::to_value(replay)? != value {
            return Err("same-operation package replay changed identity across Hosts".into());
        }
        let AutomationAnswer::Instruction(instruction) = transport.call(AutomationCall::InstructionShow(
            InstructionShowParams { instruction_id: package.instruction.instruction_id.clone() })).await? else {
            return Err("imported instruction is absent from central".into());
        };
        if instruction.text != package.instruction.text || instruction.revision_id.as_str() == package.instruction.source_revision_id {
            return Err("import lost instruction bytes or reused its source revision".into());
        }
        let operation = OperationId::generate();
        let exists = second.import_schedule(ScheduleImportRequest { operation_id: operation.clone(), ..request.clone() }).await;
        if !matches!(exists, Err(ScheduleClientError::Catalog(CentralCatalogFailure::Refused { operation_id: Some(actual), error, .. }))
            if actual == operation && matches!(&*error, AutomationRefusal::ScheduleImportExists { schedule_id } if schedule_id == &package.schedule_id)) {
            return Err("duplicate package import lost typed explicit-overwrite refusal".into());
        }
        let mut changed = package.clone();
        changed.definition.effort = Some("high".into());
        let changed = ScheduleImportRequest { operation_id: OperationId::generate(), overwrite: true,
            package_utf8: agent_automation::encode_schedule_package(&changed, 1_048_576)? };
        let updated = serde_json::to_value(second.import_schedule(changed).await?)?;
        if updated.pointer("/automation/value/definition/effort") != Some(&serde_json::json!("high"))
            || updated.pointer("/automation/value/changeId") == value.pointer("/automation/value/changeId") {
            return Err("explicit overwrite did not change the central definition and revision".into());
        }
        if serde_json::to_value(first.import_schedule(request).await?)? != value {
            return Err("original import receipt changed after a later overwrite".into());
        }
        // Assert: neither API Host owns a definition or a local import receipt.
        use sqlx::Connection;
        for root in [&hosts.first_root, &hosts.second_root] {
            let mut sql = sqlx::SqliteConnection::connect_with(&sqlx::sqlite::SqliteConnectOptions::new().filename(root.join("automation.sqlite"))).await?;
            for (table, query) in [
                ("instruction_documents", "SELECT count(*) FROM instruction_documents"),
                ("schedule_definitions", "SELECT count(*) FROM schedule_definitions"),
                ("operation_receipts", "SELECT count(*) FROM operation_receipts"),
            ] {
                let count: i64 = sqlx::query_scalar(query).fetch_one(&mut sql).await?;
                if count != 0 { return Err(format!("central import retained local truth in {table}").into()); }
            }
            sql.close().await?;
        }
        Ok(())
    }.await;
    let stopped = hosts.close().await;
    proof?;
    stopped
}

#[tokio::test]
async fn public_schedule_import_preserves_atomic_conflict_and_exact_grid_refusals() -> ProofResult {
    let hosts = CatalogHosts::open().await?;
    let proof: ProofResult = async {
        drop(connected_transport(&hosts.first).await?);
        let transport = connected_transport(&hosts.second).await?;
        let client = CollaborationClient::connect(&hosts.first_root, "package-refusals", "1").await?;
        let package = portable_package()?;
        client.import_schedule(ScheduleImportRequest { operation_id: OperationId::generate(),
            package_utf8: agent_automation::encode_schedule_package(&package, 1_048_576)?, overwrite: false }).await?;
        // Arrange: a second package shares the instruction UUID but changes its content.
        let mut conflicting = package.clone();
        conflicting.schedule_id = ScheduleId::generate();
        conflicting.instruction.text = "Must not overwrite shared instructions".to_owned().try_into()?;
        let operation = OperationId::generate();
        let conflict = client.import_schedule(ScheduleImportRequest { operation_id: operation.clone(),
            package_utf8: agent_automation::encode_schedule_package(&conflicting, 1_048_576)?, overwrite: true }).await;
        if !matches!(conflict, Err(ScheduleClientError::Catalog(CentralCatalogFailure::Refused { operation_id: Some(actual), error, .. }))
            if actual == operation && matches!(&*error, AutomationRefusal::InstructionImportConflict { instruction_id, schedule_ids }
                if instruction_id == &package.instruction.instruction_id && schedule_ids == &vec![package.schedule_id.clone()])) {
            return Err("public import lost the shared instruction conflict and referencing Schedule IDs".into());
        }
        if !matches!(transport.call(AutomationCall::ScheduleShow(ScheduleShowRequest { schedule_id: conflicting.schedule_id })).await?,
            AutomationAnswer::Refused(AutomationRefusal::NotFound)) {
            return Err("refused content conflict leaked a Schedule".into());
        }
        let AutomationAnswer::Instruction(document) = transport.call(AutomationCall::InstructionShow(
            InstructionShowParams { instruction_id: package.instruction.instruction_id.clone() })).await? else {
            return Err("content conflict removed the shared instruction".into());
        };
        if document.text != package.instruction.text { return Err("content conflict changed shared instruction bytes".into()); }
        // Act: an exact off-grid instant must be refused without rounding or partial writes.
        let mut off_grid = portable_package()?;
        let earlier = String::from(super::central_catalog_fixture::future_wake_time()?);
        let earlier_ms = chrono::DateTime::parse_from_rfc3339(&earlier)?.timestamp_millis();
        let at = chrono::DateTime::from_timestamp_millis(earlier_ms + 60_000).ok_or("fixture timestamp")?
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        off_grid.definition.timing = serde_json::from_value(serde_json::json!({"kind":"at","at":at}))?;
        let operation = OperationId::generate();
        let refused = client.import_schedule(ScheduleImportRequest { operation_id: operation.clone(),
            package_utf8: agent_automation::encode_schedule_package(&off_grid, 1_048_576)?, overwrite: false }).await;
        if !matches!(refused, Err(ScheduleClientError::Catalog(CentralCatalogFailure::Refused { operation_id: Some(actual), error, .. }))
            if actual == operation && matches!(&*error, AutomationRefusal::Validation { error: collaboration_protocol::AutomationValidationError::OffGrid { earlier: before, later: after, .. } }
                if chrono::DateTime::parse_from_rfc3339(before).is_ok_and(|value| value.timestamp_millis() == earlier_ms)
                    && chrono::DateTime::parse_from_rfc3339(after).is_ok_and(|value| value.timestamp_millis() == earlier_ms + 300_000))) {
            return Err("public off-grid import rounded the instant or lost nearest grid marks".into());
        }
        for call in [AutomationCall::ScheduleShow(ScheduleShowRequest { schedule_id: off_grid.schedule_id }),
            AutomationCall::InstructionShow(InstructionShowParams { instruction_id: off_grid.instruction.instruction_id })] {
            if !matches!(transport.call(call).await?, AutomationAnswer::Refused(AutomationRefusal::NotFound)) {
                return Err("off-grid import partially wrote the Schedule or Instruction".into());
            }
        }
        Ok(())
    }.await;
    let stopped = hosts.close().await;
    proof?;
    stopped
}

#[tokio::test]
async fn public_schedule_export_uses_current_central_instruction_and_imported_continuity()
-> ProofResult {
    let hosts = CatalogHosts::open().await?;
    let proof: ProofResult = async {
        drop(connected_transport(&hosts.first).await?);
        let transport = connected_transport(&hosts.second).await?;
        let first = CollaborationClient::connect(&hosts.first_root, "package-export-a", "1").await?;
        let second = CollaborationClient::connect(&hosts.second_root, "package-export-b", "1").await?;
        let mut package = portable_package()?;
        package.continuity = Some(agent_automation::PortableContinuity {
            text: "Imported continuity from another machine".to_owned().try_into()?,
            source_run_id: "imported-source-run".into(),
            source_target: serde_json::from_value(serde_json::json!({"endpoint":{
                "serviceId":first.identity().service_id,"endpointId":"codex-local"},"sessionId":"inert-summary-source"}))?,
        });
        first.import_schedule(ScheduleImportRequest { operation_id: OperationId::generate(),
            package_utf8: agent_automation::encode_schedule_package(&package, 1_048_576)?, overwrite: false }).await?;
        let AutomationAnswer::Instruction(original) = transport.call(AutomationCall::InstructionShow(
            InstructionShowParams { instruction_id: package.instruction.instruction_id.clone() })).await? else {
            return Err("central import instruction missing".into());
        };
        // Arrange: central's instruction advances after import; neither Host has a local definition.
        let current = first.update_instruction(collaboration_protocol::InstructionUpdateParams {
            operation_id: OperationId::generate(), instruction_id: original.instruction_id,
            expected_revision_id: original.revision_id, text: "Edited in the central catalog".to_owned().try_into()?,
        }).await?;
        let AutomationAnswer::Schedule(schedule) = transport.call(AutomationCall::ScheduleShow(
            ScheduleShowRequest { schedule_id: package.schedule_id.clone() })).await? else { return Err("central imported Schedule missing".into()); };
        // Act: a different machine exports through the normal public package surface.
        let exported = second.export_schedule(ScheduleShowRequest { schedule_id: package.schedule_id.clone() }).await?;
        let decoded = agent_automation::decode_schedule_package::<SessionRef, EndpointRef>(&exported.package_utf8, 1_048_576)?;
        if decoded.schedule_id != package.schedule_id || decoded.source_change_id != schedule.change_id.as_str()
            || decoded.instruction.text != current.text || decoded.instruction.source_revision_id != current.revision_id.as_str()
            || decoded.definition.enabled || decoded.definition.destination.is_prepared()
            || serde_json::to_value(decoded.continuity)? != serde_json::to_value(package.continuity)? {
            return Err("public export used local or stale definitions, lost imported continuity, or retained a live binding".into());
        }
        Ok(())
    }.await;
    let stopped = hosts.close().await;
    proof?;
    stopped
}
