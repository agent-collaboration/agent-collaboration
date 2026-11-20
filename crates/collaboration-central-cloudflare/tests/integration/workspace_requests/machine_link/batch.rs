//! One real hibernation wake settles each well-formed write independently.
use super::super::workerd_host::c2_controls::{
    AttachmentSnapshot, EnrollmentReadOutcome, EnrollmentReadQuery, RestoreOutcome, WitnessEvent,
    WitnessPhase, WitnessRecord,
};
use super::fixture::{Fixture, MACHINE_ID, session_caller};
use super::receipt::{AreaReceipt, BatchHibernationReceipt};
use super::{baseline, workspace_reply};
use central_link_protocol::{DataFrame, ReplyPayload, RequestPayload};
use central_storage::{SqlValue, Statement};
use collaboration_central_conformance::ConformanceResult as Result;
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::Message;
use std::{io::Write, path::PathBuf};
use workspace_model::central_mail::{
    CentralMailOperation, CentralMailRefusal, CentralMailRequest, CentralMailResult,
    TimingRequirement,
};
use workspace_model::{
    Caller, ItemRef, ItemView, WorkspaceAnswer, WorkspaceId, WorkspaceOperation, WorkspaceRequest,
    WorkspaceResult,
};

const AREA_A: &str = "Batch area A";
const AREA_B: &str = "Batch area B";

#[tokio::test]
async fn one_hibernation_wake_processes_independent_writes_and_replies() -> Result<()> {
    let destination =
        std::env::var_os("CODEX_ROUTER_C2_BATCH_HIBERNATION_RECEIPT").map(PathBuf::from);
    if destination.as_ref().is_some_and(|path| !path.is_absolute()) {
        return Err("batch hibernation receipt destination must be absolute".into());
    }
    let fixture = Fixture::new().await?;
    let result = batch_journey(&fixture).await;
    fixture.close().await?;
    let mut receipt = result?;
    receipt.disposed = true;
    if let Some(destination) = destination {
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?;
        output.write_all(&serde_json::to_vec_pretty(&receipt)?)?;
    }
    Ok(())
}

async fn batch_journey(fixture: &Fixture) -> Result<BatchHibernationReceipt> {
    let (mut link, baseline, workspace, before_events, attachment) = baseline(fixture).await?;
    let payloads = writes(&workspace)?;
    fixture.host.c2_witness(WitnessPhase::Evicting).await?;
    fixture.host.c2_evict(&fixture.organization).await?;
    fixture.host.c2_witness(WitnessPhase::Heartbeat).await?;
    let mut heartbeat_events = Vec::new();
    for _ in 0..3 {
        link.socket.send(Message::Text("ping".into())).await?;
        match link.socket.next().await {
            Some(Ok(Message::Text(text))) if text == "pong" => {}
            _ => return Err("batch journey did not receive literal platform pong".into()),
        }
        let events = fixture.host.c2_witness(WitnessPhase::Heartbeat).await?;
        if events != before_events {
            return Err("batch heartbeat woke a constructor or handler".into());
        }
        heartbeat_events.push(events);
    }
    fixture.host.c2_witness(WitnessPhase::Deciding).await?;
    let (batch_request, batch_replies) = link.batch(payloads).await?;
    // No target read or later frame may contaminate the deciding interval.
    let deciding_events = fixture.host.c2_witness(WitnessPhase::Deciding).await?;
    assert_member_witnesses(
        &before_events,
        &deciding_events,
        &attachment,
        &batch_request,
    )?;
    let [first, middle, last] = batch_replies.as_slice() else {
        return Err("one batch must return exactly three independent Replies".into());
    };
    let area_a = created_area(first, AREA_A)?;
    let DataFrame::Reply { result, .. } = middle else {
        return Err("middle batch result was not a Reply".into());
    };
    if result.as_ref()
        != &ReplyPayload::Mail(CentralMailResult::Refused(
            CentralMailRefusal::TimingViolation {
                field: "dueAt".into(),
                requirement: TimingRequirement::FutureTime,
            },
        ))
    {
        return Err(
            "well-formed middle wake must return its actual FutureTime semantic refusal".into(),
        );
    }
    let area_b = created_area(last, AREA_B)?;
    if area_a.area_id == area_b.area_id {
        return Err("independent successful writes reused an area ID".into());
    }
    let areas = vec![area_a, area_b];
    fixture.host.c2_witness(WitnessPhase::Settling).await?;
    let mut read_requests = Vec::new();
    let mut read_replies = Vec::new();
    for expected in &areas {
        let exchange = link
            .call(WorkspaceRequest {
                request_id: None,
                workspace_id: Some(workspace.clone()),
                operation: WorkspaceOperation::Show {
                    item: ItemRef::Area(expected.area_id.clone()),
                },
            })
            .await?;
        match &workspace_reply(&exchange.reply)?.result {
            WorkspaceResult::Done(WorkspaceAnswer::Item {
                item: ItemView::Area(area),
            }) if area.area_id == expected.area_id
                && area.workspace_id == workspace
                && area.name.as_str() == expected.name => {}
            _ => {
                return Err(
                    "batch Show did not return the independently created area and literal name"
                        .into(),
                );
            }
        }
        read_requests.push(exchange.request);
        read_replies.push(exchange.reply);
    }
    let store = fixture
        .runtime()
        .inspect_workspace(
            &workspace,
            Statement {
                sql: "SELECT area_id,name FROM workspace_areas ORDER BY name",
                args: vec![],
            },
        )
        .await?;
    let expected_rows = areas
        .iter()
        .map(|area| {
            vec![
                SqlValue::Text(area.area_id.to_string()),
                SqlValue::Text(area.name.into()),
            ]
        })
        .collect::<Vec<_>>();
    if store.columns != vec!["area_id", "name"] || store.rows != expected_rows {
        return Err(
            "batch owning store must contain exactly the two successful area IDs and names".into(),
        );
    }
    let wakes = fixture
        .runtime()
        .read(Statement {
            sql: "SELECT COUNT(*) AS wake_count FROM automation_wakes",
            args: vec![],
        })
        .await?;
    if wakes.columns != vec!["wake_count"]
        || wakes.rows != vec![vec![SqlValue::Integer(0.try_into()?)]]
    {
        return Err("semantically refused batch member wrote a wake row".into());
    }
    let welcome = link.welcome.clone();
    link.close().await?;
    Ok(BatchHibernationReceipt {
        kind: "c2BatchHibernation",
        organization: fixture.organization.clone(),
        machine_id: MACHINE_ID.parse()?,
        welcome,
        baseline_request: baseline.request,
        baseline_reply: baseline.reply,
        batch_request,
        batch_replies,
        read_requests,
        read_replies,
        before_events,
        heartbeat_events,
        deciding_events,
        areas,
        store,
        wakes,
        disposed: false,
    })
}

fn writes(workspace: &WorkspaceId) -> Result<Vec<RequestPayload>> {
    let Caller::Session(target) = session_caller()? else {
        return Err("batch wake target must be the ordinary caller's session".into());
    };
    let area = |request_id: &str, name: &str| -> Result<RequestPayload> {
        Ok(RequestPayload::Workspace(WorkspaceRequest {
            request_id: Some(request_id.parse()?),
            workspace_id: Some(workspace.clone()),
            operation: WorkspaceOperation::CreateArea {
                name: name.to_owned().try_into()?,
                description: "Test description".to_owned().try_into().unwrap(),
            },
        }))
    };
    Ok(vec![
        area("019f0000-0000-7000-8000-00000000cf40", AREA_A)?,
        RequestPayload::Mail(CentralMailRequest {
            request_id: "019f0000-0000-7000-8000-00000000cf41".parse()?,
            operation: CentralMailOperation::SetWake {
                target,
                due_at: "2030-03-17T17:45:00Z".parse()?,
                text: "Past wake must refuse".to_owned().try_into()?,
            },
        }),
        area("019f0000-0000-7000-8000-00000000cf42", AREA_B)?,
    ])
}

fn created_area(frame: &DataFrame, name: &'static str) -> Result<AreaReceipt> {
    let WorkspaceResult::Done(WorkspaceAnswer::Created {
        item: ItemRef::Area(area_id),
        ..
    }) = &workspace_reply(frame)?.result
    else {
        return Err("successful batch member must return an actual created area".into());
    };
    Ok(AreaReceipt {
        area_id: area_id.clone(),
        name,
    })
}

fn assert_member_witnesses(
    before: &[WitnessRecord],
    after: &[WitnessRecord],
    attachment: &AttachmentSnapshot,
    frame: &DataFrame,
) -> Result<()> {
    let DataFrame::Batch { requests } = frame else {
        return Err("deciding frame must be the actual Batch".into());
    };
    let first = requests
        .requests()
        .first()
        .ok_or("batch has no first member")?;
    // This establishes one fresh constructor/handler and the original closed attachment.
    let (instance, _) = super::witness_assertions::reconstruction(
        before,
        after,
        attachment,
        first.correlation_id(),
        RestoreOutcome::Admitted,
    )?;
    let fresh = after
        .get(before.len()..)
        .ok_or("batch witness baseline changed")?;
    let handler = fresh
        .iter()
        .find(|record| matches!(record.event, WitnessEvent::Handler { .. }))
        .ok_or("batch handler witness missing")?;
    let mut floor = handler.sequence;
    for member in requests.requests() {
        let restored = fresh.iter().filter(|record| matches!(&record.event,WitnessEvent::RestoreResult { correlation_id, .. } if correlation_id == member.correlation_id())).collect::<Vec<_>>();
        let [restored] = restored.as_slice() else {
            return Err("each batch member needs exactly one matching restoration result".into());
        };
        if restored.instance_id != instance
            || restored.sequence <= floor
            || !matches!(
                restored.event,
                WitnessEvent::RestoreResult {
                    outcome: RestoreOutcome::Admitted,
                    ..
                }
            )
        {
            return Err(
                "each batch member must restore live authority on the new serving instance".into(),
            );
        }
        let required = [
            EnrollmentReadQuery::ByEnrollment,
            EnrollmentReadQuery::MachineById,
            EnrollmentReadQuery::ActiveEnrollment,
        ];
        let mut next = 0;
        for record in fresh {
            if record.instance_id == instance
                && record.sequence > floor
                && record.sequence < restored.sequence
                && let WitnessEvent::EnrollmentRead {
                    query,
                    outcome: EnrollmentReadOutcome::Read,
                    ..
                } = &record.event
                && required.get(next) == Some(query)
            {
                next += 1;
            }
        }
        if next != required.len() {
            return Err("batch member reused earlier live reads instead of reading after the previous terminal alarm snapshot".into());
        }
        super::witness_assertions::alarm_snapshot(
            after,
            member.correlation_id(),
            instance,
            restored.sequence,
        )?;
        let alarm = fresh.iter().find(|record| matches!(&record.event,WitnessEvent::AlarmSnapshot { correlation_id, .. } if correlation_id == member.correlation_id())).ok_or("batch terminal alarm snapshot missing")?;
        floor = alarm.sequence;
    }
    if fresh
        .iter()
        .filter(|record| matches!(record.event, WitnessEvent::RestoreResult { .. }))
        .count()
        != 3
        || fresh
            .iter()
            .filter(|record| matches!(record.event, WitnessEvent::AlarmSnapshot { .. }))
            .count()
            != 3
    {
        return Err(
            "batch must have exactly three member restoration results and terminal alarm snapshots"
                .into(),
        );
    }
    Ok(())
}
