//! A scheduled Run's turn observation through the real native route: a read that never
//! reached the app-server is an unreachable client, never a running turn.
use super::CodexAppServerScheduledRuns;
use crate::NativeControlBackend;
use agent_automation::{CapturedRunInputs, RunId, RunPhase};
use collaboration_contracts::{
    DeliveryContractError, RunObservationContext, RunSettlement, ScheduledRunExecution,
};
use collaboration_protocol::{CodexGeneration, EndpointRef, SessionRef};
use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
use tokio_tungstenite::tungstenite::Message;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

const SERVICE: &str = "00000000-0000-4000-8000-000000000001";

/// What the fake app-server does with one connection.
#[derive(Clone, Copy)]
enum Answer {
    /// Closes the connection before answering anything.
    Drop,
    /// Answers with the recorded turn in this state.
    Turn(&'static str),
    /// Answers with a history that does not contain the recorded turn.
    NoSuchTurn,
}

async fn serve(listener: tokio::net::UnixListener, answers: Vec<Answer>) -> TestResult {
    for answer in answers {
        let (stream, _) = listener.accept().await?;
        if matches!(answer, Answer::Drop) {
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
        let read: Value = serde_json::from_str(socket.next().await.ok_or("read")??.to_text()?)?;
        socket
            .send(Message::Text(
                json!({"id":read.get("id"),"result":{"thread":{"id":"worker-thread","model":"fixture-model","reasoningEffort":"medium"}}})
                    .to_string()
                    .into(),
            ))
            .await?;
        let page: Value = serde_json::from_str(socket.next().await.ok_or("page")??.to_text()?)?;
        let data = match answer {
            Answer::Turn(status) => json!([{"id":"worker-turn","status":status,"items":[]}]),
            Answer::NoSuchTurn | Answer::Drop => json!([]),
        };
        socket
            .send(Message::Text(
                json!({"id":page.get("id"),"result":{"data":data,"nextCursor":null}})
                    .to_string()
                    .into(),
            ))
            .await?;
    }
    Ok(())
}

fn route(socket: std::path::PathBuf) -> TestResult<(CodexAppServerScheduledRuns, SessionRef)> {
    let target: SessionRef = serde_json::from_value(json!({
        "endpoint":{"serviceId":SERVICE,"endpointId":"codex-local"},
        "sessionId":"worker-thread"
    }))?;
    let generation: CodexGeneration =
        serde_json::from_value(json!({"serviceEpoch":SERVICE,"generation":1}))?;
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
    let schemas = Arc::new(codex_native_integration::NativePayloadSchemas::from_bundle(
        &bundle,
    )?);
    let gate = crate::NativeGenerationGate::default();
    gate.activate(generation, socket, Some(schemas))?;
    let route = CodexAppServerScheduledRuns::new(
        NativeControlBackend {
            endpoint: target.endpoint.clone(),
            gate,
            codex_home: std::env::temp_dir(),
        },
        Arc::new(crate::UnmaterializedThreadHolder::new()),
    );
    Ok((route, target))
}

fn running_context(target: &SessionRef) -> TestResult<RunObservationContext> {
    let generation: CodexGeneration =
        serde_json::from_value(json!({"serviceEpoch":SERVICE,"generation":1}))?;
    let inputs: CapturedRunInputs<SessionRef, EndpointRef> = serde_json::from_value(json!({
        "scheduleChangeId":agent_automation::ChangeId::generate(),
        "instructionRevisionId":agent_automation::RevisionId::generate(),
        "instructionText":"Check work",
        "continuity":{"kind":"none"},
        "executionConfiguration":{
            "destination":{"kind":"ownedThread","target":target,"cwd":"/tmp"},
            "executionTimeoutSeconds":120,"model":"fixture-model","effort":"medium"
        }
    }))?;
    Ok(RunObservationContext {
        run_id: RunId::generate(),
        phase: RunPhase::Executing,
        recorded: serde_json::from_value(json!({
            "kind":"codexAppServer","target":target,"generation":generation,
            "clientUserMessageId":"run-correlation","nativeTurnId":"worker-turn",
            "nativeSubmissionId":null,"allocation":"notRequested","resume":"notRequested",
            "submission":"accepted","cessation":"notApplicable"
        }))?,
        inputs,
    })
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_failed_read_is_an_unreachable_client_and_only_a_read_turn_is_running() -> TestResult {
    let directory = tempfile::Builder::new().prefix("obs-").tempdir_in("/tmp")?;
    let socket = directory.path().join("native.sock");
    let listener = tokio::net::UnixListener::bind(&socket)?;
    let server = tokio::spawn(serve(
        listener,
        vec![
            Answer::Drop,
            Answer::Turn("inProgress"),
            Answer::NoSuchTurn,
            Answer::Turn("completed"),
        ],
    ));
    let (route, target) = route(socket)?;

    let failed_read = route.observe_settlement(running_context(&target)?).await;
    let running = route.observe_settlement(running_context(&target)?).await;
    let absent = route.observe_settlement(running_context(&target)?).await;
    let completed = route.observe_settlement(running_context(&target)?).await;
    server.await??;

    assert!(
        matches!(failed_read, Err(DeliveryContractError::ClientOperation)),
        "a read that reached no app-server is not a running turn: {failed_read:?}"
    );
    assert!(matches!(running, Ok(RunSettlement::Pending)), "{running:?}");
    assert!(
        matches!(absent, Ok(RunSettlement::Pending)),
        "an observed absence stays pending; the deadline still acts: {absent:?}"
    );
    assert!(
        matches!(completed, Ok(RunSettlement::Completed { .. })),
        "{completed:?}"
    );
    Ok(())
}
