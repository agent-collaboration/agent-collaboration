//! A refused mixed envelope cannot commit its otherwise valid first report.
use super::{call, fixture, open_machine, pull_work, session_create_request};
use crate::{
    AutomationClockControl, ConformanceClient, ConformanceResult,
    native_board_session::NativeBoardSession, scenario_assertions::equal,
};
use collaboration_central_core::RegistryTime;
use collaboration_protocol::{
    AckAutomationBatches, AutomationAnswer, AutomationBatch, AutomationCall, AutomationId,
    AutomationRefusal, BatchAcknowledgement, ExecutionDestination, FiringId, FiringOutcome,
    FiringReport, FirstInputState, GridTime, InstructionCreateParams, ReportWorkItems,
    ScheduleCreateRequest, ScheduleDefinition, SessionCreateState, TimingRequest, WorkEntry,
    WorkItemId, WorkItemReport,
};
use serde_json::{Value, json};

pub(super) async fn run(
    client: &ConformanceClient,
    source: &mut NativeBoardSession,
    target: &mut NativeBoardSession,
    source_service: &str,
    target_service: &str,
) -> ConformanceResult<()> {
    let foreign_service = durable_steps::Uuid::now_v7().to_string();
    let mut foreign = open_machine(client, &foreign_service).await?;
    let proof = async {
        pull_work(&mut foreign).await?;
        let request = session_create_request(source_service, &foreign_service)?;
        let foreign_item =
            match call(source, AutomationCall::RemoteSessionCreate(request.clone())).await? {
                AutomationAnswer::SessionCreate(row) => row,
                other => return Err(format!("mixed work admission failed: {other:?}").into()),
            };
        let custody = pull_work(&mut foreign).await?;
        equal(custody.batches.len(), 1)?;
        let frozen = custody
            .batches
            .first()
            .ok_or("foreign work Batch missing")?;
        if !matches!(
            call(
                &mut foreign,
                AutomationCall::AckAutomationBatches(AckAutomationBatches {
                    batches: vec![BatchAcknowledgement {
                        batch_id: frozen.batch_id.clone(),
                        digest: frozen.digest.clone()
                    }],
                })
            )
            .await?,
            AutomationAnswer::Acknowledged
        ) {
            return Err("foreign fixture owner ACK failed".into());
        }
        let (automation, firing) = prepare_firing(client, source, target, target_service).await?;
        accepted(
            target,
            WorkItemReport::Firing(FiringReport {
                firing_id: firing.clone(),
                seq: 1.try_into()?,
                outcome: FiringOutcome::Uncertain {
                    reason: "known baseline before mixed envelope"
                        .to_owned()
                        .try_into()?,
                },
            }),
        )
        .await?;
        let raw_firing =
            firing_observation(&fixture::automation(client, &automation).await?, &firing)?;
        let raw_work = fixture::machine_work(client, &foreign_item.work_item_id, None).await?;
        let firing_status = serde_json::to_value(
            call(
                source,
                AutomationCall::FiringShow {
                    firing_id: firing.clone(),
                },
            )
            .await?,
        )?;
        let work_status = serde_json::to_value(
            call(
                source,
                AutomationCall::RemoteSessionCreateStatus {
                    operation_id: request.operation_id.clone(),
                },
            )
            .await?,
        )?;
        let valid = WorkItemReport::Firing(FiringReport {
            firing_id: firing.clone(),
            seq: 2.try_into()?,
            outcome: FiringOutcome::Failed {
                reason: "must not commit ahead of a refused subject"
                    .to_owned()
                    .try_into()?,
            },
        });
        let wrong_owner = session_report(&foreign_item.work_item_id)?;
        let unknown = session_report(&durable_steps::Uuid::now_v7().to_string().try_into()?)?;
        for (reports, missing) in [
            (vec![valid.clone(), wrong_owner.clone()], false),
            (vec![wrong_owner, valid.clone()], false),
            (vec![valid.clone(), unknown], true),
        ] {
            let answer = call(
                target,
                AutomationCall::ReportWorkItems(ReportWorkItems::new(reports)?),
            )
            .await?;
            let refused = if missing {
                matches!(
                    answer,
                    AutomationAnswer::Refused(AutomationRefusal::NotFound)
                )
            } else {
                matches!(
                    answer,
                    AutomationAnswer::Refused(AutomationRefusal::OriginMismatch)
                )
            };
            if !refused {
                return Err(format!(
                    "mixed envelope lost its ownership/unknown refusal: {answer:?}"
                )
                .into());
            }
            let after_firing =
                firing_observation(&fixture::automation(client, &automation).await?, &firing)?;
            let after_work =
                fixture::machine_work(client, &foreign_item.work_item_id, None).await?;
            println!(
                "REMOTE_CREATE_MIXED_RAW_OBSERVATION {}",
                json!({
                    "refusal":answer,"beforeFiring":raw_firing,"afterFiring":after_firing,
                    "beforeWork":raw_work,"afterWork":after_work
                })
            );
            equal(after_firing, raw_firing.clone())?;
            equal(after_work, raw_work.clone())?;
            equal(
                serde_json::to_value(
                    call(
                        source,
                        AutomationCall::FiringShow {
                            firing_id: firing.clone(),
                        },
                    )
                    .await?,
                )?,
                firing_status.clone(),
            )?;
            equal(
                serde_json::to_value(
                    call(
                        source,
                        AutomationCall::RemoteSessionCreateStatus {
                            operation_id: request.operation_id.clone(),
                        },
                    )
                    .await?,
                )?,
                work_status.clone(),
            )?;
        }
        // This same report must work alone, so refusal cannot be a broken valid subject.
        accepted(target, valid).await?;
        let AutomationAnswer::Firing(settled) =
            call(source, AutomationCall::FiringShow { firing_id: firing }).await?
        else {
            return Err("settled valid Firing missing".into());
        };
        equal(settled.report_seq.map(u64::from), Some(2))?;
        if !matches!(settled.outcome, FiringOutcome::Failed { .. }) {
            return Err("valid standalone report was not applied".into());
        }
        println!(
            "REMOTE_CREATE_MIXED_REPORT_RESULT {}",
            json!({
                "bothKindOrders":true,"unknownSubject":true,"rawFiringAndWorkRowsUnchanged":true,
                "custodyAndPublicStatusesUnchanged":true,"sameValidReportWorksAlone":true
            })
        );
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let closed = foreign.close().await;
    proof?;
    closed?;
    Ok(())
}

async fn prepare_firing(
    client: &ConformanceClient,
    source: &mut NativeBoardSession,
    target: &mut NativeBoardSession,
    target_service: &str,
) -> ConformanceResult<(AutomationId, FiringId)> {
    let instruction = crate::automation_catalog_scenarios::require_instruction(
        call(
            source,
            AutomationCall::InstructionCreate(InstructionCreateParams {
                operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
                text: "mixed-kind atomicity fixture".to_owned().try_into()?,
            }),
        )
        .await?,
    )?;
    let before = client
        .automation_clock(AutomationClockControl::Observe)
        .await?;
    let due = GridTime::next_after(before.now.0.timestamp_millis())?.unix_millis();
    let due = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(due)
        .ok_or("mixed fixture time out of range")?;
    let definition = ScheduleDefinition {
        instruction_id: instruction.instruction_id,
        timing: TimingRequest::At {
            at: due
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
                .try_into()?,
        },
        enabled: true,
        destination: ExecutionDestination::OwnedThread {
            target: serde_json::from_value(
                json!({"endpoint":{"serviceId":target_service,"endpointId":"codex-local"},"sessionId":"mixed-report-target"}),
            )?,
            cwd: "/fixture".into(),
        },
        execution_timeout_seconds: None,
        model: None,
        effort: Some("low".into()),
    };
    let AutomationAnswer::Schedule(schedule) = call(
        source,
        AutomationCall::ScheduleCreate(ScheduleCreateRequest {
            operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
            definition,
        }),
    )
    .await?
    else {
        return Err("mixed Firing Schedule admission failed".into());
    };
    client
        .automation_clock(AutomationClockControl::Advance {
            now: RegistryTime(due),
        })
        .await?;
    for frozen in pull_work(target).await?.batches {
        let batch: AutomationBatch = serde_json::from_slice(&frozen.frozen_bytes)?;
        for entry in batch.entries.entries() {
            if let WorkEntry::Firing(entry) = entry
                && entry.automation_id.as_str() == schedule.schedule_id.as_str()
            {
                return Ok((entry.automation_id.clone(), entry.firing_id.clone()));
            }
        }
    }
    Err("normal pull omitted the mixed atomicity Firing".into())
}

fn session_report(work_item: &WorkItemId) -> ConformanceResult<WorkItemReport> {
    Ok(WorkItemReport::SessionCreate {
        work_item_id: work_item.clone(),
        seq: 1.try_into()?,
        create: Box::new(SessionCreateState::Unknown),
        input: FirstInputState::None,
    })
}

async fn accepted(
    machine: &mut NativeBoardSession,
    report: WorkItemReport,
) -> ConformanceResult<()> {
    match call(
        machine,
        AutomationCall::ReportWorkItems(ReportWorkItems::new(vec![report])?),
    )
    .await?
    {
        AutomationAnswer::Reports(accepted) if accepted.accepted.len() == 1 => Ok(()),
        other => Err(format!("valid standalone Firing report refused: {other:?}").into()),
    }
}

fn firing_observation(raw: &Value, firing: &FiringId) -> ConformanceResult<Value> {
    let row = raw
        .get("firings")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("firingId").and_then(Value::as_str) == Some(firing.as_str()))
        })
        .ok_or("raw mixed Firing missing")?;
    let batch_id = row
        .get("batchId")
        .and_then(Value::as_str)
        .ok_or("raw mixed Firing has no Batch")?;
    let batch = raw
        .get("batches")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("batchId").and_then(Value::as_str) == Some(batch_id))
        })
        .ok_or("raw mixed Firing Batch missing")?;
    Ok(json!({"firing":row,"batch":batch}))
}
