//! The same bounded organization list is readable from either admitted machine.
use super::call;
use crate::{ConformanceResult, native_board_session::NativeBoardSession};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationPageRequest, CatalogAutomation,
};
use std::collections::BTreeSet;

#[cfg(debug_assertions)]
#[derive(Clone, PartialEq, Debug)]
enum ListFamily {
    Instructions,
    Catalog,
    Revisions(collaboration_protocol::InstructionId),
    Firings(collaboration_protocol::AutomationId),
}

#[cfg(debug_assertions)]
impl ListFamily {
    fn request(&self, cursor: Option<String>, limit: u32) -> ConformanceResult<AutomationCall> {
        let limit = limit.try_into()?;
        Ok(match self {
            Self::Instructions => {
                AutomationCall::InstructionList(AutomationPageRequest { cursor, limit })
            }
            Self::Catalog => AutomationCall::List(AutomationPageRequest { cursor, limit }),
            Self::Revisions(instruction_id) => {
                AutomationCall::RevisionList(collaboration_protocol::RevisionListRequest {
                    instruction_id: instruction_id.clone(),
                    cursor,
                    limit,
                })
            }
            Self::Firings(automation_id) => {
                AutomationCall::FiringList(collaboration_protocol::FiringListRequest {
                    automation_id: automation_id.clone(),
                    cursor,
                    limit,
                })
            }
        })
    }
}

#[cfg(debug_assertions)]
#[derive(Debug, PartialEq)]
struct ObservedPage {
    rows: Vec<(String, serde_json::Value)>,
    next: Option<String>,
}

#[cfg(debug_assertions)]
pub(super) async fn cursor_scopes(
    client: &crate::ConformanceClient,
    first: &mut NativeBoardSession,
    second: &mut NativeBoardSession,
    target: &str,
) -> ConformanceResult<()> {
    use collaboration_protocol::{
        AutomationId, ExecutionDestination, InstructionCreateParams, InstructionUpdateParams,
        ScheduleCreateRequest, ScheduleDefinition, TimingRequest,
    };
    let mut instructions = Vec::new();
    let mut expected_revisions = Vec::new();
    for name in ["first cursor parent", "second cursor parent"] {
        let created = super::require_instruction(
            call(
                first,
                AutomationCall::InstructionCreate(InstructionCreateParams {
                    operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
                    text: name.to_owned().try_into()?,
                }),
            )
            .await?,
        )?;
        let updated = super::require_instruction(
            call(
                first,
                AutomationCall::InstructionUpdate(InstructionUpdateParams {
                    operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
                    instruction_id: created.instruction_id.clone(),
                    expected_revision_id: created.revision_id.clone(),
                    text: format!("edited {name}").try_into()?,
                }),
            )
            .await?,
        )?;
        expected_revisions.push(BTreeSet::from([
            (
                created.revision_id.as_str().to_owned(),
                String::from(created.text),
            ),
            (
                updated.revision_id.as_str().to_owned(),
                String::from(updated.text.clone()),
            ),
        ]));
        instructions.push(updated);
    }
    let instruction = ListFamily::Instructions;
    let catalog = ListFamily::Catalog;
    let instruction_cursor = read_list_page(first, &instruction, None, 1)
        .await?
        .next
        .ok_or("instruction continuation fixture missing")?;
    let catalog_cursor = read_list_page(first, &catalog, None, 1)
        .await?
        .next
        .ok_or("catalog continuation fixture missing")?;
    invalid_cursor(second, &catalog, instruction_cursor).await?;
    invalid_cursor(second, &instruction, catalog_cursor).await?;
    let mut schedules = Vec::new();
    for instruction in &instructions {
        let created = call(first, AutomationCall::ScheduleCreate(ScheduleCreateRequest {
            operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
            definition: ScheduleDefinition { instruction_id: instruction.instruction_id.clone(), timing: TimingRequest::Interval { seconds: 300.try_into()?, first_due: None }, enabled: true,
                destination: ExecutionDestination::OwnedThread { target: serde_json::from_value(serde_json::json!({"endpoint":{"serviceId":target,"endpointId":"codex-local"},"sessionId":"cursor-firing-target"}))?, cwd: "/fixture".into() },
                execution_timeout_seconds: None, model: None, effort: Some("low".into()) },
        })).await?;
        let AutomationAnswer::Schedule(created) = created else {
            return Err("cursor Firing parent creation refused".into());
        };
        schedules.push(AutomationId::try_from(
            created.schedule_id.as_str().to_owned(),
        )?);
    }
    for time in ["2026-10-31T10:05:00Z", "2026-10-31T10:10:00Z"] {
        client
            .automation_clock(crate::AutomationClockControl::Advance {
                now: collaboration_central_core::RegistryTime(time.parse()?),
            })
            .await?;
    }
    let revisions = instructions
        .iter()
        .map(|instruction| ListFamily::Revisions(instruction.instruction_id.clone()))
        .collect::<Vec<_>>();
    let firings = schedules
        .iter()
        .cloned()
        .map(ListFamily::Firings)
        .collect::<Vec<_>>();
    let mut issued = Vec::new();
    for family in [instruction.clone(), catalog.clone()]
        .into_iter()
        .chain(revisions.iter().cloned())
        .chain(firings.iter().cloned())
    {
        let baseline = complete_list_traversal(first, second, &family).await?;
        match &family {
            ListFamily::Instructions => {
                for expected in &instructions {
                    let row = baseline
                        .iter()
                        .find(|(id, _)| id == expected.instruction_id.as_str())
                        .ok_or("created instruction missing from global pages")?;
                    if row.1 != serde_json::to_value(expected)? {
                        return Err("global instruction pages changed an owned head".into());
                    }
                }
            }
            ListFamily::Catalog => {
                for expected in &schedules {
                    if !baseline.iter().any(|(id, _)| id == expected.as_str()) {
                        return Err("created Schedule missing from global pages".into());
                    }
                }
            }
            ListFamily::Revisions(parent) => {
                let index = instructions
                    .iter()
                    .position(|instruction| instruction.instruction_id == *parent)
                    .ok_or("expected revision parent missing")?;
                let actual = baseline
                    .iter()
                    .map(|(id, row)| {
                        Ok((
                            id.clone(),
                            row.get("text")
                                .and_then(serde_json::Value::as_str)
                                .ok_or("revision text missing")?
                                .to_owned(),
                        ))
                    })
                    .collect::<ConformanceResult<BTreeSet<_>>>()?;
                if Some(&actual) != expected_revisions.get(index) {
                    return Err(
                        "revision pages omitted or changed an actually created revision".into(),
                    );
                }
            }
            ListFamily::Firings(_) => {
                let dues = baseline
                    .iter()
                    .map(|(_, row)| {
                        row.get("dueAt")
                            .and_then(serde_json::Value::as_i64)
                            .ok_or_else(|| "Firing due missing".into())
                    })
                    .collect::<ConformanceResult<BTreeSet<_>>>()?;
                let expected = ["2026-10-31T10:05:00Z", "2026-10-31T10:10:00Z"]
                    .into_iter()
                    .map(|time| {
                        time.parse::<chrono::DateTime<chrono::Utc>>()
                            .map(|time| time.timestamp_millis())
                    })
                    .collect::<Result<BTreeSet<_>, _>>()?;
                if baseline.len() != 2 || dues != expected {
                    return Err("Firing pages changed the owned 10:05/10:10 occurrences".into());
                }
            }
        }
        let first_page = read_list_page(first, &family, None, 1).await?;
        let token = first_page
            .next
            .ok_or("issued cursor fixture has no continuation")?;
        if collaboration_protocol::UuidIdentity::try_from(token.clone()).is_ok() {
            return Err("list cursor exposed a bare key instead of an opaque family token".into());
        }
        let continued = read_list_page(first, &family, Some(token.clone()), 1).await?;
        if continued.rows.len() != 1
            || continued.rows == first_page.rows
            || continued.next.as_ref() == Some(&token)
        {
            return Err("valid cursor failed to advance".into());
        }
        if read_list_page(second, &family, Some(token.clone()), 1).await? != continued {
            return Err("same cursor replay changed across machines".into());
        }
        println!(
            "AUTOMATION_CURSOR_PAGE {}",
            serde_json::json!({
                "family": format!("{family:?}"), "orderedRows": baseline,
                "issuedCursor": token, "continuedRows": continued.rows, "next": continued.next,
            })
        );
        issued.push((family, token, continued));
    }
    for (source, token, continued) in &issued {
        for (destination, _, _) in &issued {
            if source != destination {
                invalid_cursor(second, destination, token.clone()).await?;
                if let (ListFamily::Firings(_), ListFamily::Firings(parent)) = (source, destination)
                {
                    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
                    let mut changed: serde_json::Value =
                        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(token)?)?;
                    *changed
                        .get_mut("automationId")
                        .ok_or("issued Firing parent missing")? = serde_json::to_value(parent)?;
                    invalid_cursor(
                        second,
                        destination,
                        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&changed)?),
                    )
                    .await?;
                }
            }
        }
        for malformed in malformed_tokens(token)? {
            invalid_cursor(second, source, malformed).await?;
        }
        if read_list_page(first, source, Some(token.clone()), 1).await? != *continued
            || read_list_page(second, source, Some(token.clone()), 1).await? != *continued
        {
            return Err("rejected cursor misuse consumed or changed its valid continuation".into());
        }
    }
    println!(
        "AUTOMATION_CURSOR_SCOPE_RESULT 4 families; 2 revision and 2 Firing parents; full page1/baseline, owned rows/dues, cross-family/parent/shape refusals and same-token replay passed"
    );
    Ok(())
}

#[cfg(debug_assertions)]
async fn complete_list_traversal(
    first: &mut NativeBoardSession,
    second: &mut NativeBoardSession,
    family: &ListFamily,
) -> ConformanceResult<Vec<(String, serde_json::Value)>> {
    let baseline = read_list_page(first, family, None, 100).await?;
    if baseline.next.is_some() || baseline.rows.len() >= 100 {
        return Err("bounded cursor baseline exceeded its fixture limit".into());
    }
    let mut cursor = None;
    let mut rows = Vec::new();
    let mut ids = BTreeSet::new();
    for _ in 0..=baseline.rows.len() {
        let observed = read_list_page(second, family, cursor.clone(), 1).await?;
        if observed.rows.len() > 1
            || (observed.rows.is_empty() && observed.next.is_some())
            || observed.rows.iter().any(|(id, _)| !ids.insert(id.clone()))
        {
            return Err(
                "bounded cursor traversal repeated a row or exceeded its request bound".into(),
            );
        }
        rows.extend(observed.rows);
        match observed.next {
            None => {
                if rows != baseline.rows {
                    return Err(format!(
                        "page1 traversal omitted, reordered or changed rows for {family:?}"
                    )
                    .into());
                }
                return Ok(rows);
            }
            Some(next) if Some(&next) != cursor.as_ref() => cursor = Some(next),
            Some(_) => return Err("cursor did not advance".into()),
        }
    }
    Err("cursor traversal failed to terminate at its baseline bound".into())
}

#[cfg(debug_assertions)]
fn malformed_tokens(issued: &str) -> ConformanceResult<Vec<String>> {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    let mut tokens = vec![
        String::new(),
        "not-a-cursor".into(),
        "e30".into(),
        durable_steps::Uuid::now_v7().to_string(),
        format!("!{issued}"),
        URL_SAFE_NO_PAD.encode(b"{"),
    ];
    let decoded: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(issued)?)?;
    for field in ["kind", "after"] {
        let mut missing = decoded.clone();
        missing
            .as_object_mut()
            .ok_or("cursor object missing")?
            .remove(field);
        tokens.push(URL_SAFE_NO_PAD.encode(serde_json::to_vec(&missing)?));
        let mut invalid = decoded.clone();
        *invalid
            .get_mut(field)
            .ok_or("issued cursor required field missing")? = serde_json::json!("invalid");
        tokens.push(URL_SAFE_NO_PAD.encode(serde_json::to_vec(&invalid)?));
    }
    let mut extra = decoded.clone();
    extra
        .as_object_mut()
        .ok_or("cursor object missing")?
        .insert("extra".into(), serde_json::json!(true));
    tokens.push(URL_SAFE_NO_PAD.encode(serde_json::to_vec(&extra)?));
    for field in ["instructionId", "automationId"] {
        if decoded.get(field).is_some() {
            let mut invalid = decoded.clone();
            *invalid
                .get_mut(field)
                .ok_or("issued cursor parent missing")? = serde_json::json!("invalid");
            tokens.push(URL_SAFE_NO_PAD.encode(serde_json::to_vec(&invalid)?));
            let mut missing = decoded.clone();
            missing
                .as_object_mut()
                .ok_or("cursor object missing")?
                .remove(field);
            tokens.push(URL_SAFE_NO_PAD.encode(serde_json::to_vec(&missing)?));
        }
    }
    if let Some(parent) = decoded.get("instructionId") {
        let legacy = serde_json::json!({
            "instructionId": parent,
            "afterContentId": decoded.get("after").ok_or("issued revision key missing")?,
        });
        tokens.push(URL_SAFE_NO_PAD.encode(serde_json::to_vec(&legacy)?));
    }
    for shape in [
        serde_json::json!([]),
        serde_json::Value::Null,
        serde_json::json!("cursor"),
    ] {
        tokens.push(URL_SAFE_NO_PAD.encode(serde_json::to_vec(&shape)?));
    }
    Ok(tokens)
}

#[cfg(debug_assertions)]
async fn invalid_cursor(
    machine: &mut NativeBoardSession,
    family: &ListFamily,
    token: String,
) -> ConformanceResult<()> {
    let answer = call(machine, family.request(Some(token), 1)?).await?;
    println!(
        "AUTOMATION_CURSOR_REFUSAL {}",
        serde_json::json!({
            "family": format!("{family:?}"), "answer": answer,
        })
    );
    if !matches!(answer, AutomationAnswer::Refused(collaboration_protocol::AutomationRefusal::InvalidRequest { ref field, reason: collaboration_protocol::AutomationRequestConstraint::InvalidCursor }) if field=="cursor")
    {
        return Err(format!(
            "cross-scope or malformed cursor did not return typed invalidCursor: {answer:?}"
        )
        .into());
    }
    Ok(())
}

#[cfg(debug_assertions)]
async fn read_list_page(
    machine: &mut NativeBoardSession,
    family: &ListFamily,
    cursor: Option<String>,
    limit: u32,
) -> ConformanceResult<ObservedPage> {
    let answer = call(machine, family.request(cursor, limit)?).await?;
    match (family, answer) {
        (ListFamily::Instructions, AutomationAnswer::Instructions(page)) => observe_page_records(page.records, page.next_cursor, |record| record.instruction_id.as_str().to_owned()),
        (ListFamily::Revisions(parent), AutomationAnswer::Revisions(page)) => {
            if page.records.iter().any(|record| record.instruction_id != *parent) { return Err("revision page returned the wrong parent".into()); }
            observe_page_records(page.records, page.next_cursor, |record| record.revision_id.as_str().to_owned())
        }
        (ListFamily::Firings(parent), AutomationAnswer::Firings(page)) => {
            if page.records.iter().any(|record| record.automation_id != *parent) { return Err("Firing page returned the wrong parent".into()); }
            observe_page_records(page.records, page.next_cursor, |record| record.firing_id.as_str().to_owned())
        }
        (ListFamily::Catalog, AutomationAnswer::List { automations, next }) => observe_page_records(automations, next, |record| match record {
            CatalogAutomation::Schedule(record) => record.schedule_id.as_str().to_owned(),
            CatalogAutomation::Wake(record) => record.definition.wakeup_id.as_str().to_owned(),
            CatalogAutomation::Faulted(record) => record.automation_id.as_str().to_owned(),
            CatalogAutomation::Suspended(record) => record.automation_id.as_str().to_owned(),
        }),
        (_, other) => Err(format!("list cursor scenario returned the wrong answer family or refused a valid page: {other:?}").into()),
    }
}

#[cfg(debug_assertions)]
fn observe_page_records<T: serde::Serialize>(
    records: Vec<T>,
    next: Option<String>,
    identity: impl Fn(&T) -> String,
) -> ConformanceResult<ObservedPage> {
    let rows = records
        .into_iter()
        .map(|record| Ok((identity(&record), serde_json::to_value(record)?)))
        .collect::<ConformanceResult<Vec<_>>>()?;
    Ok(ObservedPage { rows, next })
}

pub(super) async fn run(
    machine: &mut NativeBoardSession,
    target_service: &str,
) -> ConformanceResult<()> {
    let mut cursor = None;
    let mut cursors = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut replaced_wakes = 0;
    loop {
        let AutomationAnswer::List { automations, next } = call(
            machine,
            AutomationCall::List(AutomationPageRequest {
                cursor: cursor.clone(),
                limit: 1.try_into()?,
            }),
        )
        .await?
        else {
            return Err("organization list did not return its bounded page".into());
        };
        if automations.len() > 1 {
            return Err("organization list exceeded its record limit".into());
        }
        for automation in automations {
            let id = match automation {
                CatalogAutomation::Suspended(snapshot) => {
                    snapshot.automation_id.as_str().to_owned()
                }
                CatalogAutomation::Wake(snapshot) => {
                    if String::from(
                        snapshot
                            .definition
                            .message
                            .target
                            .endpoint
                            .service_id
                            .clone(),
                    ) == target_service
                        && String::from(snapshot.definition.message.target.session_id.clone())
                            == "wake-target"
                    {
                        if snapshot.state != collaboration_protocol::WakeState::Cancelled
                            || snapshot.first_firing.is_some()
                        {
                            return Err("global Wake list lost the cancelled central state".into());
                        }
                        replaced_wakes += 1;
                    }
                    snapshot.definition.wakeup_id.as_str().to_owned()
                }
                CatalogAutomation::Schedule(snapshot) => {
                    if matches!(&snapshot.definition.destination,collaboration_protocol::ExecutionDestination::OwnedThread {target,..} if String::from(target.endpoint.service_id.clone())==target_service)
                    {
                        return Err("a deleted schedule remained in the organization list".into());
                    }
                    snapshot.schedule_id.as_str().to_owned()
                }
                CatalogAutomation::Faulted(fault) => {
                    return Err(format!("healthy catalog fixture became faulted: {fault:?}").into());
                }
            };
            if !ids.insert(id) {
                return Err("organization list repeated a definition".into());
            }
        }
        match next {
            None => break,
            Some(next) => {
                if !cursors.insert(next.clone()) {
                    return Err("organization cursor repeated".into());
                }
                cursor = Some(next);
            }
        }
    }
    // Original, creator replacement, target-session replacement, Owner replacement.
    if replaced_wakes != 4 {
        return Err(format!(
            "organization list retained {replaced_wakes} replacement Wakes, expected 4"
        )
        .into());
    }
    Ok(())
}
