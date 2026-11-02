//! Actual frozen imports retain suspended configuration across public reads without executable fields.
use collaboration_protocol::*;
use serde_json::{Value, json};
#[path = "support/central_catalog_fixture.rs"]
mod fixture;
use fixture::{CatalogFixture, ProofResult};

async fn imported(
    kind: AutomationKind,
) -> ProofResult<(CatalogFixture, SuspendedAutomationSnapshot)> {
    let fixture = CatalogFixture::open(|transport| transport, true).await?;
    let id = ScheduleId::generate();
    let target = json!({"endpoint":{"serviceId":fixture::SERVICE,"endpointId":"codex-local"},"sessionId":"suspended-target"});
    let configuration = match kind {
        AutomationKind::Schedule => json!({"kind":"schedule","value":{
            "instructionId":InstructionId::generate(),"enabled":false,"destination":{"kind":"unprepared"},
            "executionTimeoutSeconds":null,"model":"gpt-6-luna","effort":"low"
        }}),
        AutomationKind::Wake => json!({"kind":"wake","value":{
            "target":target,"origin":{"kind":"agent","sender":target},"delivery":"auto","generationGuard":null,
            "expiresAt":"2030-11-01T12:00:00Z","state":"active"
        }}),
    };
    let payload = json!({"automationId":id,"revision":ChangeId::generate(),"creator":{"kind":"owner"},
        "header":{"anchorAt":"2026-10-31T09:00:00Z","createdAt":"2026-10-31T08:00:00Z","updatedAt":"2026-10-31T09:30:00Z","importedContinuity":null},
        "definition":{"kind":"suspended","value":{"legacyTiming":{"kind":"cron","expression":"58 * 1 * *","timezone":"UTC"},"configuration":configuration}},
        "content":{"revisionId":RevisionId::generate(),"text":"retained suspended content"},
        "watermarkLocalMs":1793440800000_i64,"firstDueMs":null,"admittedThroughMs":null,"externalOccupant":null,"carriedWaitingDueMs":null,"carriedFiring":null
    });
    let payload = FrozenAutomationImportPayload::from_bytes(
        serde_json::to_string_pretty(&payload)?.as_bytes(),
    )?;
    let answer = fixture
        .transport
        .call(AutomationCall::AutomationImport(AutomationImportRequest {
            import_id: ImportId::generate(),
            payload,
        }))
        .await?;
    if !matches!(answer, AutomationAnswer::Imported(_)) {
        return Err(format!("real suspended import failed: {answer:?}").into());
    }
    let call = match kind {
        AutomationKind::Schedule => {
            AutomationCall::ScheduleShow(ScheduleShowRequest { schedule_id: id })
        }
        AutomationKind::Wake => AutomationCall::WakeShow(WakeShowRequest {
            wakeup_id: id.as_str().to_owned().try_into()?,
        }),
    };
    let AutomationAnswer::Suspended(snapshot) = fixture.transport.call(call).await? else {
        return Err("central did not retain shared suspended snapshot".into());
    };
    Ok((fixture, snapshot))
}
fn require_envelope(value: &Value, expected: &SuspendedAutomationSnapshot) -> ProofResult {
    if value.get("kind") != Some(&json!("suspended"))
        || value.get("suspended") != Some(&serde_json::to_value(expected)?)
        || value
            .get("message")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || value.get("value").is_some()
    {
        return Err(format!("suspended public envelope changed shared evidence: {value}").into());
    }
    Ok(())
}
#[tokio::test]
async fn suspended_schedule_show_list_and_export_preserve_the_shared_record() -> ProofResult {
    let (fixture, snapshot) = imported(AutomationKind::Schedule).await?;
    let proof: ProofResult = async {
        let client = fixture.api.client("suspended-schedule").await?;
        let id = snapshot.automation_id.as_str().to_owned().try_into()?;
        require_envelope(
            &serde_json::to_value(
                client
                    .read_schedule(ScheduleShowRequest { schedule_id: id })
                    .await?,
            )?,
            &snapshot,
        )?;
        let page = client
            .list_schedules(AutomationPageRequest {
                cursor: None,
                limit: 1.try_into()?,
            })
            .await?;
        if page.records.len() != 1 {
            return Err("suspended Schedule omitted from listing".into());
        }
        require_envelope(&serde_json::to_value(page.records.first())?, &snapshot)?;
        let exported = fixture
            .api
            .call(
                "schedule_export",
                json!({"scheduleId":snapshot.automation_id}),
            )
            .await?;
        require_envelope(
            exported
                .pointer("/error/data")
                .ok_or("suspended export returned a package or untyped failure")?,
            &snapshot,
        )?;
        Ok(())
    }
    .await;
    let closed = fixture.close().await;
    proof?;
    closed
}
#[tokio::test]
async fn suspended_wake_show_list_and_first_fire_return_without_a_new_firing() -> ProofResult {
    let (fixture, snapshot) = imported(AutomationKind::Wake).await?;
    let proof: ProofResult = async {
        let client = fixture.api.client("suspended-wake").await?;
        let id: WakeupId = snapshot.automation_id.as_str().to_owned().try_into()?;
        require_envelope(
            &serde_json::to_value(
                client
                    .read_wakeup(WakeShowRequest {
                        wakeup_id: id.clone(),
                    })
                    .await?,
            )?,
            &snapshot,
        )?;
        let page = client
            .list_wakeups(AutomationPageRequest {
                cursor: None,
                limit: 1.try_into()?,
            })
            .await?;
        if page.records.len() != 1 {
            return Err("suspended Wake omitted from listing".into());
        }
        require_envelope(&serde_json::to_value(page.records.first())?, &snapshot)?;
        let answer = client
            .clone()
            .subscribe_wakeup(WakeShowRequest { wakeup_id: id })
            .await?
            .wait_until_first_fire()
            .await?;
        require_envelope(&serde_json::to_value(answer)?, &snapshot)?;
        let wait = fixture
            .api
            .call(
                "wake_wait_until_first_fire",
                json!({"wakeupId":snapshot.automation_id,"timeoutSeconds":5}),
            )
            .await?;
        require_envelope(
            wait.get("result")
                .ok_or("suspended wait returned an error")?,
            &snapshot,
        )?;
        let firings = client
            .list_firings(FiringListRequest {
                automation_id: snapshot.automation_id.clone(),
                cursor: None,
                limit: 1.try_into()?,
            })
            .await?;
        if !firings.records.is_empty() {
            return Err("suspended read invented an admitted Firing".into());
        }
        Ok(())
    }
    .await;
    let closed = fixture.close().await;
    proof?;
    closed
}
#[tokio::test]
async fn suspended_schedule_prepare_refuses_before_native_preparation() -> ProofResult {
    let (fixture, snapshot) = imported(AutomationKind::Schedule).await?;
    let proof:ProofResult=async {
        let answer=fixture.api.call("schedule_prepare",json!({"operationId":OperationId::generate(),"scheduleId":snapshot.automation_id,
            "destination":{"kind":"fresh","endpoint":{"serviceId":fixture::SERVICE,"endpointId":"codex-local"},"cwd":"/isolated-prepare"}})).await?;
        if answer.pointer("/error/data/kind")!=Some(&json!("invalidField")) || answer.pointer("/error/data/field")!=Some(&json!("destination")) {
            return Err(format!("suspended preparation did not refuse with a typed recovery action: {answer}").into());
        }
        Ok(())
    }.await;
    let closed = fixture.close().await;
    proof?;
    closed
}
