//! Inner-string validation and maximum escaped content use the ordinary transport.
use super::{
    fixtures::{import_request, required_value, required_value_mut, schedule_payload},
    receipt_cases::{require_record, unchanged},
    wake_cases::wake_payload,
};
use crate::{
    ConformanceClient, ConformanceResult,
    automation_catalog_scenarios::{open_machine, raw_call},
    automation_clock_scenarios::fixture,
};
use collaboration_central_core::AutomationFixtureMutation;
use collaboration_protocol::{
    AutomationAnswer, AutomationId, AutomationRefusal, AutomationValidationError, ContentId,
    limits::{LimitExceeded, LimitName},
};
use serde_json::{Value, json};

pub async fn run_automation_handover_validation_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    let service = durable_steps::Uuid::now_v7().to_string();
    let mut machine = open_machine(client, &service).await?;
    let proof = validate(client, &mut machine, &service).await;
    let closed = machine.close().await;
    proof?;
    closed?;
    Ok(())
}

async fn validate(
    client: &ConformanceClient,
    machine: &mut crate::native_board_session::NativeBoardSession,
    service: &str,
) -> ConformanceResult<()> {
    const CONTENT_BYTES: usize = 1_048_576;
    let unit = "\"\\雪🙂\n";
    let mut text = unit.repeat(CONTENT_BYTES / unit.len());
    text.push_str(&"x".repeat(CONTENT_BYTES - text.len()));
    let mut large = schedule_payload(service);
    *required_value_mut(&mut large, "/content/text")? = json!(text);
    *required_value_mut(&mut large, "/definition/value/destination/target/sessionId")? =
        json!(format!("{}a", "雪".repeat(1365)));
    *required_value_mut(
        &mut large,
        "/definition/value/destination/target/endpoint/endpointId",
    )? = json!("b".repeat(64));
    *required_value_mut(&mut large, "/definition/value/destination/cwd")? =
        json!(format!("/{}", "c".repeat(4095)));
    let id: AutomationId =
        serde_json::from_value(required_value(&large, "/automationId")?.clone())?;
    let frozen = serde_json::to_string_pretty(&large)?;
    let request = import_request(&frozen);
    let outer_bytes = serde_json::to_vec(&request)?.len();
    if required_value(&large, "/content/text")?
        .as_str()
        .map(str::len)
        != Some(CONTENT_BYTES)
        || required_value(&large, "/definition/value/destination/target/sessionId")?
            .as_str()
            .map(str::len)
            != Some(4096)
        || frozen.len() <= CONTENT_BYTES
        || outer_bytes <= frozen.len()
    {
        return Err("escaped UTF-8 maximum fixture lost its byte boundaries".into());
    }
    let imported = raw_call(machine, request.clone()).await?;
    if serde_json::to_value(imported)? != json!({"kind":"imported","value":{"automationId":id}}) {
        return Err("maximum escaped UTF-8 import did not traverse the real call".into());
    }
    require_record(
        &fixture::automation(client, &id, None).await?,
        &large,
        service,
        &frozen,
        required_value(&request, "/value/importId")?,
    )?;
    unchanged(
        client,
        machine,
        &id,
        request,
        &json!({"kind":"alreadyImported","value":{"automationId":id}}),
    )
    .await?;
    unchanged(
        client,
        machine,
        &id,
        import_request(&format!("{frozen}\n")),
        &json!({"kind":"refused","value":{"kind":"importConflict","automationId":id}}),
    )
    .await?;

    let base = schedule_payload(service);
    let malformed = json!({"kind":"refused","value":{"kind":"invalidRequest","field":"call","reason":"malformedShape"}});
    let mut invalid_requests = vec![
        import_request("{"),
        import_request("null"),
        import_request("[]"),
    ];
    let mut object_payload = import_request(&base.to_string());
    *required_value_mut(&mut object_payload, "/value/payload")? = base.clone();
    invalid_requests.push(object_payload);
    for name in [
        "watermarkLocalMs",
        "firstDueMs",
        "admittedThroughMs",
        "externalOccupant",
        "carriedWaitingDueMs",
        "carriedFiring",
    ] {
        let mut missing = base.clone();
        missing
            .as_object_mut()
            .ok_or("inner payload object missing")?
            .remove(name)
            .ok_or("required nullable field absent from fixture")?;
        invalid_requests.push(import_request(&missing.to_string()));
    }
    for (pointer, value) in [
        ("/content/text", Value::Null),
        ("/content/text", json!(42)),
        ("/automationId", Value::Null),
        ("/firstDueMs", json!("five")),
        ("/header/updatedAt", json!("2026-10-31T09:30:00.1234Z")),
        ("/definition/kind", json!("unknown")),
    ] {
        let mut invalid = base.clone();
        *required_value_mut(&mut invalid, pointer)? = value;
        invalid_requests.push(import_request(&invalid.to_string()));
    }
    let mut unknown = base.clone();
    unknown
        .as_object_mut()
        .ok_or("inner object missing")?
        .insert("sourceServiceId".into(), json!(service));
    invalid_requests.push(import_request(&unknown.to_string()));
    let mut unit_extra = base.clone();
    required_value_mut(&mut unit_extra, "/creator")?
        .as_object_mut()
        .ok_or("creator object missing")?
        .insert("extra".into(), json!(true));
    invalid_requests.push(import_request(&unit_extra.to_string()));
    let mut wake = wake_payload(service)?;
    *required_value_mut(&mut wake, "/content/text")? = json!("forbidden\u{1}wake control");
    invalid_requests.push(import_request(&wake.to_string()));
    let malformed_count = invalid_requests.len();
    for request in invalid_requests {
        unchanged(client, machine, &id, request, &malformed).await?;
    }

    for (pointer, text, limit, value, maximum) in [
        (
            "/content/text",
            "x".repeat(CONTENT_BYTES + 1),
            LimitName::InstructionMaxBytes,
            1_048_577,
            1_048_576,
        ),
        (
            "/definition/value/destination/target/sessionId",
            format!("{}ab", "雪".repeat(1365)),
            LimitName::SessionIdMaxBytes,
            4097,
            4096,
        ),
        (
            "/definition/value/destination/target/endpoint/endpointId",
            "b".repeat(65),
            LimitName::EndpointIdMaxBytes,
            65,
            64,
        ),
    ] {
        let mut invalid = base.clone();
        *required_value_mut(&mut invalid, pointer)? = json!(text);
        unchanged(
            client,
            machine,
            &id,
            import_request(&invalid.to_string()),
            &limit_answer(limit, value, maximum)?,
        )
        .await?;
    }
    let mut unsafe_counter = wake_payload(service)?;
    *required_value_mut(
        &mut unsafe_counter,
        "/definition/value/configuration/generationGuard",
    )? = json!({"serviceEpoch":durable_steps::Uuid::now_v7().to_string(),"generation":9_007_199_254_740_992_u64});
    unchanged(
        client,
        machine,
        &id,
        import_request(&unsafe_counter.to_string()),
        &limit_answer(
            LimitName::JsonSafeInteger,
            9_007_199_254_740_992,
            9_007_199_254_740_991,
        )?,
    )
    .await?;
    let mut off_grid = base.clone();
    *required_value_mut(&mut off_grid, "/definition/value/timing")? =
        json!({"kind":"at","at":"2026-10-31T10:05:01Z"});
    let grid_answer =
        serde_json::to_value(AutomationAnswer::Refused(AutomationRefusal::Validation {
            error: AutomationValidationError::OffGrid {
                field: "at".into(),
                earlier: "2026-10-31T10:05:00Z".into(),
                later: "2026-10-31T10:10:00Z".into(),
            },
        }))?;
    unchanged(
        client,
        machine,
        &id,
        import_request(&off_grid.to_string()),
        &grid_answer,
    )
    .await?;

    let small_id: AutomationId =
        serde_json::from_value(required_value(&base, "/automationId")?.clone())?;
    let request = import_request(&base.to_string());
    if serde_json::to_value(raw_call(machine, request.clone()).await?)?
        != json!({"kind":"imported","value":{"automationId":small_id}})
    {
        return Err("corrected request remained blocked after malformed inner payloads".into());
    }
    let stored = fixture::automation(client, &small_id, None).await?;
    require_record(
        &stored,
        &base,
        service,
        required_value(&request, "/value/payload")?
            .as_str()
            .ok_or("corrected frozen bytes missing")?,
        required_value(&request, "/value/importId")?,
    )?;
    let revision = required_value(&base, "/content/revisionId")?;
    let content = required_value(&stored, "/contentRevisions")?
        .as_array()
        .ok_or("stored revisions missing")?
        .iter()
        .find(|row| row.get("revisionId") == Some(revision))
        .ok_or("stored revision absent")?;
    let content_id: ContentId =
        serde_json::from_value(required_value(content, "/contentId")?.clone())?;
    fixture::mutate(
        client,
        &small_id,
        AutomationFixtureMutation::ContentText {
            content_id: content_id.clone(),
            text: "invalid\0stored instruction".into(),
        },
    )
    .await?;
    let mut shared = base.clone();
    *required_value_mut(&mut shared, "/automationId")? =
        json!(durable_steps::Uuid::now_v7().to_string());
    let shared_request = import_request(&shared.to_string());
    unchanged(
        client,
        machine,
        &id,
        shared_request.clone(),
        &json!({"kind":"refused","value":{"kind":"invalidRecord","field":"text"}}),
    )
    .await?;
    fixture::mutate(
        client,
        &small_id,
        AutomationFixtureMutation::ContentText {
            content_id,
            text: required_value(&base, "/content/text")?
                .as_str()
                .ok_or("original text missing")?
                .into(),
        },
    )
    .await?;
    let shared_id: AutomationId =
        serde_json::from_value(required_value(&shared, "/automationId")?.clone())?;
    if serde_json::to_value(raw_call(machine, shared_request.clone()).await?)?
        != json!({"kind":"imported","value":{"automationId":shared_id}})
    {
        return Err(
            "corrected stored content reference remained blocked after atomic refusal".into(),
        );
    }
    require_record(
        &fixture::automation(client, &shared_id, None).await?,
        &shared,
        service,
        required_value(&shared_request, "/value/payload")?
            .as_str()
            .ok_or("shared frozen bytes missing")?,
        required_value(&shared_request, "/value/importId")?,
    )?;
    println!(
        "HANDOVER_VALIDATION_RESULT {}",
        json!({"contentUtf8Bytes":CONTENT_BYTES,"frozenPayloadUtf8Bytes":frozen.len(),"automationCallJsonBytes":outer_bytes,"sessionIdUtf8Bytes":4096,"endpointIdBytes":64,"malformedInnerOrEnvelopeCases":malformed_count,"namedLimits":4,"offGridNearestMarks":true,"storedInvalidContentNoWrites":true,"correctedRequestsAccepted":true})
    );
    Ok(())
}

fn limit_answer(limit: LimitName, value: u64, maximum: u64) -> ConformanceResult<Value> {
    Ok(serde_json::to_value(AutomationAnswer::Refused(
        AutomationRefusal::Validation {
            error: AutomationValidationError::LimitExceeded(LimitExceeded {
                limit,
                value,
                minimum: 1,
                maximum,
                step: 1,
            }),
        },
    ))?)
}
