//! Retained request and report ownership outlive Batch retention and owner restart.
use super::{call, fixture, open_machine, pull_work, report_cases, session_create_request};
use crate::{
    AutomationClockControl, ConformanceClient, ConformanceResult, enroll_admitted_peer,
    native_board_session::NativeBoardSession, scenario_assertions::equal,
};
use collaboration_central_core::RegistryTime;
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationRefusal, FirstInputState, FrozenAutomationBatch,
    MessageContent, PullAutomationBatches, RemoteCreatedSettings, RemoteSessionCreateRequest,
    RemoteSessionCreateSnapshot, SessionCreateState,
};
use serde_json::{Value, json};

/// Driver-owned facts cross the owner restart; executable custody remains in Fleet.
pub struct RemoteCreateRestartCase {
    source_service: String,
    source_peer: String,
    target_service: String,
    target_peer: String,
    request: RemoteSessionCreateRequest,
    known: RemoteSessionCreateSnapshot,
    raw_after_prune: Value,
    unacked: FrozenAutomationBatch,
    now: RegistryTime,
}

pub async fn prepare_remote_create_restart(
    client: &ConformanceClient,
) -> ConformanceResult<RemoteCreateRestartCase> {
    let source_service = durable_steps::Uuid::now_v7().to_string();
    let target_service = durable_steps::Uuid::now_v7().to_string();
    let mut source = open_machine(client, &source_service).await?;
    let mut target = match open_machine(client, &target_service).await {
        Ok(target) => target,
        Err(error) => {
            source.close().await?;
            return Err(error);
        }
    };
    let prepared = prepare(
        client,
        &mut source,
        &mut target,
        source_service,
        target_service,
    )
    .await;
    let source_closed = source.close().await;
    let target_closed = target.close().await;
    let mut prepared = prepared?;
    source_closed?;
    target_closed?;
    prepared.now = RegistryTime(
        prepared
            .now
            .0
            .checked_add_months(chrono::Months::new(2))
            .and_then(|time| time.checked_add_signed(chrono::Duration::days(4)))
            .ok_or("remote retention time out of range")?,
    );
    client
        .automation_clock(AutomationClockControl::Advance { now: prepared.now })
        .await?;
    let mut source = reconnect(client, &prepared.source_service, &prepared.source_peer).await?;
    let pruned = async {
        equal(
            serde_json::to_value(
                report_cases::status(&mut source, &prepared.request.operation_id).await?,
            )?,
            serde_json::to_value(&prepared.known)?,
        )?;
        let raw = fixture::machine_work(client, &prepared.known.work_item_id, None).await?;
        equal(fixture::field::<Vec<Value>>(&raw, "/batches")?.len(), 0)?;
        // The retained eight-column item is unchanged; only its old Batch is gone.
        let mut expected = prepared.raw_after_prune.clone();
        *expected
            .get_mut("batches")
            .ok_or("raw Batch array missing")? = json!([]);
        equal(raw.clone(), expected)?;
        println!(
            "REMOTE_CREATE_RETENTION_PREPARED {}",
            json!({
                "beforePrune":prepared.raw_after_prune,"afterPrune":raw,
                "knownStatus":prepared.known,"unackedBatch":prepared.unacked,"now":prepared.now,
                "nativeEffects":"target report stand-in only"
            })
        );
        Ok::<Value, Box<dyn std::error::Error + Send + Sync>>(raw)
    }
    .await;
    let closed = source.close().await;
    prepared.raw_after_prune = pruned?;
    closed?;
    Ok(prepared)
}

async fn prepare(
    client: &ConformanceClient,
    source: &mut NativeBoardSession,
    target: &mut NativeBoardSession,
    source_service: String,
    target_service: String,
) -> ConformanceResult<RemoteCreateRestartCase> {
    equal(pull_work(target).await?.batches.len(), 0)?;
    let mut request = session_create_request(&source_service, &target_service)?;
    request.definition.first_input = Some(MessageContent::HumanUser {
        text: "retained first-input fixture; no provider invocation"
            .to_owned()
            .try_into()?,
    });
    let admitted = match call(source, AutomationCall::RemoteSessionCreate(request.clone())).await? {
        AutomationAnswer::SessionCreate(row) => row,
        other => return Err(format!("retention admission refused: {other:?}").into()),
    };
    let page = pull_work(target).await?;
    equal(page.batches.len(), 1)?;
    let acked = page.batches.first().ok_or("retention Batch missing")?;
    report_cases::acknowledge(target, acked).await?;
    report_cases::accept_report(
        target,
        &admitted.work_item_id,
        1,
        SessionCreateState::Created {
            session_ref: serde_json::from_value(json!({
                "endpoint":{"serviceId":target_service,"endpointId":"codex-local"},
                "sessionId":"retained-target-reported-session"
            }))?,
            settings: RemoteCreatedSettings::Applied {
                effective_settings: None,
                permission_differences: Vec::new(),
            },
        },
        FirstInputState::Accepted,
    )
    .await?;
    let known = report_cases::status(source, &request.operation_id).await?;
    equal(
        serde_json::to_value(&known)?,
        json!({
            "workItemId":admitted.work_item_id,"operationId":request.operation_id,
            "state":"received","reportSeq":1,"input":{"state":"accepted"},
            "create":{"state":"created","sessionRef":{
                "endpoint":{"serviceId":target_service,"endpointId":"codex-local"},
                "sessionId":"retained-target-reported-session"
            },"settings":{"state":"applied","effectiveSettings":null,"permissionDifferences":[]}}
        }),
    )?;
    let raw_after_prune = fixture::machine_work(client, &known.work_item_id, None).await?;
    equal(
        fixture::field::<Vec<Value>>(&raw_after_prune, "/batches")?.len(),
        1,
    )?;
    if fixture::field::<Option<i64>>(&raw_after_prune, "/batches/0/ackedAtMs")?.is_none() {
        return Err("retention fixture has no normal Batch acknowledgement".into());
    }
    let second = session_create_request(&source_service, &target_service)?;
    if !matches!(
        call(source, AutomationCall::RemoteSessionCreate(second)).await?,
        AutomationAnswer::SessionCreate(_)
    ) {
        return Err("unACKed retention admission refused".into());
    }
    let page = pull_work(target).await?;
    equal(page.batches.len(), 1)?;
    let unacked = page
        .batches
        .into_iter()
        .next()
        .ok_or("unACKed retention Batch missing")?;
    Ok(RemoteCreateRestartCase {
        source_service,
        source_peer: source.0.peer.to_string(),
        target_service,
        target_peer: target.0.peer.to_string(),
        request,
        known,
        raw_after_prune,
        unacked,
        now: client
            .automation_clock(AutomationClockControl::Observe)
            .await?
            .now,
    })
}

pub async fn resume_remote_create_restart(
    client: &ConformanceClient,
    prepared: RemoteCreateRestartCase,
) -> ConformanceResult<()> {
    let reopened = client
        .automation_clock(AutomationClockControl::Observe)
        .await?;
    equal(reopened.now, prepared.now)?;
    let mut source = reconnect(client, &prepared.source_service, &prepared.source_peer).await?;
    let mut target = match reconnect(client, &prepared.target_service, &prepared.target_peer).await
    {
        Ok(target) => target,
        Err(error) => {
            source.close().await?;
            return Err(error);
        }
    };
    let proof = after_restart(client, &mut source, &mut target, &prepared).await;
    let source_closed = source.close().await;
    let target_closed = target.close().await;
    proof?;
    source_closed?;
    target_closed?;
    Ok(())
}

async fn after_restart(
    client: &ConformanceClient,
    source: &mut NativeBoardSession,
    target: &mut NativeBoardSession,
    prepared: &RemoteCreateRestartCase,
) -> ConformanceResult<()> {
    let raw_reopened = fixture::machine_work(client, &prepared.known.work_item_id, None).await?;
    equal(raw_reopened.clone(), prepared.raw_after_prune.clone())?;
    equal(
        serde_json::to_value(report_cases::status(source, &prepared.request.operation_id).await?)?,
        serde_json::to_value(&prepared.known)?,
    )?;
    equal(
        serde_json::to_value(pull_work(target).await?.batches)?,
        serde_json::to_value(vec![&prepared.unacked])?,
    )?;
    report_cases::accept_report(
        target,
        &prepared.known.work_item_id,
        2,
        SessionCreateState::Unknown,
        FirstInputState::Unknown,
    )
    .await?;
    let newer = report_cases::status(source, &prepared.request.operation_id).await?;
    equal(newer.report_seq.map(u64::from), Some(2))?;
    equal(
        serde_json::to_value(&newer.create)?,
        serde_json::to_value(&prepared.known.create)?,
    )?;
    equal(
        serde_json::to_value(&newer.input)?,
        serde_json::to_value(&prepared.known.input)?,
    )?;
    let raw_newer = fixture::machine_work(client, &prepared.known.work_item_id, None).await?;
    let mut expected_raw = prepared.raw_after_prune.clone();
    *expected_raw
        .get_mut("resultJson")
        .ok_or("retained result column missing")? = raw_newer
        .get("resultJson")
        .ok_or("newer result column missing")?
        .clone();
    equal(raw_newer.clone(), expected_raw)?;
    let mut expected_result: Value = serde_json::from_str(&fixture::field::<String>(
        &prepared.raw_after_prune,
        "/resultJson",
    )?)?;
    *expected_result
        .get_mut("seq")
        .ok_or("retained sequence missing")? = json!(2);
    equal(
        serde_json::from_str::<Value>(&fixture::field::<String>(&raw_newer, "/resultJson")?)?,
        expected_result,
    )?;
    let denied = call(
        source,
        report_cases::report_call(
            &prepared.known.work_item_id,
            3,
            SessionCreateState::Unknown,
            FirstInputState::Unknown,
        )?,
    )
    .await?;
    if !matches!(
        denied,
        AutomationAnswer::Refused(AutomationRefusal::OriginMismatch)
    ) {
        return Err(format!("retention lost target report owner: {denied:?}").into());
    }
    equal(
        fixture::machine_work(client, &prepared.known.work_item_id, None).await?,
        raw_newer.clone(),
    )?;
    let withdrawn = call(
        target,
        AutomationCall::PullAutomationBatches(PullAutomationBatches {
            after: None,
            max_bytes: 1_048_576,
            work_protocols: Vec::new(),
        }),
    )
    .await?;
    if !matches!(withdrawn, AutomationAnswer::Batches(_)) {
        return Err("capability withdrawal refused".into());
    }
    let replay = call(
        source,
        AutomationCall::RemoteSessionCreate(prepared.request.clone()),
    )
    .await?;
    equal(
        serde_json::to_value(replay)?,
        serde_json::to_value(AutomationAnswer::SessionCreate(newer.clone()))?,
    )?;
    let fresh = session_create_request(&prepared.source_service, &prepared.target_service)?;
    let refused = call(source, AutomationCall::RemoteSessionCreate(fresh.clone())).await?;
    if !matches!(
        refused,
        AutomationAnswer::Refused(AutomationRefusal::TargetNeedsUpgrade { .. })
    ) {
        return Err("withdrawn target admitted new work".into());
    }
    if !matches!(
        call(
            source,
            AutomationCall::RemoteSessionCreateStatus {
                operation_id: fresh.operation_id,
            }
        )
        .await?,
        AutomationAnswer::Refused(AutomationRefusal::NotFound)
    ) {
        return Err("refused admission leaked a retained work item".into());
    }
    equal(
        serde_json::to_value(pull_work(target).await?.batches)?,
        serde_json::to_value(vec![&prepared.unacked])?,
    )?;
    equal(
        fixture::machine_work(client, &prepared.known.work_item_id, None).await?,
        raw_newer.clone(),
    )?;
    println!(
        "REMOTE_CREATE_RESTART_RESULT {}",
        json!({
            "reopenedRaw":raw_reopened,"newerRaw":raw_newer,"newerStatus":newer,
            "ackedBatchPruned":true,"unackedBytesRetained":true,"foreignReportNoWrites":true,
            "replayAfterCapabilityWithdrawal":true,"freshWithdrawnTargetRefused":true,
            "nativeEffects":"target report stand-in only"
        })
    );
    Ok(())
}

async fn reconnect(
    client: &ConformanceClient,
    service: &str,
    peer: &str,
) -> ConformanceResult<NativeBoardSession> {
    Ok(NativeBoardSession(
        enroll_admitted_peer(
            client,
            service,
            peer,
            &format!("automation-catalog-{service}"),
            &durable_steps::Uuid::now_v7().to_string(),
        )
        .await?,
    ))
}
