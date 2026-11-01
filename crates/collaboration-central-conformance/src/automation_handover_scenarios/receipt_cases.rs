//! Raw owner observations decide replay and whole-import refusal atomicity.
use super::fixtures::{import_request, required_value, required_value_mut, schedule_payload};
use crate::{
    ConformanceClient, ConformanceResult, automation_catalog_scenarios::raw_call,
    automation_clock_scenarios::fixture, native_board_session::NativeBoardSession,
};
use collaboration_protocol::{AutomationAnswer, AutomationId, AutomationRefusal};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub(super) async fn run(
    client: &ConformanceClient,
    first: &mut NativeBoardSession,
    second: &mut NativeBoardSession,
    importer: &str,
    target: &str,
) -> ConformanceResult<()> {
    let payload = schedule_payload(target);
    let id: AutomationId =
        serde_json::from_value((*required_value(&payload, "/automationId")?).clone())?;
    let frozen = serde_json::to_string_pretty(&payload)?;
    let request = import_request(&frozen);
    let before = fixture::automation(client, &id, None).await?;
    let imported = raw_call(first, request.clone()).await?;
    if serde_json::to_value(imported)? != json!({"kind":"imported","value":{"automationId":id}}) {
        return Err("ordinary handover did not commit the preserved Automation identity".into());
    }
    let committed = fixture::automation(client, &id, None).await?;
    require_record(
        &committed,
        &payload,
        importer,
        &frozen,
        required_value(&request, "/value/importId")?,
    )?;
    require_count_delta(
        &before,
        &committed,
        &[
            "automations",
            "timing",
            "imports",
            "contentRevisions",
            "instructionDocuments",
        ],
    )?;
    let replay = json!({"kind":"alreadyImported","value":{"automationId":id}});
    for request in [request.clone(), import_request(&frozen)] {
        unchanged(client, first, &id, request, &replay).await?;
    }
    let conflict = json!({"kind":"refused","value":{"kind":"importConflict","automationId":id}});
    let reversed = format!(
        "{{{}}}",
        payload
            .as_object()
            .ok_or("payload object missing")?
            .iter()
            .rev()
            .map(|(key, value)| format!("{}:{}", json!(key), value))
            .collect::<Vec<_>>()
            .join(",")
    );
    if serde_json::from_str::<Value>(&reversed)? != payload {
        return Err("key-order fixture changed meaning".into());
    }
    for bytes in [
        payload.to_string(),
        format!("{frozen}\n"),
        reversed,
        frozen.replace('雪', "\\u96ea").replace("\\n", "\\u000a"),
    ] {
        if serde_json::from_str::<Value>(&bytes)? != payload {
            return Err("equivalent-byte fixture changed meaning".into());
        }
        unchanged(client, first, &id, import_request(&bytes), &conflict).await?;
    }
    let mut changed = payload.clone();
    *required_value_mut(&mut changed, "/content/text")? = json!("changed frozen instruction");
    unchanged(
        client,
        first,
        &id,
        import_request(&changed.to_string()),
        &conflict,
    )
    .await?;
    unchanged(client, second, &id, import_request(&frozen), &conflict).await?;
    let mut other = payload.clone();
    *required_value_mut(&mut other, "/automationId")? =
        json!(durable_steps::Uuid::now_v7().to_string());
    let mut reused = import_request(&other.to_string());
    *required_value_mut(&mut reused, "/value/importId")? =
        (*required_value(&request, "/value/importId")?).clone();
    let other_conflict = json!({"kind":"refused","value":{"kind":"importConflict","automationId":(*required_value(&other, "/automationId")?)}});
    unchanged(client, first, &id, reused, &other_conflict).await?;
    late_instruction_conflict(client, first, &id, &payload).await?;
    Ok(())
}

pub(super) async fn unchanged(
    client: &ConformanceClient,
    machine: &mut NativeBoardSession,
    observed: &AutomationId,
    request: Value,
    expected: &Value,
) -> ConformanceResult<()> {
    let before = fixture::automation(client, observed, None).await?;
    let answer = serde_json::to_value(raw_call(machine, request).await?)?;
    if &answer != expected {
        return Err(format!("import response differs: {answer}").into());
    }
    let after = fixture::automation(client, observed, None).await?;
    if after != before {
        eprintln!(
            "HANDOVER_NO_WRITE_MISMATCH {}",
            json!({"before":before,"after":after,"answer":answer,"expected":expected})
        );
        return Err("import replay/refusal changed raw owner rows or counts".into());
    }
    Ok(())
}

pub(super) fn require_record(
    row: &Value,
    payload: &Value,
    importer: &str,
    frozen: &str,
    import_id: &Value,
) -> ConformanceResult<()> {
    let body: Value = serde_json::from_str(
        (*required_value(row, "/definitionJson")?)
            .as_str()
            .ok_or("definition missing")?,
    )?;
    let creator: Value = serde_json::from_str(
        (*required_value(row, "/creatorJson")?)
            .as_str()
            .ok_or("creator missing")?,
    )?;
    if (*required_value(row, "/automationId")?) != (*required_value(payload, "/automationId")?)
        || (*required_value(row, "/revision")?) != (*required_value(payload, "/revision")?)
        || (*required_value(row, "/importedWatermarkLocalMs")?)
            != (*required_value(payload, "/watermarkLocalMs")?)
        || (*required_value(row, "/admittedThroughMs")?)
            != (*required_value(payload, "/admittedThroughMs")?)
        || (*required_value(row, "/nextDueMs")?) != (*required_value(payload, "/firstDueMs")?)
        || (*required_value(row, "/state")?) != "enabled"
        || (*required_value(&body, "/value/definition")?)
            != (*required_value(payload, "/definition/value")?)
        || creator
            != json!({"installation":importer,"declared":(*required_value(payload, "/creator")?)})
    {
        eprintln!(
            "HANDOVER_IMPORTED_RECORD_MISMATCH {}",
            json!({"actual":row,"payload":payload,"importer":importer})
        );
        return Err(
            "import changed preserved identity, definition, creator or timing progress".into(),
        );
    }
    for (column, field) in [
        ("anchorAtMs", "anchorAt"),
        ("createdAtMs", "createdAt"),
        ("updatedAtMs", "updatedAt"),
    ] {
        let expected = chrono::DateTime::parse_from_rfc3339(
            (*required_value(payload, &format!("/header/{field}"))?)
                .as_str()
                .ok_or("header timestamp missing")?,
        )?
        .timestamp_millis();
        if (*required_value(row, &format!("/{column}"))?) != json!(expected) {
            return Err("import changed the independent record header".into());
        }
    }
    let bytes = [
        b"automationImport\0".as_slice(),
        importer.as_bytes(),
        b"\0",
        frozen.as_bytes(),
    ]
    .concat();
    let expected_digest = format!("{:x}", Sha256::digest(bytes));
    let receipts = (*required_value(row, "/imports")?)
        .as_array()
        .ok_or("raw import receipts missing")?;
    let receipt = receipts
        .iter()
        .find(|receipt| receipt.get("automationId") == payload.get("automationId"))
        .ok_or("committed import receipt missing")?;
    if (*required_value(receipt, "/importId")?) != *import_id
        || (*required_value(receipt, "/serviceId")?) != importer
        || (*required_value(receipt, "/digest")?) != expected_digest
    {
        return Err("receipt lost exact-byte/authenticated digest binding".into());
    }
    let expected_revision = required_value(payload, "/content/revisionId")?;
    let content = (*required_value(row, "/contentRevisions")?)
        .as_array()
        .ok_or("raw content missing")?
        .iter()
        .find(|content| content.get("revisionId") == Some(expected_revision))
        .ok_or("imported revision missing")?;
    if (*required_value(content, "/ownerId")?)
        != (*required_value(payload, "/definition/value/instructionId")?)
        || (*required_value(content, "/text")?) != (*required_value(payload, "/content/text")?)
        || (*required_value(content, "/digest")?)
            != format!(
                "{:x}",
                Sha256::digest(
                    (*required_value(payload, "/content/text")?)
                        .as_str()
                        .ok_or("fixture text missing")?
                        .as_bytes()
                )
            )
    {
        return Err("frozen content identity or bytes changed".into());
    }
    let owner_id = required_value(content, "/ownerId")?;
    let content_id = required_value(content, "/contentId")?;
    if !(*required_value(row, "/instructionDocuments")?)
        .as_array()
        .ok_or("instruction heads missing")?
        .iter()
        .any(|head| {
            head.get("instructionId") == Some(owner_id)
                && head.get("currentContentId") == Some(content_id)
        })
    {
        return Err("imported instruction head lost its frozen revision".into());
    }
    Ok(())
}

fn require_count_delta(before: &Value, after: &Value, changed: &[&str]) -> ConformanceResult<()> {
    for name in [
        "automations",
        "timing",
        "firings",
        "batches",
        "imports",
        "contentRevisions",
        "instructionDocuments",
    ] {
        let previous = (*required_value(before, &format!("/counts/{name}"))?)
            .as_u64()
            .ok_or("before count missing")?;
        let current = (*required_value(after, &format!("/counts/{name}"))?)
            .as_u64()
            .ok_or("after count missing")?;
        if current != previous + u64::from(changed.contains(&name)) {
            return Err(format!("import changed unexpected {name} count").into());
        }
    }
    Ok(())
}

async fn late_instruction_conflict(
    client: &ConformanceClient,
    machine: &mut NativeBoardSession,
    observed: &AutomationId,
    original: &Value,
) -> ConformanceResult<()> {
    let mut payload = original.clone();
    *required_value_mut(&mut payload, "/automationId")? =
        json!(durable_steps::Uuid::now_v7().to_string());
    *required_value_mut(&mut payload, "/content/revisionId")? =
        json!(durable_steps::Uuid::now_v7().to_string());
    *required_value_mut(&mut payload, "/content/text")? =
        json!("different local head must not replace the shared immutable instruction");
    let before = fixture::automation(client, observed, None).await?;
    let answer = raw_call(machine, import_request(&payload.to_string())).await?;
    if !matches!(
        answer,
        AutomationAnswer::Refused(AutomationRefusal::ImportConflict { .. })
    ) {
        return Err("late instruction-head conflict was not refused".into());
    }
    if fixture::automation(client, observed, None).await? != before {
        return Err("late head conflict leaked content, header, head or receipt writes".into());
    }
    *required_value_mut(&mut payload, "/content")? =
        (*required_value(original, "/content")?).clone();
    let corrected = raw_call(machine, import_request(&payload.to_string())).await?;
    if !matches!(corrected, AutomationAnswer::Imported(_)) {
        return Err("valid shared instruction retry remained blocked after refusal".into());
    }
    let after = fixture::automation(client, observed, None).await?;
    require_count_delta(&before, &after, &["automations", "timing", "imports"])?;
    if (*required_value(&before, "/contentRevisions")?)
        != (*required_value(&after, "/contentRevisions")?)
        || (*required_value(&before, "/instructionDocuments")?)
            != (*required_value(&after, "/instructionDocuments")?)
    {
        return Err("valid shared import rewrote its existing revision/head".into());
    }
    Ok(())
}
