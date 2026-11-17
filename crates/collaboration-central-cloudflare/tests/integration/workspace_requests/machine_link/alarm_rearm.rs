//! A second warm upgrade repairs the real platform alarm without a semantic frame.
use super::super::workerd_host::c2_controls::{WitnessEvent, WitnessPhase};
use super::baseline;
use super::fixture::{ConnectedLink, Fixture, session_caller};
use central_link_protocol::{DataFrame, ReplyPayload, RequestPayload};
use central_storage::{SqlValue, Statement};
use collaboration_central_conformance::ConformanceResult as Result;
use collaboration_central_core::ENGINE_ALARM_INPUTS;
use workspace_model::{
    Caller,
    central_mail::{
        CentralMailAnswer, CentralMailOperation, CentralMailRequest, CentralMailResult,
    },
};

#[tokio::test]
async fn warm_reconnect_rearms_platform_alarm_before_welcome_is_observable() -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = reconnect_journey(&fixture).await;
    fixture.close().await?;
    result
}

async fn reconnect_journey(fixture: &Fixture) -> Result<()> {
    // Baseline creates the actual normal session identity while the first socket stays open.
    let (mut original, _, _, _, _) = baseline(fixture).await?;
    let Caller::Session(target) = session_caller()? else {
        return Err("normal session caller fixture changed".into());
    };
    let due: workspace_model::Timestamp = "2030-03-17T17:55:00Z".parse()?;
    let (_, replies) = original
        .batch(vec![RequestPayload::Mail(CentralMailRequest {
            request_id: "019f0000-0000-7000-8000-00000000d512".parse()?,
            operation: CentralMailOperation::SetWake {
                target,
                due_at: due,
                text: "Healthy warm Worker wake".to_owned().try_into()?,
            },
        })])
        .await?;
    let [DataFrame::Reply { result, .. }] = replies.as_slice() else {
        return Err("healthy wake did not return one real correlated Reply".into());
    };
    let ReplyPayload::Mail(CentralMailResult::Done(CentralMailAnswer::WakeSet { wake_id, due_at })) =
        result.as_ref()
    else {
        return Err("healthy future Worker wake fixture was refused".into());
    };
    if due_at != &due {
        return Err("healthy Worker wake deadline changed".into());
    }
    let runtime = fixture.runtime();
    let rows = runtime
        .read(Statement {
            sql: "SELECT wake_id,due_at,text,state FROM wakes ORDER BY wake_id",
            args: vec![],
        })
        .await?;
    let expected_rows = vec![vec![
        SqlValue::Text(wake_id.to_string()),
        SqlValue::Text(due.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        SqlValue::Text("Healthy warm Worker wake".into()),
        SqlValue::Text("active".into()),
    ]];
    let armed: Option<i64> = runtime.control("raw-alarm-time", &()).await?;
    if rows.rows != expected_rows || armed != Some(due.timestamp_millis()) {
        return Err(
            "healthy Worker fixture must retain its actual armed platform alarm and row".into(),
        );
    }
    let before = fixture.host.c2_witness(WitnessPhase::Connecting).await?;
    let constructors_before = before
        .iter()
        .filter(|record| matches!(record.event, WitnessEvent::Constructor { .. }))
        .cloned()
        .collect::<Vec<_>>();
    let upgrades_before = before
        .iter()
        .filter(|record| matches!(record.event, WitnessEvent::Upgrade { .. }))
        .collect::<Vec<_>>();
    let [first_upgrade] = upgrades_before.as_slice() else {
        return Err("warm Worker fixture needs exactly one existing upgraded socket".into());
    };
    let first_instance = first_upgrade.instance_id;

    // The fault control touches storage.delete_alarm only; it never opens a core runtime.
    let dropped: bool = runtime.control("raw-drop-alarm", &()).await?;
    let cleared: Option<i64> = runtime.control("raw-alarm-time", &()).await?;
    if !dropped || cleared.is_some() {
        return Err("physical Worker alarm fault did not clear the actual platform alarm".into());
    }
    let reopened = ConnectedLink::open(fixture).await?;
    // FIRST after authenticated Welcome: read actual platform state, before any core query.
    let physical_after_welcome: Option<i64> = runtime.control("raw-alarm-time", &()).await?;
    let after = fixture.host.c2_witness(WitnessPhase::Connecting).await?;
    if !after.starts_with(&before) {
        return Err("warm Worker witness history changed".into());
    }
    let constructors_after = after
        .iter()
        .filter(|record| matches!(record.event, WitnessEvent::Constructor { .. }))
        .cloned()
        .collect::<Vec<_>>();
    if constructors_after != constructors_before {
        return Err("reconnect constructed a cold Worker and would mask warm rearm".into());
    }
    let upgrades_after = after
        .iter()
        .filter(|record| matches!(record.event, WitnessEvent::Upgrade { .. }))
        .collect::<Vec<_>>();
    if upgrades_after.len() != upgrades_before.len() + 1
        || upgrades_after
            .last()
            .is_none_or(|record| record.instance_id != first_instance)
    {
        return Err("second authenticated Welcome must use the same warm Worker instance".into());
    }

    // Core reads happen only after the captured physical result, so request safety cannot mask it.
    let earliest = runtime
        .read(Statement {
            sql: ENGINE_ALARM_INPUTS,
            args: vec![],
        })
        .await?;
    let after_rows = runtime
        .read(Statement {
            sql: "SELECT wake_id,due_at,text,state FROM wakes ORDER BY wake_id",
            args: vec![],
        })
        .await?;
    if after_rows != rows {
        return Err("warm Worker reconnect changed healthy wake rows".into());
    }
    let [row] = earliest.rows.as_slice() else {
        return Err("canonical Worker deadline missing".into());
    };
    // Engine inputs: (on_grid_due, legacy_due); only the healthy wake is due.
    let [SqlValue::Integer(earliest), SqlValue::Null] = row.as_slice() else {
        return Err("canonical healthy Worker deadline was not an integer".into());
    };
    if earliest.get() != due.timestamp_millis() {
        return Err("canonical healthy Worker deadline changed".into());
    }
    if physical_after_welcome != Some(earliest.get()) {
        return Err(format!("warm Worker Welcome left physical alarm {physical_after_welcome:?}; canonical healthy deadline is {}", earliest.get()).into());
    }
    reopened.close().await?;
    original.close().await?;
    Ok(())
}

#[tokio::test]
async fn unavailable_alarm_publication_still_delivers_worker_welcome_and_reports_typed_failure()
-> Result<()> {
    let fixture = Fixture::new().await?;
    let result = unavailable_rearm_journey(&fixture).await;
    fixture.close().await?;
    result
}

async fn unavailable_rearm_journey(fixture: &Fixture) -> Result<()> {
    let (mut original, _, _, _, _) = baseline(fixture).await?;
    let Caller::Session(target) = session_caller()? else {
        return Err("normal session caller fixture changed".into());
    };
    let due: workspace_model::Timestamp = "2030-03-17T17:55:00Z".parse()?;
    let (_, replies) = original
        .batch(vec![RequestPayload::Mail(CentralMailRequest {
            request_id: "019f0000-0000-7000-8000-00000000d532".parse()?,
            operation: CentralMailOperation::SetWake {
                target,
                due_at: due,
                text: "Nonfatal warm Worker wake".to_owned().try_into()?,
            },
        })])
        .await?;
    let [DataFrame::Reply { result, .. }] = replies.as_slice() else {
        return Err("failure fixture wake Reply missing".into());
    };
    let ReplyPayload::Mail(CentralMailResult::Done(CentralMailAnswer::WakeSet { wake_id, due_at })) =
        result.as_ref()
    else {
        return Err("failure fixture healthy Worker wake refused".into());
    };
    if due_at != &due {
        return Err("failure fixture healthy Worker wake deadline changed".into());
    }
    let runtime = fixture.runtime();
    let rows = runtime
        .read(Statement {
            sql: "SELECT wake_id,due_at,text,state FROM wakes ORDER BY wake_id",
            args: vec![],
        })
        .await?;
    let armed: Option<i64> = runtime.control("raw-alarm-time", &()).await?;
    if rows.rows
        != vec![vec![
            SqlValue::Text(wake_id.to_string()),
            SqlValue::Text(due.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
            SqlValue::Text("Nonfatal warm Worker wake".into()),
            SqlValue::Text("active".into()),
        ]]
        || armed != Some(due.timestamp_millis())
    {
        return Err("failure fixture healthy armed Worker wake missing".into());
    }
    let before = fixture.host.c2_witness(WitnessPhase::Connecting).await?;
    let constructors_before = before
        .iter()
        .filter(|record| matches!(record.event, WitnessEvent::Constructor { .. }))
        .cloned()
        .collect::<Vec<_>>();
    let upgrades_before = before
        .iter()
        .filter(|record| matches!(record.event, WitnessEvent::Upgrade { .. }))
        .collect::<Vec<_>>();
    let [first_upgrade] = upgrades_before.as_slice() else {
        return Err("nonfatal fixture needs one warm upgraded Worker".into());
    };
    let first_instance = first_upgrade.instance_id;
    let dropped: bool = runtime.control("raw-drop-alarm", &()).await?;
    let cleared: Option<i64> = runtime.control("raw-alarm-time", &()).await?;
    if !dropped || cleared.is_some() {
        return Err("nonfatal Worker physical alarm fault did not apply".into());
    }
    let injected: bool = runtime
        .control(
            "fail-alarm-batch",
            &central_storage::StoreFailure::Unavailable,
        )
        .await?;
    let after_injection: Option<i64> = runtime.control("raw-alarm-time", &()).await?;
    if !injected || after_injection.is_some() {
        return Err("nonfatal alarm-only failure injection changed physical alarm".into());
    }
    let log_baseline = fixture.host.captured_logs().len();

    let reopened = ConnectedLink::open(fixture).await?;
    // Welcome must survive the failed real publication; no retry may repair the physical alarm.
    let physical_after_welcome: Option<i64> = runtime.control("raw-alarm-time", &()).await?;
    fixture
        .host
        .until_log_after(log_baseline, "post_welcome_alarm_recompute_failed")
        .await;
    let logs = fixture.host.captured_logs();
    let interval = logs
        .get(log_baseline..)
        .ok_or("Worker rearm log interval changed")?;
    let failures = interval
        .iter()
        .filter(|line| line.contains("post_welcome_alarm_recompute_failed"))
        .collect::<Vec<_>>();
    let [failure] = failures.as_slice() else {
        return Err("failed Worker rearm must report exactly one diagnostic".into());
    };
    if !failure.contains("failure=Unavailable") {
        return Err("Worker rearm diagnostic lost typed Unavailable".into());
    }
    if interval.iter().any(|line| line.contains(&fixture.token)) {
        return Err("Worker rearm diagnostic exposed the bearer token".into());
    }
    if physical_after_welcome.is_some() {
        return Err("failed Worker rearm must not retry alarm publication".into());
    }
    let after = fixture.host.c2_witness(WitnessPhase::Connecting).await?;
    if !after.starts_with(&before) {
        return Err("nonfatal Worker witness history changed".into());
    }
    let constructors_after = after
        .iter()
        .filter(|record| matches!(record.event, WitnessEvent::Constructor { .. }))
        .cloned()
        .collect::<Vec<_>>();
    let upgrades_after = after
        .iter()
        .filter(|record| matches!(record.event, WitnessEvent::Upgrade { .. }))
        .collect::<Vec<_>>();
    if constructors_after != constructors_before
        || upgrades_after.len() != upgrades_before.len() + 1
        || upgrades_after
            .last()
            .is_none_or(|record| record.instance_id != first_instance)
    {
        return Err("nonfatal reconnect must keep the same warm Worker instance".into());
    }
    let after_rows = runtime
        .read(Statement {
            sql: "SELECT wake_id,due_at,text,state FROM wakes ORDER BY wake_id",
            args: vec![],
        })
        .await?;
    if after_rows != rows {
        return Err("failed Worker publication changed healthy wake rows".into());
    }
    reopened.close().await?;
    original.close().await?;
    Ok(())
}
