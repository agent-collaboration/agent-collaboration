//! A running summary turn through the scheduler and a fake native app-server: the summary's
//! deadline arms the scheduler's timer, and at that time, not before, the scheduler stores
//! the stop and interrupts the summary turn once.
use super::{BASE_MS, SchedulerFixture, TestResult, WAIT_LIMIT, advance, at, settled};
use agent_automation::{
    CessationEvidence, PreparationEffect, RunId, SubmissionEffect, SummaryAttempt, SummaryPhase,
};
use collaboration_protocol::{CodexGeneration, EndpointRef, SessionRef};
use futures_util::{SinkExt as _, StreamExt as _};
use runtime_primitives::{Clock, SyncTime};
use serde_json::{Value, json};
use sqlx::Connection as _;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio_tungstenite::tungstenite::Message;

const SERVICE: &str = "00000000-0000-4000-8000-000000000001";
const SUMMARY_SECONDS: u64 = 60;

/// One request the fake app-server answered, with the wall time it arrived.
type NativeRequest = (String, SyncTime);

/// Answers every connection: the summary turn is still in progress, and an interrupt is
/// accepted. Any other request, or one for another turn, ends the server with an error.
async fn serve_running_summary(
    listener: tokio::net::UnixListener,
    clock: Arc<crate::paused_wall_clock::PausedWallClock>,
    requests: Arc<std::sync::Mutex<Vec<NativeRequest>>>,
) -> TestResult<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let mut socket = tokio_tungstenite::accept_async(stream).await?;
        let init: Value = serde_json::from_str(socket.next().await.ok_or("init")??.to_text()?)?;
        socket
            .send(Message::Text(
                json!({"id":init.get("id"),"result":{}}).to_string().into(),
            ))
            .await?;
        let _initialized = socket.next().await.ok_or("initialized")??;
        let request: Value =
            serde_json::from_str(socket.next().await.ok_or("request")??.to_text()?)?;
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .ok_or("method")?
            .to_owned();
        if request.pointer("/params/threadId") != Some(&json!("summary-thread")) {
            return Err("a request for another thread".into());
        }
        let result = match method.as_str() {
            "thread/turns/list" => json!({
                "data":[{"id":"summary-turn","status":"inProgress","items":[]}],
                "nextCursor":null
            }),
            "turn/interrupt"
                if request.pointer("/params/turnId") == Some(&json!("summary-turn")) =>
            {
                json!({})
            }
            _ => return Err(format!("unexpected request {request}").into()),
        };
        requests.lock().unwrap().push((method, clock.now()));
        socket
            .send(Message::Text(
                json!({"id":request.get("id"),"result":result})
                    .to_string()
                    .into(),
            ))
            .await?;
    }
}

fn native_schemas() -> TestResult<Arc<codex_native_integration::NativePayloadSchemas>> {
    let mut definitions = serde_json::Map::new();
    for name in [
        "ThreadRead",
        "ThreadTurnsList",
        "ThreadResume",
        "ThreadStart",
        "ThreadLoadedList",
        "TurnStart",
        "TurnSteer",
        "TurnInterrupt",
    ] {
        definitions.insert(format!("{name}Params"), json!({"type":"object"}));
        definitions.insert(format!("{name}Response"), json!({"type":"object"}));
    }
    let bundle = codex_native_integration::NativeSchemaBundle::from_documents(BTreeMap::from([(
        "codex_app_server_protocol.schemas.json".to_owned(),
        serde_json::to_vec(&json!({"definitions":{"v2":definitions}}))?,
    )]))?;
    Ok(Arc::new(
        codex_native_integration::NativePayloadSchemas::from_bundle(&bundle)?,
    ))
}

impl SchedulerFixture {
    /// Seeds a Run whose summary turn runs on `summary-thread` until base + 60 s.
    async fn seed_running_summary(&self) -> TestResult<RunId> {
        let target: SessionRef = serde_json::from_value(json!({
            "endpoint":{"serviceId":SERVICE,"endpointId":"codex-local"},
            "sessionId":"summary-thread"
        }))?;
        let generation: CodexGeneration =
            serde_json::from_value(json!({"serviceEpoch":SERVICE,"generation":1}))?;
        let effects = agent_automation::NativeEffectEvidence {
            permission_differences: Vec::new(),
            target: Some(target.clone()),
            generation: Some(generation),
            client_user_message_id: None,
            native_turn_id: Some("summary-turn".into()),
            native_submission_id: None,
            allocation: PreparationEffect::Accepted,
            resume: PreparationEffect::NotRequested,
            submission: SubmissionEffect::Accepted,
            cessation: CessationEvidence::Unconfirmed,
        };
        let attempt = SummaryAttempt {
            attempt_id: agent_automation::AttemptId::generate(),
            source_target: target.clone(),
            source_reference: agent_automation::SummarySourceReference::NativeTurn {
                turn_id: "worker-turn".into(),
            },
            target: Some(target),
            native_turn_id: Some("summary-turn".into()),
            effective_timeout_seconds: u32::try_from(SUMMARY_SECONDS)?,
            started_at_ms: BASE_MS,
            deadline_at_ms: at(Duration::from_secs(SUMMARY_SECONDS)).unix_millis(),
            phase: SummaryPhase::Running,
            effects: effects.clone(),
            explanation: None,
        };
        let evidence = agent_automation::RunExecutionEvidence {
            route: Some(effects.into()),
            timing: None,
            acceptance: None::<crate::stored_run_receipt::StoredRunReceipt>,
        };
        let run_id = RunId::generate();
        let schedule_id = agent_automation::ScheduleId::generate();
        let instruction_id = agent_automation::InstructionId::generate();
        let revision_id = agent_automation::RevisionId::generate();
        let mut seed = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(self.root.path().join("automation.sqlite"))
                .foreign_keys(true),
        )
        .await?;
        let mut transaction = seed.begin().await?;
        sqlx::query("INSERT INTO instruction_documents VALUES (?,?, 'fixture',0)")
            .bind(instruction_id.as_str())
            .bind(revision_id.as_str())
            .execute(&mut *transaction)
            .await?;
        sqlx::query("INSERT INTO instruction_revisions VALUES (?,?,'fixture',NULL,0)")
            .bind(revision_id.as_str())
            .bind(instruction_id.as_str())
            .execute(&mut *transaction)
            .await?;
        sqlx::query("INSERT INTO schedule_definitions VALUES (?,?,?,0,'{}','{}',0,0)")
            .bind(schedule_id.as_str())
            .bind(agent_automation::ChangeId::generate().as_str())
            .bind(instruction_id.as_str())
            .execute(&mut *transaction)
            .await?;
        sqlx::query("INSERT INTO workflow_runs(run_id,schedule_id,due_at_ms,run_status,execution_evidence_json,summary_attempt_json,native_turn_id) VALUES (?,?,0,'summaryRunning',?,?,'worker-turn')")
            .bind(run_id.as_str())
            .bind(schedule_id.as_str())
            .bind(serde_json::to_string(&evidence)?)
            .bind(serde_json::to_string(&attempt)?)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        seed.close().await?;
        Ok(run_id)
    }

    async fn summary_phase(&self, run: &RunId) -> TestResult<Option<SummaryPhase>> {
        Ok(self
            .store
            .lock()
            .await
            .read_run::<SessionRef, EndpointRef, CodexGeneration, crate::stored_run_receipt::StoredRunReceipt>(run)
            .await?
            .summary_attempt
            .map(|attempt| attempt.phase))
    }
}

/// Waits, without moving the clock, until the server answered `count` requests. It fails
/// once no request arrived for a whole wait limit.
async fn answered(requests: &std::sync::Mutex<Vec<NativeRequest>>, count: usize) -> TestResult<()> {
    let mut seen = requests.lock().unwrap().len();
    let mut idle = 0;
    while idle < WAIT_LIMIT {
        let now_seen = requests.lock().unwrap().len();
        if now_seen >= count {
            return Ok(());
        }
        if now_seen == seen {
            idle += 1;
        } else {
            seen = now_seen;
            idle = 0;
        }
        tokio::task::yield_now().await;
    }
    Err(format!("native requests stalled at {seen} of {count}").into())
}

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_running_summary_is_interrupted_once_at_its_deadline() -> TestResult<()> {
    let fixture = SchedulerFixture::empty().await?;
    let run = fixture.seed_running_summary().await?;
    let socket_root = tempfile::Builder::new()
        .prefix("summary-deadline")
        .tempdir_in("/tmp")?;
    let socket = socket_root.path().join("native.sock");
    let listener = tokio::net::UnixListener::bind(&socket)?;
    let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
    let server = tokio::spawn(serve_running_summary(
        listener,
        Arc::clone(&fixture.clock),
        Arc::clone(&requests),
    ));
    let gate = crate::NativeGenerationGate::default();
    gate.activate(
        serde_json::from_value(json!({"serviceEpoch":SERVICE,"generation":1}))?,
        socket,
        Some(native_schemas()?),
    )?;
    let backend = crate::NativeControlBackend {
        endpoint: serde_json::from_value(json!({"serviceId":SERVICE,"endpointId":"codex-local"}))?,
        gate,
        codex_home: socket_root.path().to_path_buf(),
    };
    let deadline = at(Duration::from_secs(SUMMARY_SECONDS));

    let scheduler = fixture.start_on(Some(backend), |worker| worker)?;
    answered(&requests, 1).await?;
    settled(&scheduler).await?;
    let armed = scheduler.observation.snapshot().armed_deadline;
    advance(Duration::from_secs(SUMMARY_SECONDS - 1)).await;
    settled(&scheduler).await?;
    let before_deadline = requests.lock().unwrap().len();
    advance(Duration::from_secs(1)).await;
    answered(&requests, 4).await?;
    settled(&scheduler).await?;
    let armed_after = scheduler.observation.snapshot().armed_deadline;
    advance(Duration::from_secs(3_600)).await;
    settled(&scheduler).await?;

    assert_eq!(armed, Some(deadline), "the summary deadline arms the timer");
    assert_eq!(before_deadline, 1, "nothing acts before the deadline");
    assert_eq!(
        *requests.lock().unwrap(),
        [
            ("thread/turns/list".to_owned(), at(Duration::ZERO)),
            ("thread/turns/list".to_owned(), deadline),
            ("turn/interrupt".to_owned(), deadline),
            ("thread/turns/list".to_owned(), deadline),
        ],
        "one interrupt at the deadline; the stopping summary is only read again"
    );
    assert_eq!(
        fixture.summary_phase(&run).await?,
        Some(SummaryPhase::Stopping)
    );
    assert_eq!(armed_after, None, "a stopping summary arms nothing");
    assert!(
        !server.is_finished(),
        "the server saw only expected requests"
    );
    scheduler.stop().await;
    server.abort();
    Ok(())
}
