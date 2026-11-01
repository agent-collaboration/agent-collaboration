//! Converted phase and lifecycle suppress old work independently for each definition.
use super::{
    fixtures::{import_request, required_value, required_value_mut, schedule_payload},
    wake_cases::wake_payload,
};
use crate::{
    AutomationClockControl, ConformanceClient, ConformanceResult,
    automation_catalog_scenarios::{open_machine, raw_call},
    automation_clock_scenarios::fixture,
};
use collaboration_central_core::RegistryTime;
use collaboration_protocol::{AutomationAnswer, AutomationId};
use serde_json::{Value, json};

pub async fn run_automation_handover_lifecycle_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    let service = durable_steps::Uuid::now_v7().to_string();
    let mut machine = open_machine(client, &service).await?;
    let proof: ConformanceResult<()> = async {
        let mut unended = Vec::new();
        for suspended in [false, true] {
            for state in ["active", "paused", "cancelled", "expired", "finished"] {
                let mut payload = wake_payload(&service)?;
                *required_value_mut(&mut payload, "/carriedFiring")? = Value::Null;
                *required_value_mut(&mut payload, "/definition/value/configuration/state")? = json!(state);
                *required_value_mut(&mut payload, "/definition/value/configuration/target/sessionId")? =
                    json!(format!("lifecycle-{suspended}-{state}"));
                if suspended {
                    let configuration = (*required_value(&payload, "/definition/value/configuration")?).clone();
                    *required_value_mut(&mut payload, "/definition")? = json!({"kind":"suspended","value":{
                        "legacyTiming":{"kind":"cron","expression":"58 * 1 * *","timezone":"UTC"},
                        "configuration":{"kind":"wake","value":configuration}
                    }});
                    *required_value_mut(&mut payload, "/firstDueMs")? = Value::Null;
                }
                let id: AutomationId = serde_json::from_value((*required_value(&payload, "/automationId")?).clone())?;
                if !matches!(
                    raw_call(&mut machine, import_request(&payload.to_string())).await?,
                    AutomationAnswer::Imported(_)
                ) {
                    return Err(format!(
                        "valid Wake lifecycle refused: suspended={suspended},state={state}"
                    )
                    .into());
                }
                let row = fixture::automation(client, &id, None).await?;
                let owns_slot = matches!(state, "active" | "paused");
                let expected_state = if suspended && owns_slot {
                    "suspended"
                } else {
                    state
                };
                if (*required_value(&row, "/state")?) != expected_state
                    || (*required_value(&row, "/slotSessionKey")?).is_string() != owns_slot
                    || ((*required_value(&row, "/nextDueMs")?).is_number() != (!suspended && state == "active"))
                    || (*required_value(&row, "/expiresAtMs")?) != json!(1793443200000_i64)
                {
                    return Err(format!(
                        "Wake lifecycle changed at import: suspended={suspended},state={state}"
                    )
                    .into());
                }
                if suspended {
                    let body: Value = serde_json::from_str(
                        (*required_value(&row, "/definitionJson")?)
                            .as_str()
                            .ok_or("suspended body missing")?,
                    )?;
                    if (*required_value(&body, "/value")?).get("timing").is_some()
                        || (*required_value(&body, "/value")?).get("legacyTiming").is_some()
                        || serde_json::from_str::<Value>(
                            (*required_value(&row, "/legacyTimingJson")?)
                                .as_str()
                                .ok_or("legacy timing missing")?,
                        )? != (*required_value(&payload, "/definition/value/legacyTiming")?)
                    {
                        return Err(
                            "suspended body fabricated timing or duplicated legacy authority"
                                .into(),
                        );
                    }
                }
                let shown = serde_json::to_value(raw_call(&mut machine, json!({"kind":"wakeShow","value":{"wakeupId":id}})).await?)?;
                let shown_state = if suspended {
                    shown.pointer("/value/configuration/value/configuration/state")
                } else { shown.pointer("/value/state") };
                if (*required_value(&shown, "/kind")?) != if suspended { "suspended" } else { "wake" }
                    || shown_state != Some(&json!(state)) {
                    return Err(format!("stored Wake lifecycle was not normally readable: suspended={suspended},state={state}").into());
                }
                if owns_slot {
                    unended.push(id);
                }
            }
        }
        let mut disabled = schedule_payload(&service);
        *required_value_mut(&mut disabled, "/definition/value/enabled")? = json!(false);
        let disabled_id: AutomationId = serde_json::from_value((*required_value(&disabled, "/automationId")?).clone())?;
        let mut handled = schedule_payload(&service);
        *required_value_mut(&mut handled, "/admittedThroughMs")? = (*required_value(&handled, "/firstDueMs")?).clone();
        let handled_id: AutomationId = serde_json::from_value((*required_value(&handled, "/automationId")?).clone())?;
        let mut phases = Vec::new();
        for (due, watermark) in [
            (1793441100000_i64, 1793440920000_i64),
            (1793441400000_i64, 1793441220000_i64),
        ] {
            let mut payload = schedule_payload(&service);
            *required_value_mut(&mut payload, "/definition/value/timing")? = json!({"kind":"interval","seconds":600});
            *required_value_mut(&mut payload, "/firstDueMs")? = json!(due);
            *required_value_mut(&mut payload, "/admittedThroughMs")? = json!(due);
            *required_value_mut(&mut payload, "/watermarkLocalMs")? = json!(watermark);
            let id: AutomationId = serde_json::from_value((*required_value(&payload, "/automationId")?).clone())?;
            if !matches!(
                raw_call(&mut machine, import_request(&payload.to_string())).await?,
                AutomationAnswer::Imported(_)
            ) {
                return Err("converted phase import refused".into());
            }
            let row = fixture::automation(client, &id, None).await?;
            let body: Value = serde_json::from_str(
                (*required_value(&row, "/definitionJson")?)
                    .as_str()
                    .ok_or("phase definition missing")?,
            )?;
            if (*required_value(&body, "/value/timing/firstDue")?) != json!(due)
                || (*required_value(&row, "/admittedThroughMs")?) != json!(due)
                || (*required_value(&row, "/importedWatermarkLocalMs")?) != json!(watermark)
            {
                return Err(
                    "per-definition converted phase or admitted-through was recomputed".into(),
                );
            }
            phases.push((id, due));
        }
        for payload in [disabled, handled] {
            if !matches!(
                raw_call(&mut machine, import_request(&payload.to_string())).await?,
                AutomationAnswer::Imported(_)
            ) {
                return Err("disabled/already-handled one-shot import refused".into());
            }
        }
        for time in [
            "2026-10-31T10:04:59.999Z",
            "2026-10-31T10:05:00Z",
            "2026-10-31T10:10:00Z",
            "2026-10-31T10:20:00Z",
        ] {
            client
                .automation_clock(AutomationClockControl::Advance {
                    now: RegistryTime(time.parse()?),
                })
                .await?;
            client
                .automation_clock(AutomationClockControl::InvokeAlarm)
                .await?;
            let now = chrono::DateTime::parse_from_rfc3339(time)?.timestamp_millis();
            for (id, phase) in &phases {
                let row = fixture::automation(client, id, None).await?;
                let firings = (*required_value(&row, "/firings")?).as_array().ok_or("phase Firings missing")?;
                if now < phase + 600_000 {
                    if !firings.is_empty() {
                        return Err(
                            "an already-handled first due or an early occurrence refired".into(),
                        );
                    }
                } else if firings.len() != 1
                    || (*required_value(firings.first().ok_or("first phase Firing missing")?, "/dueMs")?)
                        != json!(phase + 600_000)
                {
                    return Err("converted definitions lost their independent phases".into());
                }
            }
        }
        for id in [&disabled_id, &handled_id] {
            if (*required_value(&fixture::automation(client, id, None).await?, "/firings")?)
                .as_array()
                .map(Vec::len)
                != Some(0)
            {
                return Err("disabled or already-handled one-shot admitted work".into());
            }
        }
        client
            .automation_clock(AutomationClockControl::Advance {
                now: RegistryTime("2026-10-31T10:40:00Z".parse()?),
            })
            .await?;
        for id in unended {
            let row = fixture::automation(client, &id, None).await?;
            if (*required_value(&row, "/state")?) != "expired"
                || !(*required_value(&row, "/slotSessionKey")?).is_null()
                || !(*required_value(&row, "/nextDueMs")?).is_null()
                || (*required_value(&row, "/expiresAtMs")?) != json!(1793443200000_i64)
            {
                return Err(
                    "unended imported Wake extended its end or retained its expired slot".into(),
                );
            }
        }
        Ok(())
    }
    .await;
    let closed = machine.close().await;
    proof?;
    closed?;
    Ok(())
}
