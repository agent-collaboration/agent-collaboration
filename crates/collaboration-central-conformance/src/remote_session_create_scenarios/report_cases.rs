//! Real authenticated machines report facts without standing in for native execution.
use super::{call, open_machine, pull_work, session_create_request};
use crate::{
    ConformanceClient, ConformanceResult,
    native_board_session::NativeBoardSession,
    scenario_assertions::{check, equal},
};
use collaboration_protocol::{
    AcceptedWorkReport, AckAutomationBatches, AutomationAnswer, AutomationBatch, AutomationCall,
    AutomationRefusal, BatchAcknowledgement, FirstInputState, FrozenAutomationBatch,
    MachineWorkCustodyState, MessageContent, OperationId, ProviderIdentity,
    ProviderRequestedSettings, RemoteCreateRefusal, RemoteCreatedSettings,
    RemoteSessionCreateRequest, RemoteSessionCreateSnapshot, ReportWorkItems, SessionCreateState,
    WorkEntry, WorkItemId, WorkItemReport, WorkReportSubject,
};
use serde_json::json;

pub(super) async fn run(
    client: &ConformanceClient,
    source: &mut NativeBoardSession,
    target: &mut NativeBoardSession,
    original: &RemoteSessionCreateRequest,
    admitted: &RemoteSessionCreateSnapshot,
    frozen: &FrozenAutomationBatch,
) -> ConformanceResult<()> {
    let reader_service = durable_steps::Uuid::now_v7().to_string();
    let mut reader = open_machine(client, &reader_service).await?;
    let proof = report_cases(source, target, &mut reader, original, admitted, frozen).await;
    let closed = reader.close().await;
    proof?;
    closed?;
    Ok(())
}

async fn report_cases(
    source: &mut NativeBoardSession,
    target: &mut NativeBoardSession,
    reader: &mut NativeBoardSession,
    original: &RemoteSessionCreateRequest,
    admitted: &RemoteSessionCreateSnapshot,
    frozen: &FrozenAutomationBatch,
) -> ConformanceResult<()> {
    let source_service = match &original.definition.created_by {
        ProviderIdentity::Session(session) => String::from(session.endpoint.service_id.clone()),
        ProviderIdentity::Human { .. } => {
            return Err("initial fixture needs its source session".into());
        }
    };
    let target_service = String::from(original.definition.endpoint.service_id.clone());
    let third_reader = status(reader, &original.operation_id).await?;
    equal(third_reader.work_item_id, admitted.work_item_id.clone())?;
    check(
        third_reader.create.is_none() && third_reader.report_seq.is_none(),
        "inspection fabricated an unreported outcome",
    )?;

    let foreign = call(
        source,
        report_call(
            &admitted.work_item_id,
            99,
            SessionCreateState::Unknown,
            FirstInputState::None,
        )?,
    )
    .await?;
    check(
        matches!(
            foreign,
            AutomationAnswer::Refused(AutomationRefusal::OriginMismatch)
        ),
        "creator installation was allowed to report target work",
    )?;
    check(
        status(reader, &original.operation_id)
            .await?
            .report_seq
            .is_none(),
        "foreign report wrote an outcome",
    )?;

    accept_report(
        target,
        &admitted.work_item_id,
        1,
        SessionCreateState::Unknown,
        FirstInputState::None,
    )
    .await?;
    let uncertain = status(reader, &original.operation_id).await?;
    check(
        matches!(uncertain.create, Some(SessionCreateState::Unknown)),
        "reported unknown was fabricated or discarded",
    )?;
    check(
        uncertain.state != MachineWorkCustodyState::Received,
        "a report implicitly acknowledged the Batch",
    )?;
    equal(pull_work(target).await?.batches.len(), 1)?;
    acknowledge(target, frozen).await?;
    acknowledge(target, frozen).await?;
    equal(
        status(reader, &original.operation_id).await?.state,
        MachineWorkCustodyState::Received,
    )?;

    let mut unsupported = session_create_request(&source_service, &target_service)?;
    unsupported.definition.approver = Some(original.definition.created_by.clone());
    unsupported.definition.settings = Some(ProviderRequestedSettings {
        mode: Some("target-ineligible-mode".into()),
        model: Some("target-ineligible-model".into()),
        effort: Some("target-ineligible-effort".into()),
    });
    unsupported.definition.first_input = Some(MessageContent::HumanUser {
        text: "frozen optional input; no native effect in this fixture"
            .to_owned()
            .try_into()?,
    });
    let (refused_item, refused_batch) =
        admit_exact(source, target, &unsupported, &source_service).await?;
    monotone_report_case(
        target,
        reader,
        &unsupported.operation_id,
        &refused_item.work_item_id,
        SessionCreateState::Refused {
            reason: RemoteCreateRefusal::ApproverNotSupported,
        },
        FirstInputState::Failed {
            reason: "target fixture reported input refusal"
                .to_owned()
                .try_into()?,
        },
    )
    .await?;
    acknowledge(target, &refused_batch).await?;

    let mut human = session_create_request(&source_service, &target_service)?;
    human.definition.created_by = serde_json::from_value(
        json!({"humanId":"opaque declared human; no installation encoded"}),
    )?;
    human.definition.first_input = Some(MessageContent::HumanUser {
        text: "input is only frozen and target-reported here"
            .to_owned()
            .try_into()?,
    });
    let (created_item, created_batch) =
        admit_exact(source, target, &human, &source_service).await?;
    let created = SessionCreateState::Created {
        session_ref: serde_json::from_value(json!({
            "endpoint":{"serviceId":target_service,"endpointId":"codex-local"},
            "sessionId":"reported-stand-in-target"
        }))?,
        settings: RemoteCreatedSettings::Applied {
            effective_settings: None,
            permission_differences: Vec::new(),
        },
    };
    monotone_report_case(
        target,
        reader,
        &human.operation_id,
        &created_item.work_item_id,
        created,
        FirstInputState::Accepted,
    )
    .await?;
    let before_foreign_creator = serde_json::to_value(pull_work(target).await?.batches)?;
    let mut forged = session_create_request(&source_service, &target_service)?;
    forged.definition.created_by = serde_json::from_value(json!({
        "endpoint":{"serviceId":target_service,"endpointId":"codex-local"},"sessionId":"foreign-creator"
    }))?;
    let refused = call(source, AutomationCall::RemoteSessionCreate(forged)).await?;
    check(
        matches!(
            refused,
            AutomationAnswer::Refused(AutomationRefusal::OriginMismatch)
        ),
        "foreign session creator was admitted",
    )?;
    equal(
        serde_json::to_value(pull_work(target).await?.batches)?,
        before_foreign_creator,
    )?;

    let foreign_target = SessionCreateState::Created {
        session_ref: serde_json::from_value(json!({
            "endpoint":{"serviceId":source_service,"endpointId":"codex-local"},"sessionId":"foreign-result"
        }))?,
        settings: RemoteCreatedSettings::Applied {
            effective_settings: None,
            permission_differences: Vec::new(),
        },
    };
    let refused = call(
        target,
        report_call(
            &created_item.work_item_id,
            9,
            foreign_target,
            FirstInputState::Accepted,
        )?,
    )
    .await?;
    check(
        matches!(
            refused,
            AutomationAnswer::Refused(AutomationRefusal::OriginMismatch)
        ),
        "report attached a created session from another machine",
    )?;
    equal(
        status(reader, &human.operation_id)
            .await?
            .report_seq
            .map(u64::from),
        Some(3),
    )?;
    let unknown_id: WorkItemId = durable_steps::Uuid::now_v7().to_string().try_into()?;
    let unknown = call(
        target,
        report_call(
            &unknown_id,
            1,
            SessionCreateState::Unknown,
            FirstInputState::Unknown,
        )?,
    )
    .await?;
    check(
        matches!(
            unknown,
            AutomationAnswer::Refused(AutomationRefusal::NotFound)
        ),
        "unknown report subject was accepted",
    )?;
    equal(
        status(reader, &human.operation_id)
            .await?
            .report_seq
            .map(u64::from),
        Some(3),
    )?;
    acknowledge(target, &created_batch).await?;

    println!(
        "REMOTE_CREATE_REPORT_RESULT {}",
        json!({
            "organizationWideStatus":true,"targetOnlyReports":true,
            "noImplicitAck":true,"noTargetPolicyDefaults":true,
            "humanSourceServerFilled":true,"knownRefusedAndCreatedRetained":true,
            "newerUnknownAcknowledgedAtSeq":3,"nativeCreateAndInput":"unverified stand-in"
        })
    );
    Ok(())
}

async fn admit_exact(
    source: &mut NativeBoardSession,
    target: &mut NativeBoardSession,
    request: &RemoteSessionCreateRequest,
    expected_source: &str,
) -> ConformanceResult<(RemoteSessionCreateSnapshot, FrozenAutomationBatch)> {
    let admitted = match call(source, AutomationCall::RemoteSessionCreate(request.clone())).await? {
        AutomationAnswer::SessionCreate(item) => item,
        other => {
            return Err(format!("central applied target policy at admission: {other:?}").into());
        }
    };
    check(
        admitted.create.is_none() && admitted.input.is_none() && admitted.report_seq.is_none(),
        "requested first input was reported before the target",
    )?;
    let page = pull_work(target).await?;
    equal(page.batches.len(), 1)?;
    let frozen = page
        .batches
        .first()
        .ok_or("immediate Batch missing")?
        .clone();
    let batch: AutomationBatch = serde_json::from_slice(&frozen.frozen_bytes)?;
    equal(batch.entries.entries().len(), 1)?;
    let Some(WorkEntry::SessionCreate(entry)) = batch.entries.entries().first() else {
        return Err("immediate Batch has another entry kind".into());
    };
    equal(entry.work_item_id.clone(), admitted.work_item_id.clone())?;
    equal(entry.operation_id.clone(), request.operation_id.clone())?;
    equal(entry.source_installation.as_str(), expected_source)?;
    equal(
        serde_json::to_value(&entry.request)?,
        serde_json::to_value(&request.definition)?,
    )?;
    Ok((admitted, frozen))
}

async fn monotone_report_case(
    target: &mut NativeBoardSession,
    reader: &mut NativeBoardSession,
    operation_id: &OperationId,
    item_id: &WorkItemId,
    create: SessionCreateState,
    input: FirstInputState,
) -> ConformanceResult<()> {
    accept_report(
        target,
        item_id,
        1,
        SessionCreateState::Unknown,
        FirstInputState::Unknown,
    )
    .await?;
    let first = status(reader, operation_id).await?;
    check(
        matches!(first.create, Some(SessionCreateState::Unknown))
            && matches!(first.input, Some(FirstInputState::Unknown)),
        "an actual uncertain report was not retained",
    )?;
    accept_report(target, item_id, 2, create.clone(), input.clone()).await?;
    for seq in [3, 2, 3] {
        accept_report(
            target,
            item_id,
            seq,
            SessionCreateState::Unknown,
            FirstInputState::Unknown,
        )
        .await?;
        let row = status(reader, operation_id).await?;
        equal(row.report_seq.map(u64::from), Some(3))?;
        equal(
            serde_json::to_value(row.create)?,
            serde_json::to_value(&create)?,
        )?;
        equal(
            serde_json::to_value(row.input)?,
            serde_json::to_value(&input)?,
        )?;
    }
    Ok(())
}

pub(super) async fn accept_report(
    target: &mut NativeBoardSession,
    item_id: &WorkItemId,
    seq: u64,
    create: SessionCreateState,
    input: FirstInputState,
) -> ConformanceResult<()> {
    let answer = call(target, report_call(item_id, seq, create, input)?).await?;
    let AutomationAnswer::Reports(acknowledged) = answer else {
        return Err(format!("target report refused: {answer:?}").into());
    };
    equal(
        acknowledged.accepted,
        vec![AcceptedWorkReport {
            subject: WorkReportSubject::SessionCreate {
                work_item_id: item_id.clone(),
            },
            seq: seq.try_into()?,
        }],
    )?;
    Ok(())
}

pub(super) fn report_call(
    item_id: &WorkItemId,
    seq: u64,
    create: SessionCreateState,
    input: FirstInputState,
) -> ConformanceResult<AutomationCall> {
    Ok(AutomationCall::ReportWorkItems(ReportWorkItems::new(
        vec![WorkItemReport::SessionCreate {
            work_item_id: item_id.clone(),
            seq: seq.try_into()?,
            create: Box::new(create),
            input,
        }],
    )?))
}

pub(super) async fn status(
    reader: &mut NativeBoardSession,
    operation_id: &OperationId,
) -> ConformanceResult<RemoteSessionCreateSnapshot> {
    match call(
        reader,
        AutomationCall::RemoteSessionCreateStatus {
            operation_id: operation_id.clone(),
        },
    )
    .await?
    {
        AutomationAnswer::SessionCreate(item) => Ok(item),
        other => Err(format!("status read refused: {other:?}").into()),
    }
}

pub(super) async fn acknowledge(
    target: &mut NativeBoardSession,
    batch: &FrozenAutomationBatch,
) -> ConformanceResult<()> {
    let answer = call(
        target,
        AutomationCall::AckAutomationBatches(AckAutomationBatches {
            batches: vec![BatchAcknowledgement {
                batch_id: batch.batch_id.clone(),
                digest: batch.digest.clone(),
            }],
        }),
    )
    .await?;
    check(
        matches!(answer, AutomationAnswer::Acknowledged),
        "target acknowledgement refused",
    )
}
