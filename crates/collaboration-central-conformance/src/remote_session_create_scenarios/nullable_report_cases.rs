//! Literal target reports exercise the real nullable wire and stored-result decoder.
use super::{
    open_machine,
    report_cases::{acknowledge, admit_exact, status},
    session_create_request,
};
use crate::{
    ConformanceClient, ConformanceResult,
    automation_catalog_scenarios::raw_call,
    native_board_session::NativeBoardSession,
    scenario_assertions::{check, equal},
};
use collaboration_protocol::{
    AutomationAnswer, AutomationRefusal, FirstInputState, MachineWorkCustodyState, MessageContent,
    OperationId, RemoteCreateRefusal, RemoteCreatedSettings, SessionCreateState, WorkItemId,
    WorkReportSubject,
};
use serde_json::{Value, json};

pub(super) async fn run(
    client: &ConformanceClient,
    source: &mut NativeBoardSession,
    target: &mut NativeBoardSession,
    source_service: &str,
    target_service: &str,
) -> ConformanceResult<()> {
    let reader_service = durable_steps::Uuid::now_v7().to_string();
    let mut reader = open_machine(client, &reader_service).await?;
    let proof = run_cases(source, target, &mut reader, source_service, target_service).await;
    let closed = reader.close().await;
    proof?;
    closed?;
    println!(
        "REMOTE_CREATE_INCREMENTAL_REPORT_RESULT {}",
        json!({"createKnownInputNull":true,"laterInputFill":true,"reportedUnknownSurvivesNull":true,"explicitNoneSurvivesNull":true,"unrequestedInputRemainsNone":true,"conflictingFinalFactsIgnored":true,"sameFactsHigherSequence":true,"incomingAckAndStoredMaxSeparate":true,"nullableOriginChecks":true,"noImplicitBatchAck":true,"nativeEffects":"unverified target-reported facts"})
    );
    Ok(())
}

async fn run_cases(
    source: &mut NativeBoardSession,
    target: &mut NativeBoardSession,
    reader: &mut NativeBoardSession,
    source_service: &str,
    target_service: &str,
) -> ConformanceResult<()> {
    let original_created = created_session_result(
        target_service,
        "reported-incremental-target",
        RemoteCreatedSettings::Applied {
            effective_settings: None,
            permission_differences: Vec::new(),
        },
    )?;
    let different_created = created_session_result(
        target_service,
        "different-reported-target",
        RemoteCreatedSettings::PartlyApplied {
            applied: Vec::new(),
            failed: Vec::new(),
            not_applied: Vec::new(),
        },
    )?;
    let refused = SessionCreateState::Refused {
        reason: RemoteCreateRefusal::ApproverNotSupported,
    };
    for (create, final_input) in [
        (original_created.clone(), FirstInputState::Accepted),
        (
            refused,
            FirstInputState::Failed {
                reason: "target reported first-input failure"
                    .to_owned()
                    .try_into()?,
            },
        ),
    ] {
        let mut request = session_create_request(source_service, target_service)?;
        request.definition.first_input = Some(MessageContent::HumanUser {
            text: "requested input has no outcome yet".to_owned().try_into()?,
        });
        let (item, batch) = admit_exact(source, target, &request, source_service).await?;
        let mut expected = ExpectedReport {
            item: item.work_item_id.clone(),
            operation: request.operation_id.clone(),
            state: status(reader, &request.operation_id).await?.state,
            create: serde_json::to_value(&create)?,
            input: Value::Null,
            seq: 1,
        };
        report(
            target,
            &expected.item,
            1,
            expected.create.clone(),
            Value::Null,
        )
        .await?;
        expected.assert_status(reader).await?;
        null_authorization(source, target, reader, source_service, &expected).await?;
        for (seq, input) in [(2, json!({"state":"unknown"})), (3, Value::Null)] {
            report(
                target,
                &expected.item,
                seq,
                json!({"state":"unknown"}),
                input,
            )
            .await?;
            expected.seq = seq;
            expected.input = json!({"state":"unknown"});
            expected.assert_status(reader).await?;
        }
        expected.input = serde_json::to_value(&final_input)?;
        report(
            target,
            &expected.item,
            4,
            json!({"state":"unknown"}),
            expected.input.clone(),
        )
        .await?;
        expected.seq = 4;
        expected.assert_status(reader).await?;
        let conflicting_input = match final_input {
            FirstInputState::Accepted => {
                json!({"state":"failed","reason":"conflicting later input failure"})
            }
            FirstInputState::Failed { .. } => json!({"state":"accepted"}),
            FirstInputState::None | FirstInputState::Unknown => {
                return Err("final input fixture must be accepted or failed".into());
            }
        };
        let conflicting_create = serde_json::to_value(&different_created)?;
        let different_refusal = json!({"state":"refused","reason":{"kind":"approverRequired"}});
        for (seq, create, input) in [
            (5, json!({"state":"unknown"}), Value::Null),
            (6, json!({"state":"unknown"}), conflicting_input.clone()),
            (7, conflicting_create, json!({"state":"unknown"})),
            (8, different_refusal, Value::Null),
            (9, expected.create.clone(), expected.input.clone()),
        ] {
            report(target, &expected.item, seq, create, input).await?;
            expected.seq = seq;
            expected.assert_status(reader).await?;
        }
        for seq in [4, 8, 1, 9] {
            report(
                target,
                &expected.item,
                seq,
                json!({"state":"unknown"}),
                conflicting_input.clone(),
            )
            .await?;
            // Each acknowledgement names the incoming sequence; status keeps the maximum 9.
            expected.assert_status(reader).await?;
        }
        let unacknowledged = super::pull_work(target).await?;
        equal(
            serde_json::to_value(&unacknowledged.batches)?,
            json!([batch.clone()]),
        )?;
        acknowledge(target, &batch).await?;
    }
    let request = session_create_request(source_service, target_service)?;
    let (item, batch) = admit_exact(source, target, &request, source_service).await?;
    let mut expected = ExpectedReport {
        item: item.work_item_id,
        operation: request.operation_id.clone(),
        state: status(reader, &request.operation_id).await?.state,
        create: serde_json::to_value(original_created)?,
        input: json!({"state":"none"}),
        seq: 1,
    };
    for (seq, input) in [
        (1, Value::Null),
        (2, json!({"state":"none"})),
        (3, Value::Null),
        (4, json!({"state":"unknown"})),
        (5, json!({"state":"accepted"})),
    ] {
        report(target, &expected.item, seq, expected.create.clone(), input).await?;
        expected.seq = seq;
        expected.assert_status(reader).await?;
    }
    acknowledge(target, &batch).await?;
    Ok(())
}

struct ExpectedReport {
    item: WorkItemId,
    operation: OperationId,
    state: MachineWorkCustodyState,
    create: Value,
    input: Value,
    seq: u64,
}
impl ExpectedReport {
    async fn assert_status(&self, reader: &mut NativeBoardSession) -> ConformanceResult<()> {
        equal(
            serde_json::to_value(status(reader, &self.operation).await?)?,
            json!({
                "workItemId":self.item,"operationId":self.operation,"state":self.state,
                "create":self.create,"input":self.input,"reportSeq":self.seq,
            }),
        )
    }
}

fn created_session_result(
    service: &str,
    session: &str,
    settings: RemoteCreatedSettings,
) -> ConformanceResult<SessionCreateState> {
    Ok(SessionCreateState::Created {
        session_ref: serde_json::from_value(
            json!({"endpoint":{"serviceId":service,"endpointId":"codex-local"},"sessionId":session}),
        )?,
        settings,
    })
}

async fn null_authorization(
    source: &mut NativeBoardSession,
    target: &mut NativeBoardSession,
    reader: &mut NativeBoardSession,
    foreign_service: &str,
    expected: &ExpectedReport,
) -> ConformanceResult<()> {
    let foreign = created_session_result(
        foreign_service,
        "foreign-created-address",
        RemoteCreatedSettings::Applied {
            effective_settings: None,
            permission_differences: Vec::new(),
        },
    )?;
    for answer in [
        send_report(
            source,
            &expected.item,
            99,
            expected.create.clone(),
            Value::Null,
        )
        .await?,
        send_report(
            target,
            &expected.item,
            99,
            serde_json::to_value(foreign)?,
            Value::Null,
        )
        .await?,
    ] {
        check(
            matches!(
                answer,
                AutomationAnswer::Refused(AutomationRefusal::OriginMismatch)
            ),
            "null input bypassed report origin or created-service authorization",
        )?;
        expected.assert_status(reader).await?;
    }
    Ok(())
}

async fn send_report(
    target: &mut NativeBoardSession,
    id: &WorkItemId,
    seq: u64,
    create: Value,
    input: Value,
) -> ConformanceResult<AutomationAnswer> {
    raw_call(target,json!({"kind":"reportWorkItems","value":{"reports":[{"kind":"sessionCreate","value":{"workItemId":id,"seq":seq,"create":create,"input":input}}]}})).await
}
async fn report(
    target: &mut NativeBoardSession,
    id: &WorkItemId,
    seq: u64,
    create: Value,
    input: Value,
) -> ConformanceResult<()> {
    let answer = send_report(target, id, seq, create, input).await?;
    let AutomationAnswer::Reports(accepted) = answer else {
        return Err(format!("create-known/input-null report refused: {answer:?}").into());
    };
    equal(accepted.accepted.len(), 1)?;
    let accepted = accepted
        .accepted
        .first()
        .ok_or("report acknowledgement missing")?;
    check(
        matches!(&accepted.subject,WorkReportSubject::SessionCreate {work_item_id} if work_item_id==id),
        "acknowledgement changed report subject",
    )?;
    equal(u64::from(accepted.seq), seq)
}
