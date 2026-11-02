//! Pre-upgrade local Schedule custody remains readable and frozen mutations do not dispatch.
use collaboration_protocol::{OperationId, ScheduleId};
use collaboration_service::ApplicationFixture;
use serde_json::json;
use sqlx::Connection;
use std::{os::unix::fs::PermissionsExt, sync::Arc};
use tokio::sync::Mutex;
#[path = "support/central_catalog_fixture.rs"]
mod central_fixture;
#[path = "support/served_api.rs"]
mod served_api;
type ProofResult<Value = ()> = Result<Value, Box<dyn std::error::Error + Send + Sync>>;
const SERVICE: &str = "00000000-0000-4000-8000-000000000001";
const EPOCH: &str = "00000000-0000-4000-8000-000000000002";

async fn seed(store: &Arc<Mutex<automation_storage::AutomationStore>>) -> ProofResult<ScheduleId> {
    let mut store = store.lock().await;
    let now = chrono::Utc::now().timestamp_millis();
    let instruction = store
        .create_instruction(
            &OperationId::generate(),
            &"Retained local instructions".to_owned().try_into()?,
            now,
        )
        .await?;
    Ok(store
        .create_schedule(&automation_storage::ScheduleCreate::<
            collaboration_protocol::SessionRef,
            collaboration_protocol::EndpointRef,
        > {
            operation_id: OperationId::generate(),
            definition: agent_automation::ScheduleDefinition {
                instruction_id: instruction.instruction_id,
                timing: agent_automation::TimingRule::Interval { seconds: 600 },
                enabled: false,
                destination: agent_automation::ExecutionDestination::Unprepared,
                execution_timeout_seconds: None,
                model: Some("gpt-6-luna".into()),
                effort: Some("low".into()),
            },
            imported_continuity: agent_automation::ContinuityInput::ImportedSummary {
                text: "Retained imported continuity".to_owned(),
                source_run_id: collaboration_protocol::RunId::generate()
                    .as_str()
                    .to_owned(),
                source_target: serde_json::from_value(json!({
                    "endpoint":{"serviceId":SERVICE,"endpointId":"codex-local"},
                    "sessionId":"retained-continuity-source"
                }))?,
                import_operation_id: OperationId::generate(),
            },
            now_ms: now,
        })
        .await?
        .schedule_id)
}

#[tokio::test]
async fn local_schedule_read_and_frozen_mutation_preserve_actual_custody_without_central()
-> ProofResult {
    let directory = tempfile::Builder::new()
        .prefix("s6-local-schedule-")
        .tempdir_in("/tmp")?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let path = directory.path().join("automation.sqlite");
    let store = Arc::new(Mutex::new(
        automation_storage::AutomationStore::open(&path).await?,
    ));
    let id = seed(&store).await?;
    let package = store
        .lock()
        .await
        .export_schedule::<collaboration_protocol::SessionRef, collaboration_protocol::EndpointRef>(
            &id,
        )
        .await?;
    if package
        .continuity
        .as_ref()
        .map(|summary| summary.text.as_str())
        != Some("Retained imported continuity")
    {
        return Err("retained fixture did not arrange real stored continuity".into());
    }
    let expected_export = agent_automation::encode_schedule_package(&package, 1_048_576)?;
    let served = served_api::ServedApi::start(
        ApplicationFixture::new(SERVICE, EPOCH)?.with_automation_store(store.clone()),
    )?;
    let proof: ProofResult=async {
        let request=json!({"scheduleId":id});
        let shown=served.call("schedule_show",request.clone()).await?;
        let before=shown.get("result").ok_or_else(||format!("local Schedule read failed: {shown}"))?;
        if before["kind"]!="local" || before.pointer("/value/scheduleId")!=Some(&json!(id))
            || before.pointer("/value/activeRunId")!=Some(&serde_json::Value::Null)
            || before.pointer("/value/waitingRunId")!=Some(&serde_json::Value::Null) {
            return Err("local Schedule lost its actual local occupancy fields or identity".into());
        }
        let exported=served.call("schedule_export",request.clone()).await?;
        if exported.pointer("/result/packageUtf8")!=Some(&json!(expected_export)) {
            return Err(format!("local export lost the stored Instruction or continuity: {exported}").into());
        }
        let mut sql=sqlx::SqliteConnection::connect_with(&sqlx::sqlite::SqliteConnectOptions::new().filename(&path)).await?;
        // Producer stand-in only: S7's actual atomic freeze and payload remain separate proof.
        sqlx::query("INSERT INTO automation_handover(automation_id,state,watermark_local_ms,frozen_payload,digest) VALUES (?,'pending',0,?,?)")
            .bind(id.as_str()).bind(b"{}".as_slice()).bind("44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a").execute(&mut sql).await?;
        let before_version:i64=sqlx::query_scalar("PRAGMA data_version").fetch_one(&mut sql).await?;
        let frozen=served.call("schedule_show",request).await?;
        if frozen.get("result")!=Some(before) {return Err("freeze changed the readable local Schedule".into());}
        let exported=served.call("schedule_export",json!({"scheduleId":id})).await?;
        if exported.pointer("/result/packageUtf8")!=Some(&json!(expected_export)) {
            return Err(format!("frozen export lost the retained package: {exported}").into());
        }
        for method in ["schedule_enable","schedule_disable","schedule_update","schedule_prepare"] {
            let mut params=json!({"operationId":OperationId::generate(),"scheduleId":id});
            if method=="schedule_update" {
                params["expectedChangeId"]=before["value"]["changeId"].clone();
                params["definition"]=before["value"]["definition"].clone();
            }
            if method=="schedule_prepare" {
                params["destination"]=json!({"kind":"existing","target":{"endpoint":{"serviceId":SERVICE,"endpointId":"codex-local"},"sessionId":"never-dispatched-frozen-schedule"},"cwd":"/tmp"});
            }
            let rejected=served.call(method,params).await?;
            if rejected.pointer("/error/data/kind")!=Some(&json!("handoverPending")) {
                return Err(format!("frozen Schedule mutation lacked typed handoverPending: {rejected}").into());
            }
        }
        let after_version:i64=sqlx::query_scalar("PRAGMA data_version").fetch_one(&mut sql).await?;
        if before_version!=after_version {return Err("frozen Schedule read/refusals changed SQLite".into());}
        sql.close().await?;
        Ok(())
    }.await;
    let stopped = served.stop().await;
    Arc::try_unwrap(store)
        .map_err(|_| "local Schedule storage still owned")?
        .into_inner()
        .close()
        .await?;
    directory.close()?;
    proof?;
    stopped?;
    Ok(())
}

#[tokio::test]
async fn schedule_list_and_controls_preserve_local_precedence_across_real_central_pages()
-> ProofResult {
    use collaboration_client::{
        ScheduleClientError, ScheduleMutationResponse, ScheduleReadResponse,
    };
    use collaboration_protocol::{
        AutomationAnswer, AutomationCall, AutomationPageRequest, InstructionCreateParams,
        ScheduleEnableRequest, ScheduleImportRequest, ScheduleShowRequest, ScheduleUpdateRequest,
    };
    let fixture = central_fixture::CatalogFixture::open(|transport| transport, true).await?;
    let proof: ProofResult = async {
        let store = fixture.store.as_ref().ok_or("local store missing")?;
        let local = seed(store).await?;
        let frozen = seed(store).await?;
        let client = fixture.api.client("mixed-schedule-custody").await?;
        let show = ScheduleShowRequest { schedule_id: local.clone() };
        let ScheduleReadResponse::Local { value: before } = client.read_schedule(show.clone()).await? else {
            return Err("actual pre-upgrade Schedule did not select Local".into());
        };
        // Import always uses the real central owner, even when this ID still has Local custody.
        let package = client.export_schedule(show.clone()).await?;
        client.import_schedule(ScheduleImportRequest {
            operation_id: OperationId::generate(), package_utf8: package.package_utf8, overwrite: false,
        }).await?;
        let AutomationAnswer::Schedule(central_before) = fixture.transport.call(AutomationCall::ScheduleShow(show.clone())).await? else {
            return Err("portable import did not create the actual central duplicate".into());
        };
        let operation = OperationId::generate();
        let request = ScheduleEnableRequest { operation_id: operation.clone(), schedule_id: local.clone() };
        let ScheduleMutationResponse::Local(changed) = client.disable_schedule(request.clone()).await? else {
            return Err("unmoved Schedule control selected central".into());
        };
        let replay = client.disable_schedule(request).await?;
        if serde_json::to_value(&replay)? != serde_json::to_value(ScheduleMutationResponse::Local(changed.clone()))? {
            return Err("Local control replay changed its actual receipt".into());
        }
        let stale_operation = OperationId::generate();
        let stale = client.update_schedule(ScheduleUpdateRequest {
            operation_id: stale_operation.clone(), schedule_id: local.clone(),
            expected_change_id: before.change_id, definition: before.definition,
        }).await;
        if !matches!(stale, Err(ScheduleClientError::Rejected(error))
            if matches!(error.kind, collaboration_protocol::ScheduleFailureKind::ChangeConflict)
                && error.operation_id.as_ref() == Some(&stale_operation)
                && error.current_change_id.as_ref() == Some(&changed.change_id)) {
            return Err("stale Local update lost its typed conflict or actual current revision".into());
        }
        let central_after = fixture.transport.call(AutomationCall::ScheduleShow(show)).await?;
        if serde_json::to_value(central_after)? != serde_json::to_value(AutomationAnswer::Schedule(central_before))? {
            return Err("Local controls changed the central duplicate".into());
        }
        let path = fixture.api.directory().join("automation.sqlite");
        let mut sql = sqlx::SqliteConnection::connect_with(&sqlx::sqlite::SqliteConnectOptions::new().filename(path)).await?;
        // Only S7's producer marker stands in; lists must preserve the real frozen Local snapshot.
        sqlx::query("INSERT INTO automation_handover(automation_id,state,watermark_local_ms,frozen_payload,digest) VALUES (?,'pending',0,?,?)")
            .bind(frozen.as_str()).bind(b"{}".as_slice()).bind("44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a").execute(&mut sql).await?;
        sql.close().await?;
        let instruction = client.create_instruction(InstructionCreateParams {
            operation_id: OperationId::generate(), text: "Central-only definition".to_owned().try_into()?,
        }).await?;
        let central = client.create_schedule(serde_json::from_value(json!({
            "operationId":OperationId::generate(),"definition":{
                "instructionId":instruction.instruction_id,"timing":{"kind":"interval","seconds":600},
                "enabled":false,"destination":{"kind":"unprepared"},"executionTimeoutSeconds":null,
                "model":"gpt-6-luna","effort":"low"
            }
        }))?).await?;
        let first = client.list_schedules(AutomationPageRequest { cursor: None, limit: 1.try_into()? }).await?;
        let budget = collaboration_service::ResultByteBudget::new(serde_json::to_vec(&first)?.len(), 0);
        let bounded = fixture.application.automation().schedule_list(
            AutomationPageRequest { cursor: None, limit: 100.try_into()? }, budget,
        ).await?;
        if serde_json::to_value(&bounded)? != serde_json::to_value(&first)? || !budget.admits(&bounded) {
            return Err("bounded Local Schedule page truncated a record or skipped its cursor position".into());
        }
        if fixture.application.automation().schedule_list(
            AutomationPageRequest { cursor: None, limit: 100.try_into()? },
            collaboration_service::ResultByteBudget::new(1, 0),
        ).await.is_ok() {
            return Err("impossible Schedule response budget returned a partial record".into());
        }
        let mut page = first;
        let mut seen = std::collections::BTreeMap::new();
        let mut complete = false;
        for _page_guard in 0..10 {
            for record in page.records {
                let (id, owner) = match record {
                    ScheduleReadResponse::Local { value } => (value.schedule_id, "local"),
                    ScheduleReadResponse::Schedule { value } => (value.schedule_id, "central"),
                    ScheduleReadResponse::Faulted { .. } | ScheduleReadResponse::Suspended { .. } => return Err("healthy fixture became faulted or suspended".into()),
                };
                if seen.insert(id.as_str().to_owned(), owner).is_some() {
                    return Err("central duplicate escaped Local Schedule precedence".into());
                }
            }
            let Some(cursor) = page.next_cursor else { complete = true; break; };
            page = client.list_schedules(AutomationPageRequest { cursor: Some(cursor), limit: 1.try_into()? }).await?;
        }
        let expected = std::collections::BTreeMap::from([
            (local.as_str().to_owned(), "local"), (frozen.as_str().to_owned(), "local"),
            (central.schedule_id.as_str().to_owned(), "central"),
        ]);
        if !complete || seen != expected {
            return Err(format!("mixed Schedule traversal changed custody or lost records: {seen:?}").into());
        }
        Ok(())
    }.await;
    let stopped = fixture.close().await;
    proof?;
    stopped
}
