//! One catalog case drives the real machine-session contract on either runtime.
use crate::{
    ConformanceClient, ConformanceResult, enroll_admitted_peer,
    native_board_session::NativeBoardSession,
};
use board_sync_protocol::{BoardConsumerAnswer, BoardConsumerRequest, BoardPayload};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationPageRequest, AutomationRefusal,
    InstructionCreateParams, InstructionShowParams, InstructionSnapshot, InstructionUpdateParams,
    RevisionListRequest,
};
mod collection_pages;
mod lost_reply;
mod package_cases;
mod preparation_cases;
mod raw_limits;
mod schedule_cases;
mod timing_cases;
mod wake_cases;

pub async fn run_automation_catalog_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    let first_service = durable_steps::Uuid::now_v7().to_string();
    let second_service = durable_steps::Uuid::now_v7().to_string();
    let mut first = open_machine(client, &first_service).await?;
    let mut second = match open_machine(client, &second_service).await {
        Ok(second) => second,
        Err(error) => {
            first.close().await?;
            return Err(error);
        }
    };
    let proof = async {
        let request = InstructionCreateParams {
            operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
            text: "One organization instruction".to_owned().try_into()?,
        };
        let instruction = require_instruction(
            call(
                &mut first,
                AutomationCall::InstructionCreate(request.clone()),
            )
            .await?,
        )?;
        let shown = call(
            &mut second,
            AutomationCall::InstructionShow(InstructionShowParams {
                instruction_id: instruction.instruction_id.clone(),
            }),
        )
        .await?;
        same_answer(shown, AutomationAnswer::Instruction(instruction.clone()))?;
        let current = edit_and_replay(&mut first, &mut second, &request, &instruction).await?;
        instruction_pages(&mut first, &mut second, &instruction, &current).await?;
        raw_limits::run(&mut first, &second_service).await?;
        let prepared = schedule_cases::run(&mut first, &mut second, &second_service).await?;
        wake_cases::run(&mut first, &mut second, &second_service).await?;
        collection_pages::run(&mut second, &second_service).await?;
        lost_reply::run(client, &mut first, &mut second, &first_service).await?;
        timing_cases::run(&mut first, &mut second, &second_service).await?;
        preparation_cases::run(client, &mut first, &mut second, &second_service, &prepared).await?;
        package_cases::run(client, &mut first, &mut second, &second_service).await?;
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let first_closed = first.close().await;
    let second_closed = second.close().await;
    proof?;
    first_closed?;
    second_closed?;
    Ok(())
}

async fn edit_and_replay(
    first: &mut NativeBoardSession,
    second: &mut NativeBoardSession,
    original_request: &InstructionCreateParams,
    original: &InstructionSnapshot,
) -> ConformanceResult<InstructionSnapshot> {
    let update = InstructionUpdateParams {
        operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
        instruction_id: original.instruction_id.clone(),
        expected_revision_id: original.revision_id.clone(),
        text: "Next explicit revision".to_owned().try_into()?,
    };
    let current = require_instruction(
        call(second, AutomationCall::InstructionUpdate(update.clone())).await?,
    )?;
    if current.revision_id == original.revision_id
        || current.text.as_str() != "Next explicit revision"
        || current.created_at != original.created_at
        || current.updated_at != original.updated_at
    {
        return Err("instruction head did not move independently of equal timestamps".into());
    }
    same_answer(
        call(
            first,
            AutomationCall::InstructionShow(InstructionShowParams {
                instruction_id: original.instruction_id.clone(),
            }),
        )
        .await?,
        AutomationAnswer::Instruction(current.clone()),
    )?;
    same_answer(
        call(second, AutomationCall::InstructionUpdate(update)).await?,
        AutomationAnswer::Instruction(current.clone()),
    )?;
    same_answer(
        call(
            first,
            AutomationCall::InstructionCreate(original_request.clone()),
        )
        .await?,
        AutomationAnswer::Instruction(original.clone()),
    )?;
    let mut conflict = original_request.clone();
    conflict.text = "Changed bytes under the original operation ID"
        .to_owned()
        .try_into()?;
    if !matches!(call(first, AutomationCall::InstructionCreate(conflict)).await?,
        AutomationAnswer::Refused(AutomationRefusal::OperationConflict {operation_id}) if operation_id == original_request.operation_id)
    {
        return Err("changed operation content did not conflict".into());
    }
    let stale = InstructionUpdateParams {
        operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
        instruction_id: original.instruction_id.clone(),
        expected_revision_id: original.revision_id.clone(),
        text: "Must not replace the newer head".to_owned().try_into()?,
    };
    if !matches!(call(second, AutomationCall::InstructionUpdate(stale)).await?,
        AutomationAnswer::Refused(AutomationRefusal::InstructionRevisionConflict {current_revision_id}) if current_revision_id == current.revision_id)
    {
        return Err("stale revision guard was not an explicit conflict".into());
    }
    Ok(current)
}

async fn instruction_pages(
    first: &mut NativeBoardSession,
    second: &mut NativeBoardSession,
    original: &InstructionSnapshot,
    current: &InstructionSnapshot,
) -> ConformanceResult<()> {
    require_instruction(
        call(
            second,
            AutomationCall::InstructionCreate(InstructionCreateParams {
                operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
                text: "A second document forces a continuation"
                    .to_owned()
                    .try_into()?,
            }),
        )
        .await?,
    )?;
    let mut cursor = None;
    let mut found = false;
    loop {
        let AutomationAnswer::Instructions(page) = call(
            first,
            AutomationCall::InstructionList(AutomationPageRequest {
                cursor: cursor.clone(),
                limit: 1.try_into()?,
            }),
        )
        .await?
        else {
            return Err("instruction list did not return its typed page".into());
        };
        if page.records.len() > 1 || (page.next_cursor.is_some() && page.next_cursor == cursor) {
            return Err("instruction page bound or continuation failed".into());
        }
        for record in page.records {
            if record.instruction_id == current.instruction_id {
                same_answer(
                    AutomationAnswer::Instruction(record),
                    AutomationAnswer::Instruction(current.clone()),
                )?;
                found = true;
            }
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    if !found {
        return Err("global instruction list omitted the current head".into());
    }
    let mut cursor = None;
    let mut history = std::collections::BTreeMap::new();
    loop {
        let AutomationAnswer::Revisions(page) = call(
            second,
            AutomationCall::RevisionList(RevisionListRequest {
                instruction_id: current.instruction_id.clone(),
                cursor: cursor.clone(),
                limit: 1.try_into()?,
            }),
        )
        .await?
        else {
            return Err("revision list did not return its typed page".into());
        };
        if page.records.len() > 1 || (page.next_cursor.is_some() && page.next_cursor == cursor) {
            return Err("revision page bound or continuation failed".into());
        }
        for record in page.records {
            if record.instruction_id != current.instruction_id
                || record.recorded_at != current.updated_at
            {
                return Err("revision history lost its owner or fixed clock".into());
            }
            if history.insert(record.revision_id, record.text).is_some() {
                return Err("revision repeated across pages".into());
            }
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    if history.len() != 2
        || history.get(&original.revision_id).map(String::as_str)
            != Some("One organization instruction")
        || history.get(&current.revision_id).map(String::as_str) != Some("Next explicit revision")
    {
        return Err("immutable revision history changed or lost content".into());
    }
    Ok(())
}

pub(crate) fn require_instruction(
    answer: AutomationAnswer,
) -> ConformanceResult<InstructionSnapshot> {
    match answer {
        AutomationAnswer::Instruction(snapshot) => Ok(snapshot),
        other => Err(format!("expected central instruction, got {other:?}").into()),
    }
}

pub(crate) fn same_answer(
    actual: AutomationAnswer,
    expected: AutomationAnswer,
) -> ConformanceResult<()> {
    if serde_json::to_value(actual)? != serde_json::to_value(expected)? {
        return Err("catalog answer did not preserve the exact snapshot".into());
    }
    Ok(())
}

pub(crate) async fn open_machine(
    client: &ConformanceClient,
    service: &str,
) -> ConformanceResult<NativeBoardSession> {
    Ok(NativeBoardSession(
        enroll_admitted_peer(
            client,
            service,
            &durable_steps::Uuid::now_v7().to_string(),
            &format!("automation-catalog-{service}"),
            &durable_steps::Uuid::now_v7().to_string(),
        )
        .await?,
    ))
}

pub(crate) async fn call(
    machine: &mut NativeBoardSession,
    request: AutomationCall,
) -> ConformanceResult<AutomationAnswer> {
    raw_call(machine, serde_json::to_value(request)?).await
}

pub(crate) async fn raw_call(
    machine: &mut NativeBoardSession,
    request: serde_json::Value,
) -> ConformanceResult<AutomationAnswer> {
    match machine
        .request(BoardConsumerRequest::Automation(BoardPayload::encode(
            &request,
        )?))
        .await?
    {
        BoardConsumerAnswer::Automation(answer) => Ok(answer.decode()?),
        other => Err(format!("automation call changed answer family: {other:?}").into()),
    }
}
