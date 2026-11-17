//! A warm connection must repair a lost physical alarm before its Welcome is observable.
use super::fixture::{Fixture, INSTALLATION, receive, send};
use central_link_client::{ClientAction, ClientEvent, ConnectionAttempt, LinkClient};
use central_link_protocol::{LinkFrame, ReplyPayload, RequestPayload};
use central_storage::{AlarmUpdate, BatchOutcome, SqlValue, Statement, StatementBatch};
use collaboration_central_conformance::{ConformanceResult as Result, signin_fixtures as signed};
use collaboration_central_core::ENGINE_ALARM_INPUTS;
use reqwest_websocket::WebSocket;
use std::sync::Arc;
use workspace_model::{
    Caller, SessionAddress, WorkspaceAnswer, WorkspaceOperation, WorkspaceRequest, WorkspaceResult,
    central_mail::{
        CentralMailAnswer, CentralMailOperation, CentralMailRequest, CentralMailResult,
    },
};

#[tokio::test]
async fn warm_reconnect_rearms_healthy_wake_before_welcome_is_observable() -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = reconnect_journey(&fixture).await;
    fixture.close().await?;
    result
}

async fn call(
    client: &mut LinkClient,
    attempt: &ConnectionAttempt,
    socket: &mut WebSocket,
    caller: &Caller,
    payload: RequestPayload,
) -> Result<ReplyPayload> {
    let request = client.begin_request(caller.clone(), payload)?;
    send(socket, &request).await?;
    match client.receive(attempt, receive(socket).await?)? {
        ClientEvent::Reply { result, .. } => Ok(result),
        _ => Err("fixture request did not receive its correlated Reply".into()),
    }
}

async fn reconnect_journey(fixture: &Fixture) -> Result<()> {
    let target = SessionAddress {
        machine_id: INSTALLATION.parse()?,
        endpoint: "codex-local".to_owned().try_into()?,
        session_id: "019f0000-0000-7000-8000-00000000cf07"
            .to_owned()
            .try_into()?,
    };
    let caller = Caller::Session(target.clone());
    let mut client = LinkClient::new(INSTALLATION.parse()?, fixture.clock.clone());
    let Some(ClientAction::Connect { attempt }) = client.poll_time() else {
        return Err("fixture client did not start".into());
    };
    let mut original = fixture.connect().await?;
    if client.receive(&attempt, receive(&mut original).await?)? != ClientEvent::Welcomed {
        return Err("fixture original connection did not Welcome".into());
    }
    let baseline = call(
        &mut client,
        &attempt,
        &mut original,
        &caller,
        RequestPayload::Workspace(WorkspaceRequest {
            request_id: Some("019f0000-0000-7000-8000-00000000d501".parse()?),
            workspace_id: None,
            operation: WorkspaceOperation::CreateWorkspace {
                name: "Warm alarm workspace".to_owned().try_into()?,
            },
        }),
    )
    .await?;
    if !matches!(
        baseline,
        ReplyPayload::Workspace(workspace_model::WorkspaceReply {
            result: WorkspaceResult::Done(WorkspaceAnswer::WorkspaceCreated { .. }),
            ..
        })
    ) {
        return Err("baseline workspace did not establish the normal session".into());
    }
    let due: workspace_model::Timestamp = "2030-03-17T17:55:00Z".parse()?;
    let wake = call(
        &mut client,
        &attempt,
        &mut original,
        &caller,
        RequestPayload::Mail(CentralMailRequest {
            request_id: "019f0000-0000-7000-8000-00000000d502".parse()?,
            operation: CentralMailOperation::SetWake {
                target,
                due_at: due,
                text: "Healthy reconnect wake".to_owned().try_into()?,
            },
        }),
    )
    .await?;
    let ReplyPayload::Mail(CentralMailResult::Done(CentralMailAnswer::WakeSet { wake_id, due_at })) =
        wake
    else {
        return Err("healthy future wake fixture was refused".into());
    };
    if due_at != due {
        return Err("wake fixture deadline changed".into());
    }
    let organization = signed::ORGANIZATION.to_owned().try_into()?;
    let runtime = fixture.host.central(&organization).await?;
    let same_before = fixture.host.central(&organization).await?;
    if !Arc::ptr_eq(&runtime, &same_before) {
        return Err("fixture runtime was not cached".into());
    }
    let rows = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT wake_id,due_at,text,state FROM wakes ORDER BY wake_id",
            args: vec![],
        })
        .await?;
    let expected_rows = vec![vec![
        SqlValue::Text(wake_id.to_string()),
        SqlValue::Text(due.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        SqlValue::Text("Healthy reconnect wake".into()),
        SqlValue::Text("active".into()),
    ]];
    if rows.rows != expected_rows
        || runtime.statement_host().alarm().map(|at| at.unix_millis())
            != Some(due.timestamp_millis())
    {
        return Err("healthy wake fixture did not retain its actual armed alarm and row".into());
    }

    // Fault only the actual retained alarm. No row, runtime or injected time changes.
    let dropped = runtime
        .statement_host()
        .run_batch(StatementBatch {
            statements: vec![],
            alarm: AlarmUpdate::SetFromQuery(Statement {
                sql: "SELECT NULL AS earliest_due",
                args: vec![],
            }),
        })
        .await?;
    if !matches!(dropped, BatchOutcome::Applied { .. })
        || runtime.statement_host().alarm().is_some()
    {
        return Err("physical alarm fault did not clear the actual native alarm".into());
    }
    let mut reopened = fixture.connect().await?;
    let welcome = receive(&mut reopened).await?;
    if !matches!(welcome, LinkFrame::Data(central_link_protocol::DataFrame::Welcome { machine_id, .. }) if machine_id.as_str() == INSTALLATION)
    {
        return Err("warm reconnect did not receive its authenticated Welcome".into());
    }
    // This physical read precedes any runtime lookup, core query or semantic frame.
    let physical_after_welcome = runtime.statement_host().alarm().map(|at| at.unix_millis());
    let same_after = fixture.host.central(&organization).await?;
    if !Arc::ptr_eq(&runtime, &same_after) {
        return Err("reconnect cold-opened a different runtime".into());
    }
    let earliest = runtime
        .statement_host()
        .read(Statement {
            sql: ENGINE_ALARM_INPUTS,
            args: vec![],
        })
        .await?;
    let after_rows = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT wake_id,due_at,text,state FROM wakes ORDER BY wake_id",
            args: vec![],
        })
        .await?;
    if after_rows != rows {
        return Err("reconnect changed healthy retained wake rows".into());
    }
    let [row] = earliest.rows.as_slice() else {
        return Err("canonical earliest deadline missing".into());
    };
    // Engine inputs: (on_grid_due, legacy_due); only the healthy wake is due.
    let [SqlValue::Integer(earliest), SqlValue::Null] = row.as_slice() else {
        return Err("healthy canonical deadline was not an integer".into());
    };
    if earliest.get() != due.timestamp_millis() {
        return Err("healthy canonical deadline changed".into());
    }
    if physical_after_welcome != Some(earliest.get()) {
        return Err(format!("warm Welcome left physical alarm {physical_after_welcome:?}; canonical healthy deadline is {}", earliest.get()).into());
    }
    reopened
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    original
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    Ok(())
}

#[derive(Clone, Default)]
struct CapturedRearmLogs(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
impl std::io::Write for CapturedRearmLogs {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| std::io::Error::other("rearm log capture poisoned"))?
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl CapturedRearmLogs {
    fn snapshot(&self) -> Result<Vec<u8>> {
        Ok(self
            .0
            .lock()
            .map_err(|_| "rearm log capture poisoned")?
            .clone())
    }
}

#[tokio::test]
async fn unavailable_alarm_publication_still_delivers_welcome_and_reports_typed_failure()
-> Result<()> {
    // Nextest runs each case in its own process, matching native_access_logging's capture.
    let logs = CapturedRearmLogs::default();
    let output = logs.clone();
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .json()
            .with_ansi(false)
            .without_time()
            .with_writer(move || output.clone())
            .finish(),
    )?;
    let fixture = Fixture::new().await?;
    let result = unavailable_rearm_journey(&fixture, &logs).await;
    fixture.close().await?;
    result
}

async fn unavailable_rearm_journey(fixture: &Fixture, logs: &CapturedRearmLogs) -> Result<()> {
    let target = SessionAddress {
        machine_id: INSTALLATION.parse()?,
        endpoint: "codex-local".to_owned().try_into()?,
        session_id: "019f0000-0000-7000-8000-00000000cf07"
            .to_owned()
            .try_into()?,
    };
    let caller = Caller::Session(target.clone());
    let mut client = LinkClient::new(INSTALLATION.parse()?, fixture.clock.clone());
    let Some(ClientAction::Connect { attempt }) = client.poll_time() else {
        return Err("failure fixture client did not start".into());
    };
    let mut original = fixture.connect().await?;
    if client.receive(&attempt, receive(&mut original).await?)? != ClientEvent::Welcomed {
        return Err("failure fixture original connection did not Welcome".into());
    }
    let baseline = call(
        &mut client,
        &attempt,
        &mut original,
        &caller,
        RequestPayload::Workspace(WorkspaceRequest {
            request_id: Some("019f0000-0000-7000-8000-00000000d521".parse()?),
            workspace_id: None,
            operation: WorkspaceOperation::CreateWorkspace {
                name: "Nonfatal alarm workspace".to_owned().try_into()?,
            },
        }),
    )
    .await?;
    if !matches!(
        baseline,
        ReplyPayload::Workspace(workspace_model::WorkspaceReply {
            result: WorkspaceResult::Done(WorkspaceAnswer::WorkspaceCreated { .. }),
            ..
        })
    ) {
        return Err("failure fixture baseline workspace refused".into());
    }
    let due: workspace_model::Timestamp = "2030-03-17T17:55:00Z".parse()?;
    let wake = call(
        &mut client,
        &attempt,
        &mut original,
        &caller,
        RequestPayload::Mail(CentralMailRequest {
            request_id: "019f0000-0000-7000-8000-00000000d522".parse()?,
            operation: CentralMailOperation::SetWake {
                target,
                due_at: due,
                text: "Nonfatal reconnect wake".to_owned().try_into()?,
            },
        }),
    )
    .await?;
    let ReplyPayload::Mail(CentralMailResult::Done(CentralMailAnswer::WakeSet { wake_id, due_at })) =
        wake
    else {
        return Err("failure fixture healthy wake refused".into());
    };
    if due_at != due {
        return Err("failure fixture wake deadline changed".into());
    }
    let organization = signed::ORGANIZATION.to_owned().try_into()?;
    let runtime = fixture.host.central(&organization).await?;
    let rows = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT wake_id,due_at,text,state FROM wakes ORDER BY wake_id",
            args: vec![],
        })
        .await?;
    if rows.rows
        != vec![vec![
            SqlValue::Text(wake_id.to_string()),
            SqlValue::Text(due.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
            SqlValue::Text("Nonfatal reconnect wake".into()),
            SqlValue::Text("active".into()),
        ]]
        || runtime.statement_host().alarm().map(|at| at.unix_millis())
            != Some(due.timestamp_millis())
    {
        return Err("failure fixture healthy armed wake missing".into());
    }
    let dropped = runtime
        .statement_host()
        .run_batch(StatementBatch {
            statements: vec![],
            alarm: AlarmUpdate::SetFromQuery(Statement {
                sql: "SELECT NULL AS earliest_due",
                args: vec![],
            }),
        })
        .await?;
    if !matches!(dropped, BatchOutcome::Applied { .. })
        || runtime.statement_host().alarm().is_some()
    {
        return Err("failure fixture physical alarm did not clear".into());
    }
    let log_baseline = logs.snapshot()?.len();
    runtime
        .statement_host()
        .fail_next_alarm_batch(central_storage::StoreFailure::Unavailable);
    let mut reopened = fixture.connect().await?;
    if !matches!(receive(&mut reopened).await?, LinkFrame::Data(central_link_protocol::DataFrame::Welcome { machine_id, .. }) if machine_id.as_str() == INSTALLATION)
    {
        return Err("Unavailable alarm publication suppressed authenticated Welcome".into());
    }
    // Capture before any later runtime request can repair the missing deadline.
    let physical_after_welcome = runtime.statement_host().alarm();
    let captured = logs.snapshot()?;
    let interval = std::str::from_utf8(
        captured
            .get(log_baseline..)
            .ok_or("rearm log interval changed")?,
    )?;
    let events = interval
        .lines()
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let failures = events
        .iter()
        .filter(|event| {
            event
                .pointer("/fields/message")
                .and_then(serde_json::Value::as_str)
                == Some("post_welcome_alarm_recompute_failed")
        })
        .collect::<Vec<_>>();
    let [failure] = failures.as_slice() else {
        return Err("failed native rearm must report exactly one diagnostic".into());
    };
    if failure.pointer("/fields/failure") != Some(&serde_json::json!("Unavailable"))
        || failure.get("level") != Some(&serde_json::json!("WARN"))
    {
        return Err("native rearm diagnostic must retain typed Unavailable at WARN".into());
    }
    if interval.contains(&fixture.token) {
        return Err("native rearm diagnostic exposed the bearer token".into());
    }
    if physical_after_welcome.is_some() {
        return Err("failed native rearm must not retry alarm publication".into());
    }
    let same = fixture.host.central(&organization).await?;
    if !Arc::ptr_eq(&same, &runtime) {
        return Err("nonfatal reconnect replaced its warm runtime".into());
    }
    let after = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT wake_id,due_at,text,state FROM wakes ORDER BY wake_id",
            args: vec![],
        })
        .await?;
    if after != rows {
        return Err("failed publication changed healthy native wake rows".into());
    }
    reopened
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    original
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    Ok(())
}
