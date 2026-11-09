//! A running summary turn through the scheduler and a fake native app-server: the summary's
//! deadline arms the scheduler's timer, and at that time, not before, the scheduler stores
//! the stop and interrupts the summary turn once.
use super::{BASE_MS, SchedulerFixture, TestResult, advance, at, settled, wait_until};
use agent_automation::{
    CessationEvidence, PreparationEffect, RunId, SubmissionEffect, SummaryAttempt, SummaryPhase,
};
use collaboration_protocol::{CodexGeneration, EndpointRef, SessionRef};
use futures_util::{SinkExt as _, StreamExt as _};
use router_clock::ClockInstant;
use runtime_primitives::{Clock, SyncTime};
use serde_json::{Value, json};
use sqlx::Connection as _;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio_tungstenite::tungstenite::Message;

const SERVICE: &str = "00000000-0000-4000-8000-000000000001";
const SUMMARY_SECONDS: u64 = 60;

/// One request the fake app-server answered, with the wall time it arrived.
type NativeRequest = (String, SyncTime);

/// How the fake app-server answers.
#[derive(Clone, Default)]
struct NativeScript {
    /// Connections (counting from 1) closed before the handshake: the client cannot
    /// connect.
    dropped_connections: Vec<usize>,
    /// After an interrupt, the turn reads as interrupted.
    interrupt_ends_turn: bool,
    /// After an interrupt, this many reads find the turn absent from the history.
    absent_reads_after_interrupt: usize,
    held_history: Option<Arc<tokio::sync::Notify>>,
}

/// Answers each connection: the summary turn is in progress until interrupted, and an
/// interrupt is accepted. Any other request, or one for another turn, ends the server
/// with an error.
async fn serve_running_summary(
    listener: tokio::net::UnixListener,
    clock: Arc<router_clock::ManualClock>,
    requests: Arc<std::sync::Mutex<Vec<NativeRequest>>>,
    script: NativeScript,
) -> TestResult<()> {
    let mut connections = 0;
    let mut interrupted = false;
    let mut absent_reads = script.absent_reads_after_interrupt;
    loop {
        let (stream, _) = listener.accept().await?;
        connections += 1;
        if script.dropped_connections.contains(&connections) {
            drop(stream);
            continue;
        }
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
            "thread/turns/list" if interrupted && absent_reads > 0 => {
                absent_reads -= 1;
                json!({"data":[],"nextCursor":null})
            }
            "thread/turns/list" => {
                let status = if interrupted && script.interrupt_ends_turn {
                    "interrupted"
                } else {
                    "inProgress"
                };
                json!({
                    "data":[{"id":"summary-turn","status":status,"items":[]}],
                    "nextCursor":null
                })
            }
            "turn/interrupt"
                if request.pointer("/params/turnId") == Some(&json!("summary-turn")) =>
            {
                interrupted = true;
                json!({})
            }
            _ => return Err(format!("unexpected request {request}").into()),
        };
        requests.lock().unwrap().push((method.clone(), clock.now()));
        if method == "thread/turns/list"
            && let Some(hold) = &script.held_history
        {
            hold.notified().await;
        }
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
    wait_until(
        "the fake app-server answered every expected request",
        || requests.lock().unwrap().len() >= count,
    )
    .await
}

/// A seeded running summary, its fake app-server and a scheduler with a native backend.
struct SummaryRig {
    fixture: SchedulerFixture,
    run: RunId,
    requests: Arc<std::sync::Mutex<Vec<NativeRequest>>>,
    server: tokio::task::JoinHandle<TestResult<()>>,
    scheduler: super::RunningScheduler,
    _socket_root: tempfile::TempDir,
}

impl SummaryRig {
    async fn start(script: NativeScript) -> TestResult<Self> {
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
            script,
        ));
        let gate = crate::NativeGenerationGate::default();
        gate.activate(
            serde_json::from_value(json!({"serviceEpoch":SERVICE,"generation":1}))?,
            socket,
            Some(native_schemas()?),
        )?;
        let backend = crate::NativeControlBackend {
            endpoint: serde_json::from_value(
                json!({"serviceId":SERVICE,"endpointId":"codex-local"}),
            )?,
            gate,
            codex_home: socket_root.path().to_path_buf(),
        };
        let scheduler = fixture.start_on(Some(backend), |worker| worker)?;
        Ok(Self {
            fixture,
            run,
            requests,
            server,
            scheduler,
            _socket_root: socket_root,
        })
    }

    /// Waits for `count` answered requests, then for the scheduler to sleep.
    async fn answered(&self, count: usize) -> TestResult<()> {
        answered(&self.requests, count).await?;
        settled(&self.scheduler).await
    }

    fn requests(&self) -> Vec<NativeRequest> {
        self.requests.lock().unwrap().clone()
    }

    fn armed(&self) -> Option<SyncTime> {
        self.scheduler.observation.snapshot().armed_deadline
    }

    async fn stop(self) -> TestResult<()> {
        let server_failed = self.server.is_finished();
        self.scheduler.stop().await;
        self.server.abort();
        if server_failed {
            return Err("the fake app-server saw an unexpected request".into());
        }
        Ok(())
    }
}

fn list(at_time: SyncTime) -> NativeRequest {
    ("thread/turns/list".to_owned(), at_time)
}

fn interrupt(at_time: SyncTime) -> NativeRequest {
    ("turn/interrupt".to_owned(), at_time)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions report violated deadline contracts."
)]
async fn a_summary_read_timeout_uses_the_injected_clock_and_arms_its_retry() -> TestResult<()> {
    let rig = SummaryRig::start(NativeScript {
        held_history: Some(Arc::new(tokio::sync::Notify::new())),
        ..NativeScript::default()
    })
    .await?;
    answered(&rig.requests, 1).await?;
    let deadline = ClockInstant::ZERO.after(Duration::from_secs(20));
    assert!(
        rig.fixture.clock.has_not_due_check_now(deadline),
        "summary must register its injected read deadline"
    );

    advance(&rig.fixture.clock, Duration::from_secs(19)).await;
    rig.fixture.clock.not_due_check_now(deadline).await;
    assert_eq!(rig.requests(), [list(at(Duration::ZERO))]);
    advance(&rig.fixture.clock, Duration::from_secs(1)).await;
    settled(&rig.scheduler).await?;

    assert_eq!(rig.armed(), Some(at(Duration::from_secs(21))));
    assert_eq!(
        rig.fixture.summary_phase(&rig.run).await?,
        Some(SummaryPhase::Running)
    );
    rig.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_running_summary_is_interrupted_once_at_its_deadline() -> TestResult<()> {
    let rig = SummaryRig::start(NativeScript::default()).await?;
    let deadline = at(Duration::from_secs(SUMMARY_SECONDS));

    rig.answered(1).await?;
    let armed = rig.armed();
    advance(&rig.fixture.clock, Duration::from_secs(SUMMARY_SECONDS - 1)).await;
    settled(&rig.scheduler).await?;
    let before_deadline = rig.requests().len();
    advance(&rig.fixture.clock, Duration::from_secs(1)).await;
    rig.answered(4).await?;
    let armed_after = rig.armed();
    advance(&rig.fixture.clock, Duration::from_secs(3_600)).await;
    settled(&rig.scheduler).await?;

    assert_eq!(armed, Some(deadline), "the summary deadline arms the timer");
    assert_eq!(before_deadline, 1, "nothing acts before the deadline");
    assert_eq!(
        rig.requests(),
        [
            list(at(Duration::ZERO)),
            list(deadline),
            interrupt(deadline),
            list(deadline)
        ],
        "one interrupt at the deadline; the stopping summary is only read again"
    );
    assert_eq!(
        rig.fixture.summary_phase(&rig.run).await?,
        Some(SummaryPhase::Stopping)
    );
    assert_eq!(
        armed_after, None,
        "a stopping summary still running arms nothing"
    );
    rig.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_deadline_stop_that_cannot_connect_is_retried_and_sent_once() -> TestResult<()> {
    // The startup read and the deadline read connect; the stop's connection fails.
    let rig = SummaryRig::start(NativeScript {
        dropped_connections: vec![3],
        interrupt_ends_turn: true,
        ..NativeScript::default()
    })
    .await?;
    let deadline = at(Duration::from_secs(SUMMARY_SECONDS));
    let retry = at(Duration::from_secs(SUMMARY_SECONDS + 1));

    rig.answered(1).await?;
    advance(&rig.fixture.clock, Duration::from_secs(SUMMARY_SECONDS)).await;
    rig.answered(2).await?;
    let armed_after_failed_stop = rig.armed();
    let phase_after_failed_stop = rig.fixture.summary_phase(&rig.run).await?;
    advance(&rig.fixture.clock, Duration::from_secs(1)).await;
    rig.answered(5).await?;

    assert_eq!(phase_after_failed_stop, Some(SummaryPhase::Running));
    assert_eq!(
        armed_after_failed_stop,
        Some(retry),
        "the stop that could not connect retries on the backoff"
    );
    assert_eq!(
        rig.requests(),
        [
            list(at(Duration::ZERO)),
            list(deadline),
            list(retry),
            interrupt(retry),
            list(retry)
        ],
        "one interrupt, sent by the retry"
    );
    assert_ne!(
        rig.fixture.read_phase(&rig.run).await?,
        super::RunPhase::SummaryRunning,
        "the interrupted summary turn settles the Run"
    );
    assert_eq!(
        rig.fixture.summary_phase(&rig.run).await?,
        Some(SummaryPhase::Failed)
    );
    rig.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_summary_read_that_cannot_connect_before_its_deadline_is_retried() -> TestResult<()> {
    let rig = SummaryRig::start(NativeScript {
        dropped_connections: vec![1],
        ..NativeScript::default()
    })
    .await?;
    let deadline = at(Duration::from_secs(SUMMARY_SECONDS));

    settled(&rig.scheduler).await?;
    let armed_after_failed_read = rig.armed();
    let before_retry = rig.requests();
    advance(&rig.fixture.clock, Duration::from_secs(1)).await;
    rig.answered(1).await?;
    let armed_after_retry = rig.armed();

    assert!(before_retry.is_empty(), "the first read never connected");
    assert_eq!(
        armed_after_failed_read,
        Some(at(Duration::from_secs(1))),
        "the failed read retries before the deadline"
    );
    assert_eq!(rig.requests(), [list(at(Duration::from_secs(1)))]);
    assert_eq!(
        armed_after_retry,
        Some(deadline),
        "a summary read as running waits quietly for its deadline"
    );
    rig.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_stopped_summary_absent_from_its_history_is_read_again_until_it_settles() -> TestResult<()>
{
    let rig = SummaryRig::start(NativeScript {
        interrupt_ends_turn: true,
        absent_reads_after_interrupt: 1,
        ..NativeScript::default()
    })
    .await?;
    let deadline = at(Duration::from_secs(SUMMARY_SECONDS));
    let retry = at(Duration::from_secs(SUMMARY_SECONDS + 1));

    rig.answered(1).await?;
    advance(&rig.fixture.clock, Duration::from_secs(SUMMARY_SECONDS)).await;
    rig.answered(4).await?;
    let armed_while_absent = rig.armed();
    let phase_while_absent = rig.fixture.summary_phase(&rig.run).await?;
    advance(&rig.fixture.clock, Duration::from_secs(1)).await;
    rig.answered(5).await?;

    assert_eq!(phase_while_absent, Some(SummaryPhase::Stopping));
    assert_eq!(
        armed_while_absent,
        Some(retry),
        "a stopped turn not yet in its history is read again on the backoff"
    );
    assert_eq!(
        rig.requests(),
        [
            list(at(Duration::ZERO)),
            list(deadline),
            interrupt(deadline),
            list(deadline),
            list(retry)
        ],
        "one interrupt; the retry only reads"
    );
    assert_ne!(
        rig.fixture.read_phase(&rig.run).await?,
        super::RunPhase::SummaryRunning,
        "the turn read as interrupted settles the Run"
    );
    rig.stop().await
}
