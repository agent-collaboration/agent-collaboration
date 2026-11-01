//! An actual post-commit answer drop cannot mint a second instruction on replay.
use super::{call, require_instruction, same_answer};
use crate::{
    AnswerFaultObservation, ConformanceClient, ConformanceResult, RuntimeControl, composed_client,
    native_board_session::NativeBoardSession,
};
use board_sync_protocol::{BoardConsumerRequest, BoardPayload};
use collaboration_central_core::ObjectAddress;
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationPageRequest, AutomationRefusal,
    InstructionCreateParams, InstructionSnapshot,
};
use fleet_sync_protocol::{FrameId, OperationRequest};
use std::collections::BTreeSet;

pub(super) async fn run(
    client: &ConformanceClient,
    first: &mut NativeBoardSession,
    second: &mut NativeBoardSession,
    source_service: &str,
) -> ConformanceResult<()> {
    let operation_id = durable_steps::Uuid::now_v7().to_string().try_into()?;
    let text = format!(
        "The lost answer retains this exact creation: {}",
        durable_steps::Uuid::now_v7()
    );
    let request = InstructionCreateParams {
        operation_id,
        text: text.clone().try_into()?,
    };
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    let connection = ObjectAddress::Connection(source_service.to_owned().try_into()?);
    composed_client::control(
        client,
        connection.clone(),
        RuntimeControl::DropNextAnswer { frame },
    )
    .await?;
    composed_client::send(
        &mut first.0.socket,
        frame,
        OperationRequest::Consumer(BoardConsumerRequest::Automation(BoardPayload::encode(
            &AutomationCall::InstructionCreate(request.clone()),
        )?)),
    )
    .await?;
    let observed = composed_client::control(
        client,
        connection,
        RuntimeControl::AwaitDroppedAnswer { frame },
    )
    .await?;
    if !matches!(observed.answer,AnswerFaultObservation::Dropped {frame:actual} if actual==frame) {
        return Err("the actual post-commit frame was not dropped".into());
    }
    let original = unique(second, &text).await?;
    same_answer(
        call(first, AutomationCall::InstructionCreate(request.clone())).await?,
        AutomationAnswer::Instruction(original.clone()),
    )?;
    same_answer(
        call(second, AutomationCall::InstructionCreate(request.clone())).await?,
        AutomationAnswer::Instruction(original.clone()),
    )?;
    same_answer(
        AutomationAnswer::Instruction(unique(first, &text).await?),
        AutomationAnswer::Instruction(original),
    )?;
    let mut conflict = request.clone();
    conflict.text = "different bytes under the lost operation"
        .to_owned()
        .try_into()?;
    if !matches!(call(second,AutomationCall::InstructionCreate(conflict)).await?,AutomationAnswer::Refused(AutomationRefusal::OperationConflict {operation_id}) if operation_id==request.operation_id)
    {
        return Err("changed lost operation content did not conflict".into());
    }
    Ok(())
}

async fn unique(
    machine: &mut NativeBoardSession,
    text: &str,
) -> ConformanceResult<InstructionSnapshot> {
    let mut cursor = None;
    let mut cursors = BTreeSet::new();
    let mut found = None;
    loop {
        let AutomationAnswer::Instructions(page) = call(
            machine,
            AutomationCall::InstructionList(AutomationPageRequest {
                cursor: cursor.clone(),
                limit: 2.try_into()?,
            }),
        )
        .await?
        else {
            return Err("lost-reply proof could not read the real catalog".into());
        };
        for record in page.records {
            if record.text.as_str() == text {
                if found.is_some() {
                    return Err("lost reply minted a duplicate instruction".into());
                }
                found = Some(require_instruction(AutomationAnswer::Instruction(record))?);
            }
        }
        match page.next_cursor {
            None => break,
            Some(next) => {
                if !cursors.insert(next.clone()) {
                    return Err("lost-reply proof list cursor repeated".into());
                }
                cursor = Some(next);
            }
        }
    }
    found.ok_or_else(|| "the dropped answer's committed instruction is absent".into())
}
