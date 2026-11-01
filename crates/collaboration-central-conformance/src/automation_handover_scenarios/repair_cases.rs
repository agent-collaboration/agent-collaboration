//! Suspended inspection and repair use the existing catalog mutation contracts.
use super::{
    fixtures::{import_request, required_value, required_value_mut, schedule_payload},
    receipt_cases::unchanged,
    wake_cases::wake_payload,
};
use crate::{
    AutomationClockControl, ConformanceClient, ConformanceResult,
    automation_catalog_scenarios::{open_machine, raw_call},
    automation_clock_scenarios::fixture,
    native_board_session::NativeBoardSession,
};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationId, CatalogAutomation, ExecutionDestination,
    ExpiryRequest, MessageContent, MessageDelivery, SavedMessage, SessionRef, TimingRequest,
    WakeCreator, WakeSendRequest,
};
use serde_json::{Value, json};

pub async fn run_automation_handover_repair_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    let importer = durable_steps::Uuid::now_v7().to_string();
    let destination = durable_steps::Uuid::now_v7().to_string();
    let mut source = open_machine(client, &importer).await?;
    let mut target = match open_machine(client, &destination).await {
        Ok(target) => target,
        Err(error) => {
            source.close().await?;
            return Err(error);
        }
    };
    let proof:ConformanceResult<()>=async {
        if !matches!(raw_call(&mut target,json!({"kind":"pullAutomationBatches","value":{"after":null,"maxBytes":1048576,"workProtocols":["machineWorkV1"]}})).await?,AutomationAnswer::Batches(ref page) if page.batches.is_empty()){return Err("repair target capability advertisement was not initially empty".into());}
        for enabled in [true,false]{repair_schedule(client,&mut source,&destination,enabled).await?;}
        for faulted in [false,true]{replace_wake(client,&mut source,&mut target,&destination,faulted).await?;}
        Ok(())
    }.await;
    let source_closed = source.close().await;
    let target_closed = target.close().await;
    proof?;
    source_closed?;
    target_closed?;
    println!(
        "HANDOVER_REPAIR_RESULT suspended Schedule enabled/disabled header+mode kept; suspended and faulted imported Wake authorized replacement passed"
    );
    Ok(())
}

async fn repair_schedule(
    client: &ConformanceClient,
    machine: &mut NativeBoardSession,
    destination: &str,
    enabled: bool,
) -> ConformanceResult<()> {
    let mut payload = schedule_payload(destination);
    *required_value_mut(&mut payload, "/definition/value/enabled")? = json!(enabled);
    let mut configuration = required_value(&payload, "/definition/value")?.clone();
    configuration
        .as_object_mut()
        .ok_or("Schedule configuration missing")?
        .remove("timing")
        .ok_or("Schedule timing missing")?;
    let legacy = json!({"kind":"cron","expression":"58 * 1 * *","timezone":"UTC"});
    *required_value_mut(&mut payload, "/definition")? = json!({"kind":"suspended","value":{"legacyTiming":legacy,"configuration":{"kind":"schedule","value":configuration}}});
    *required_value_mut(&mut payload, "/firstDueMs")? = Value::Null;
    let id: AutomationId =
        serde_json::from_value(required_value(&payload, "/automationId")?.clone())?;
    if serde_json::to_value(raw_call(machine, import_request(&payload.to_string())).await?)?
        != json!({"kind":"imported","value":{"automationId":id}})
    {
        return Err("repair fixture did not import its suspended Schedule identity".into());
    }
    let shown = serde_json::to_value(
        raw_call(
            machine,
            json!({"kind":"scheduleShow","value":{"scheduleId":id}}),
        )
        .await?,
    )?;
    if shown
        != json!({"kind":"suspended","value":{"automationId":id,"kind":"schedule","revision":required_value(&payload,"/revision")?,"legacyTiming":legacy,"configuration":{"kind":"schedule","value":configuration},"header":required_value(&payload,"/header")?}})
    {
        return Err("normal suspended Schedule inspection lost preserved configuration/header or fabricated timing".into());
    }
    let before = fixture::automation(client, &id, None).await?;
    let expected_rows = required_value(&before, "/counts/automations")?
        .as_u64()
        .ok_or("catalog count missing")?;
    if expected_rows > 100 {
        return Err("repair catalog exceeded its bounded fixture".into());
    }
    let mut cursor = None;
    let mut seen = std::collections::BTreeSet::new();
    let mut matches = 0;
    let mut terminal = false;
    for _ in 0..=expected_rows {
        let page = raw_call(
            machine,
            json!({"kind":"list","value":{"cursor":cursor,"limit":1}}),
        )
        .await?;
        let AutomationAnswer::List { automations, next } = page else {
            return Err("suspended catalog page returned the wrong answer family".into());
        };
        if automations.len() > 1 {
            return Err("suspended catalog page exceeded its bound".into());
        }
        for record in automations {
            let key = match &record {
                CatalogAutomation::Schedule(snapshot) => snapshot.schedule_id.as_str(),
                CatalogAutomation::Wake(snapshot) => snapshot.definition.wakeup_id.as_str(),
                CatalogAutomation::Faulted(snapshot) => snapshot.automation_id.as_str(),
                CatalogAutomation::Suspended(snapshot) => snapshot.automation_id.as_str(),
            }
            .to_owned();
            if !seen.insert(key) {
                return Err("mixed suspended catalog repeated a record".into());
            }
            if serde_json::to_value(record)? == shown {
                matches += 1;
            }
        }
        match next {
            None => {
                terminal = true;
                break;
            }
            Some(next) if cursor.as_ref() != Some(&next) => cursor = Some(next),
            Some(_) => return Err("mixed suspended catalog cursor did not advance".into()),
        }
    }
    if !terminal || u64::try_from(seen.len())? != expected_rows || matches != 1 {
        return Err(
            "normal List lost, duplicated or fabricated the preserved suspended snapshot".into(),
        );
    }
    let mut definition = configuration.clone();
    definition
        .as_object_mut()
        .ok_or("repair definition missing")?
        .insert(
            "timing".into(),
            json!({"kind":"at","at":"2026-10-31T10:10:00Z"}),
        );
    *required_value_mut(&mut definition, "/enabled")? = json!(!enabled);
    *required_value_mut(&mut definition, "/effort")? = json!("low");
    let mut wrong_mode = definition.clone();
    let endpoint = serde_json::from_value(
        required_value(&definition, "/destination/target/endpoint")?.clone(),
    )?;
    *required_value_mut(&mut wrong_mode, "/destination")? =
        serde_json::to_value(ExecutionDestination::FreshEachRun {
            endpoint,
            cwd: "/fixture".into(),
        })?;
    *required_value_mut(&mut wrong_mode, "/model")? = json!("fixture-model");
    let expected_revision = required_value(&payload, "/revision")?.clone();
    let change = |definition: Value| json!({"kind":"scheduleUpdate","value":{"operationId":durable_steps::Uuid::now_v7().to_string(),"scheduleId":id,"expectedChangeId":expected_revision,"definition":definition}});
    unchanged(client,machine,&id,change(wrong_mode),&json!({"kind":"refused","value":{"kind":"invalidRequest","field":"destination","reason":"executionModeFixed"}})).await?;
    let answer = raw_call(machine, change(definition.clone())).await?;
    let AutomationAnswer::Mutation {
        automation: CatalogAutomation::Schedule(snapshot),
        already_sent,
    } = answer
    else {
        return Err(format!("suspended Schedule repair refused: {answer:?}").into());
    };
    *required_value_mut(&mut definition, "/enabled")? = json!(enabled);
    if snapshot.schedule_id.as_str() != id.as_str()
        || json!(snapshot.change_id) == *required_value(&payload, "/revision")?
        || serde_json::to_value(&snapshot.definition)? != definition
        || !already_sent.is_empty()
    {
        return Err(
            "suspended repair changed identity/prior enabled state or lost its new revision".into(),
        );
    }
    let after = fixture::automation(client, &id, None).await?;
    for name in [
        "anchorAtMs",
        "createdAtMs",
        "updatedAtMs",
        "creatorJson",
        "importedContinuityJson",
        "executionMode",
        "importedWatermarkLocalMs",
        "admittedThroughMs",
    ] {
        if required_value(&before, &format!("/{name}"))?
            != required_value(&after, &format!("/{name}"))?
        {
            return Err(format!("Schedule repair changed retained {name}").into());
        }
    }
    if !required_value(&after, "/legacyTimingJson")?.is_null()
        || !required_value(&after, "/faultJson")?.is_null()
        || *required_value(&after, "/state")? != if enabled { "enabled" } else { "disabled" }
        || *required_value(&after, "/nextDueMs")?
            != if enabled {
                json!(1793441400000_i64)
            } else {
                Value::Null
            }
    {
        return Err(
            "Schedule repair did not clear legacy timing and restore only its prior eligibility"
                .into(),
        );
    }
    println!(
        "HANDOVER_SCHEDULE_REPAIR {}",
        json!({"automationId":id,"priorEnabled":enabled,"before":before,"after":after,"snapshot":snapshot})
    );
    Ok(())
}

async fn replace_wake(
    client: &ConformanceClient,
    importer: &mut NativeBoardSession,
    target: &mut NativeBoardSession,
    destination: &str,
    faulted: bool,
) -> ConformanceResult<()> {
    let mut payload = wake_payload(destination)?;
    *required_value_mut(&mut payload, "/carriedFiring")? = Value::Null;
    *required_value_mut(
        &mut payload,
        "/definition/value/configuration/target/sessionId",
    )? = json!(format!("suspended-replacement-{faulted}"));
    let configuration = required_value(&payload, "/definition/value/configuration")?.clone();
    if !faulted {
        let legacy = json!({"kind":"cron","expression":"9,58 10 * * *","timezone":"UTC"});
        *required_value_mut(&mut payload, "/definition")? = json!({"kind":"suspended","value":{"legacyTiming":legacy,"configuration":{"kind":"wake","value":configuration}}});
        *required_value_mut(&mut payload, "/firstDueMs")? = Value::Null;
    }
    let id: AutomationId =
        serde_json::from_value(required_value(&payload, "/automationId")?.clone())?;
    if serde_json::to_value(raw_call(importer, import_request(&payload.to_string())).await?)?
        != json!({"kind":"imported","value":{"automationId":id}})
    {
        return Err("replacement fixture did not import its preserved Wake identity".into());
    }
    if faulted {
        fixture::automation(
            client,
            &id,
            Some("{corrupt active imported Wake body; provenance remains independent".into()),
        )
        .await?;
        client
            .automation_clock(AutomationClockControl::InvokeAlarm)
            .await?;
        let faulted_row = fixture::automation(client, &id, None).await?;
        if !required_value(&faulted_row, "/faultJson")?.is_string() {
            return Err("active imported Wake did not fault through the real clock".into());
        }
    }
    let old = fixture::automation(client, &id, None).await?;
    let session: SessionRef =
        serde_json::from_value(required_value(&configuration, "/target")?.clone())?;
    let mut request = WakeSendRequest {
        operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
        requested_by: WakeCreator::Router {},
        replace: true,
        message: SavedMessage {
            target: session.clone(),
            content: MessageContent::HumanUser {
                text: "replacement of suspended central Wake"
                    .to_owned()
                    .try_into()?,
            },
            delivery: MessageDelivery::Auto,
            generation_guard: None,
        },
        timing: TimingRequest::At {
            at: "2026-10-31T10:05:00Z".to_owned().try_into()?,
        },
        expiry: ExpiryRequest::At {
            at: "2026-10-31T10:20:00Z".to_owned().try_into()?,
        },
    };
    unchanged(client,importer,&id,serde_json::to_value(AutomationCall::WakeSend(request.clone()))?,&json!({"kind":"refused","value":{"kind":"wakeExists","creator":required_value(&payload,"/creator")?}})).await?;
    request.operation_id = durable_steps::Uuid::now_v7().to_string().try_into()?;
    request.requested_by = WakeCreator::Session(session);
    let answer = raw_call(
        target,
        serde_json::to_value(AutomationCall::WakeSend(request))?,
    )
    .await?;
    let AutomationAnswer::Wake(replacement) = answer else {
        return Err(format!("authorized suspended Wake replacement refused: {answer:?}").into());
    };
    if replacement.definition.wakeup_id.as_str() == id.as_str()
        || replacement.state != collaboration_protocol::WakeState::Active
    {
        return Err("suspended Wake replacement reused old identity or failed to activate".into());
    }
    let ended = fixture::automation(client, &id, None).await?;
    if *required_value(&ended, "/state")? != if faulted { "replaced" } else { "cancelled" }
        || !required_value(&ended, "/slotSessionKey")?.is_null()
        || !required_value(&ended, "/nextDueMs")?.is_null()
        || required_value(&ended, "/faultJson")? != required_value(&old, "/faultJson")?
        || required_value(&ended, "/expiresAtMs")? != required_value(&old, "/expiresAtMs")?
        || required_value(&ended, "/legacyTimingJson")?
            != required_value(&old, "/legacyTimingJson")?
        || required_value(&ended, "/definitionJson")? != required_value(&old, "/definitionJson")?
        || required_value(&ended, "/creatorJson")? != required_value(&old, "/creatorJson")?
        || required_value(&ended, "/firings")?.as_array().map(Vec::len) != Some(0)
    {
        return Err("suspended Wake replacement lost its end/fault/legacy history or left old executable work".into());
    }
    println!(
        "HANDOVER_WAKE_REPLACEMENT {}",
        json!({"faulted":faulted,"old":old,"ended":ended,"replacement":replacement})
    );
    Ok(())
}
