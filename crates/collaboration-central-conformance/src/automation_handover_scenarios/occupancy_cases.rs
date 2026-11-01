//! A normal import owns external occupancy and the retained waiting entry.
use super::{
    fixtures::{import_request, required_value, required_value_mut, schedule_payload},
    receipt_cases::unchanged,
};
use crate::{
    AutomationClockControl, ConformanceClient, ConformanceResult,
    automation_catalog_scenarios::{open_machine, raw_call},
    automation_clock_scenarios::fixture,
};
use collaboration_central_core::RegistryTime;
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationId, LocalRunOutcome, LocalRunSettled,
};
use serde_json::json;

pub async fn run_automation_handover_external_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    let importer = durable_steps::Uuid::now_v7().to_string();
    let target = durable_steps::Uuid::now_v7().to_string();
    let mut first = open_machine(client, &importer).await?;
    let mut second = match open_machine(client, &target).await {
        Ok(second) => second,
        Err(error) => {
            first.close().await?;
            return Err(error);
        }
    };
    let proof: ConformanceResult<()> = async {
        for future in [true, false] {
            let mut payload = schedule_payload(&target);
            let id: AutomationId =
                serde_json::from_value((*required_value(&payload, "/automationId")?).clone())?;
            let run = durable_steps::Uuid::now_v7().to_string();
            *required_value_mut(&mut payload, "/definition/value/timing")? =
                json!({"kind":"at","at":"2026-10-31T10:20:00Z"});
            *required_value_mut(&mut payload, "/firstDueMs")? = json!(1793442000000_i64);
            *required_value_mut(&mut payload, "/externalOccupant")? = json!(run);
            *required_value_mut(&mut payload, "/carriedWaitingDueMs")? = json!(1793441100000_i64);
            if !matches!(
                raw_call(&mut first, import_request(&payload.to_string())).await?,
                AutomationAnswer::Imported(_)
            ) {
                return Err("ordinary external-occupant import refused".into());
            }
            let pending = fixture::automation(client, &id, None).await?;
            if (*required_value(&pending, "/externalRunId")?) != run
                || !(*required_value(&pending, "/occupyingFiringId")?).is_null()
                || (*required_value(&pending, "/firings")?)
                    .as_array()
                    .map(Vec::len)
                    != Some(1)
                || (*required_value(&pending, "/waitingFiringId")?)
                    != (*required_value(&pending, "/firings/0/firingId")?)
                || !(*required_value(&pending, "/firings/0/batchId")?).is_null()
            {
                eprintln!(
                    "HANDOVER_EXTERNAL_IMPORT_MISMATCH {}",
                    json!({"actual":pending,"payload":payload})
                );
                return Err("import dropped or dispatched externally occupied waiting work".into());
            }
            let settle = LocalRunSettled {
                automation_id: id.clone(),
                run_id: run.try_into()?,
                outcome: LocalRunOutcome::Finished,
            };
            let request = serde_json::to_value(AutomationCall::LocalRunSettled(settle.clone()))?;
            let refusal = json!({"kind":"refused","value":{"kind":"originMismatch"}});
            let mut wrong = settle.clone();
            wrong.run_id = durable_steps::Uuid::now_v7().to_string().try_into()?;
            unchanged(
                client,
                &mut first,
                &id,
                serde_json::to_value(AutomationCall::LocalRunSettled(wrong))?,
                &refusal,
            )
            .await?;
            unchanged(client, &mut second, &id, request.clone(), &refusal).await?;
            if future {
                fixture::automation(
                    client,
                    &id,
                    Some("{invalid live definition after retained admission".into()),
                )
                .await?;
                client
                    .automation_clock(AutomationClockControl::InvokeAlarm)
                    .await?;
                if !(*required_value(&fixture::automation(client, &id, None).await?, "/faultJson")?)
                    .is_string()
                {
                    return Err(
                        "external-waiter live definition did not fault through the real clock"
                            .into(),
                    );
                }
            }
            if !matches!(
                raw_call(&mut first, request.clone()).await?,
                AutomationAnswer::Acknowledged
            ) {
                return Err("authentic external settlement refused".into());
            }
            let promoted = fixture::automation(client, &id, None).await?;
            if !(*required_value(&promoted, "/externalRunId")?).is_null()
                || !(*required_value(&promoted, "/waitingFiringId")?).is_null()
                || (*required_value(&promoted, "/occupyingFiringId")?)
                    != (*required_value(&pending, "/firings/0/firingId")?)
            {
                return Err("settlement did not promote the retained identity".into());
            }
            if future {
                if !(*required_value(&promoted, "/firings/0/batchId")?).is_null() {
                    return Err("future external waiter crossed cutoff at settlement".into());
                }
                if client
                    .automation_clock(AutomationClockControl::Observe)
                    .await?
                    .next_alarm_ms
                    != Some(1793441100000_i64)
                {
                    return Err("promoted future waiter failed to arm its own due".into());
                }
                client
                    .automation_clock(AutomationClockControl::InvokeAlarm)
                    .await?;
                if !(*required_value(
                    &fixture::automation(client, &id, None).await?,
                    "/firings/0/batchId",
                )?)
                .is_null()
                {
                    return Err("an early callback sent the promoted future waiter".into());
                }
                client
                    .automation_clock(AutomationClockControl::Advance {
                        now: RegistryTime("2026-10-31T10:05:00Z".parse()?),
                    })
                    .await?;
            }
            let released = fixture::automation(client, &id, None).await?;
            if !(*required_value(&released, "/firings/0/batchId")?).is_string()
                || (*required_value(&released, "/firings/0/firingId")?)
                    != (*required_value(&pending, "/firings/0/firingId")?)
                || !(*required_value(&released, "/firings/0/pendingEntryJson")?).is_null()
            {
                return Err("due external waiter did not release from its retained entry".into());
            }
            if future && !(*required_value(&released, "/faultJson")?).is_string() {
                return Err("owed work silently repaired the corrupt live definition".into());
            }
            raw_call(&mut first, request).await?;
            client
                .automation_clock(AutomationClockControl::InvokeAlarm)
                .await?;
            let duplicate = fixture::automation(client, &id, None).await?;
            if (*required_value(&duplicate, "/batches")?)
                != (*required_value(&released, "/batches")?)
                || (*required_value(&duplicate, "/firings")?)
                    .as_array()
                    .map(Vec::len)
                    != Some(1)
                || (*required_value(&duplicate, "/firings/0/batchId")?)
                    != (*required_value(&released, "/firings/0/batchId")?)
            {
                return Err("duplicate external settlement minted a second Batch/Firing".into());
            }
        }
        Ok(())
    }
    .await;
    let first_closed = first.close().await;
    let second_closed = second.close().await;
    proof?;
    first_closed?;
    second_closed?;
    Ok(())
}
