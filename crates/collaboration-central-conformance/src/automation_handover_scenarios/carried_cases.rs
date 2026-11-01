//! A suspended move keeps already-admitted work without inventing a live timer.
use super::fixtures::{import_request, required_value, required_value_mut, schedule_payload};
use crate::{
    AutomationClockControl, ConformanceClient, ConformanceResult,
    automation_catalog_scenarios::{open_machine, raw_call},
    automation_clock_scenarios::fixture,
};
use collaboration_central_core::RegistryTime;
use collaboration_protocol::AutomationId;
use serde_json::{Value, json};

pub async fn run_automation_handover_carried_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    let service = durable_steps::Uuid::now_v7().to_string();
    let mut machine = open_machine(client, &service).await?;
    let proof: ConformanceResult<()> = async {
        let mut payload = schedule_payload(&service);
        let id: AutomationId = serde_json::from_value((*required_value(&payload, "/automationId")?).clone())?;
        let mut configuration = (*required_value(&payload, "/definition/value")?).clone();
        configuration
            .as_object_mut()
            .ok_or("schedule configuration missing")?
            .remove("timing");
        *required_value_mut(&mut payload, "/definition")? = json!({"kind":"suspended","value":{
            "legacyTiming":{"kind":"cron","expression":"58 * 1 * *","timezone":"UTC"},
            "configuration":{"kind":"schedule","value":configuration}
        }});
        *required_value_mut(&mut payload, "/firstDueMs")? = Value::Null;
        *required_value_mut(&mut payload, "/carriedWaitingDueMs")? = json!(1793441100000_i64);
        let answer = raw_call(&mut machine, import_request(&payload.to_string())).await?;
        if serde_json::to_value(answer)? != json!({"kind":"imported","value":{"automationId":id}}) {
            return Err("suspended move did not retain its already-admitted waiter".into());
        }
        let pending = fixture::automation(client, &id, None).await?;
        if (*required_value(&pending, "/state")?) != "suspended"
            || !(*required_value(&pending, "/nextDueMs")?).is_null()
            || (*required_value(&pending, "/firings")?).as_array().map(Vec::len) != Some(1)
            || (*required_value(&pending, "/firings/0/state")?) != "pending"
            || !(*required_value(&pending, "/firings/0/batchId")?).is_null()
        {
            return Err("suspended move fabricated a live deadline or dispatched early".into());
        }
        let clock = client
            .automation_clock(AutomationClockControl::Observe)
            .await?;
        if clock.next_alarm_ms != Some(1793441100000_i64) {
            return Err("owed carried entry did not own its fitted future deadline".into());
        }
        client
            .automation_clock(AutomationClockControl::InvokeAlarm)
            .await?;
        client
            .automation_clock(AutomationClockControl::Advance {
                now: RegistryTime("2026-10-31T10:04:59.999Z".parse()?),
            })
            .await?;
        if (*required_value(&fixture::automation(client, &id, None).await?, "/firings")?) != (*required_value(&pending, "/firings")?) {
            return Err("an early clock callback released the carried waiter".into());
        }
        client
            .automation_clock(AutomationClockControl::Advance {
                now: RegistryTime("2026-10-31T10:05:00Z".parse()?),
            })
            .await?;
        let released = fixture::automation(client, &id, None).await?;
        if (*required_value(&released, "/state")?) != "suspended"
            || !(*required_value(&released, "/nextDueMs")?).is_null()
            || (*required_value(&released, "/firings")?).as_array().map(Vec::len) != Some(1)
            || !(*required_value(&released, "/firings/0/batchId")?).is_string()
            || !(*required_value(&released, "/firings/0/pendingEntryJson")?).is_null()
        {
            return Err(
                "the owed waiter was not frozen once while catalog stayed suspended".into(),
            );
        }
        client
            .automation_clock(AutomationClockControl::InvokeAlarm)
            .await?;
        client
            .automation_clock(AutomationClockControl::Advance {
                now: RegistryTime("2026-11-01T10:05:00Z".parse()?),
            })
            .await?;
        let repeated = fixture::automation(client, &id, None).await?;
        let mut held = (*required_value(&released, "/firings")?).clone();
        *required_value_mut(&mut held, "/0/state")? = json!("held");
        // The real notice receipt advances custody; identities and frozen bytes stay fixed.
        if (*required_value(&repeated, "/firings")?) != held || (*required_value(&repeated, "/batches")?) != (*required_value(&released, "/batches")?)
        {
            eprintln!("S7_OWED_REPEAT_DIAGNOSTIC {}", json!({"releasedFirings":(*required_value(&released, "/firings")?),"repeatedFirings":(*required_value(&repeated, "/firings")?),"releasedBatches":(*required_value(&released, "/batches")?),"repeatedBatches":(*required_value(&repeated, "/batches")?)}));
            return Err(
                "suspended definition admitted another occurrence or resent the owed one".into(),
            );
        }
        Ok(())
    }
    .await;
    let closed = machine.close().await;
    proof?;
    closed?;
    Ok(())
}
