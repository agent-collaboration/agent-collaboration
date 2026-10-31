//! Native control-request refusal and recovery; no provider or model is launched.
use super::test_support::{ScriptedSessionBackend, next_non_skill_frame};
use super::*;
use collaboration_contracts::CommandFailure;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_tungstenite::{WebSocketStream, client_async};

const BOUND: Duration = Duration::from_secs(5);
type NativeClient = WebSocketStream<UnixStream>;
type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

async fn connect(
    backend: &Arc<ScriptedSessionBackend>,
) -> TestResult<(
    NativeClient,
    JoinHandle<Result<(), AppServerConnectionError>>,
)> {
    let context = Arc::new(RouterSessionAppServerContext::new(
        backend.endpoint.clone(),
        ScriptedSessionBackend::actor()?,
        Arc::clone(backend) as Arc<dyn SessionCommandPort>,
        Arc::clone(backend) as Arc<dyn SessionEventHub>,
        super::test_support::model_catalog(tokio::sync::watch::channel(Vec::new()).1),
    ));
    let (client, server) = UnixStream::pair()?;
    let task = tokio::spawn(serve_router_session_app_server_connection(server, context));
    let (client, _) = client_async("ws://localhost/rpc", client).await?;
    Ok((client, task))
}

async fn send(client: &mut NativeClient, id: Value, method: &str, params: Value) -> TestResult<()> {
    client
        .send(Message::Text(
            json!({"id":id,"method":method,"params":params})
                .to_string()
                .into(),
        ))
        .await?;
    Ok(())
}

async fn frame(client: &mut NativeClient) -> TestResult<Value> {
    let message = tokio::time::timeout(BOUND, next_non_skill_frame(client))
        .await?
        .ok_or("missing native frame")??;
    Ok(serde_json::from_str(message.to_text()?)?)
}

async fn start(client: &mut NativeClient) -> TestResult<String> {
    send(
        client,
        json!(1),
        "thread/start",
        json!({"cwd":std::env::current_dir()?}),
    )
    .await?;
    frame(client).await?["result"]["thread"]["id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "missing native thread".into())
}

async fn history(client: &mut NativeClient, thread: &str) -> TestResult<Value> {
    send(
        client,
        json!("read"),
        "thread/read",
        json!({"threadId":thread,"includeTurns":true}),
    )
    .await?;
    let response = frame(client).await?;
    assert_eq!(response["id"], "read");
    assert!(response.get("error").is_none(), "{response}");
    Ok(response["result"]["thread"]["turns"].clone())
}

/// Oracle: pinned Codex0.165 thread_processor:2526-2539, notification:62-69,
/// chatwidget/protocol:206-235 and buffered_replay:777-787: ACK is control
/// admission, nonretry error is recoverable, submission identity is not a Turn.
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Wire assertions; fixture errors propagate."
)]
async fn unsupported_compact_ack_precedes_recoverable_error_without_a_synthetic_turn()
-> TestResult<()> {
    let backend = ScriptedSessionBackend::new()?;
    *backend.compact_failure.lock().expect("test lock") = Some(CommandFailure::Unsupported);
    let (mut client, task) = connect(&backend).await?;
    let thread = start(&mut client).await?;
    let before = history(&mut client, &thread).await?;

    for request_id in [json!(71), json!("71")] {
        send(
            &mut client,
            request_id.clone(),
            "thread/compact/start",
            json!({"threadId":thread}),
        )
        .await?;
        let ack = frame(&mut client).await?;
        assert_eq!(ack, json!({"id":request_id,"result":{}}));
        let error = frame(&mut client).await?;
        let expected_submission = if request_id.is_string() {
            "compact-request:\"71\""
        } else {
            "compact-request:71"
        };
        assert_eq!(
            error,
            json!({"method":"error","params":{
                "threadId":thread,"turnId":expected_submission,"willRetry":false,
                "error":{"message":"Compaction is unsupported for this provider session","codexErrorInfo":null}
            }})
        );
        assert_eq!(history(&mut client, &thread).await?, before);
        assert!(backend.history.lock().expect("test lock").is_empty());
        assert!(
            backend
                .prompt_commands
                .lock()
                .expect("test lock")
                .is_empty()
        );
    }
    assert_eq!(backend.compact_commands.lock().expect("test lock").len(), 2);

    send(
        &mut client,
        json!("next-prompt"),
        "turn/start",
        json!({
            "threadId":thread,"input":[{"type":"text","text":"still usable"}]
        }),
    )
    .await?;
    // Ordinary turn events and their RPC response use independent existing queues.
    let prompt = tokio::time::timeout(BOUND, async {
        loop {
            let received = frame(&mut client).await?;
            if received.get("id").is_some() {
                return Ok::<_, Box<dyn std::error::Error>>(received);
            }
            assert_ne!(received["method"], "error");
        }
    })
    .await??;
    assert_eq!(prompt["id"], "next-prompt");
    assert_eq!(prompt["result"]["turn"]["status"], "inProgress");
    assert_eq!(backend.prompt_commands.lock().expect("test lock").len(), 1);
    client.close(None).await?;
    tokio::time::timeout(BOUND, task).await???;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Wire assertions; fixture errors propagate."
)]
async fn supported_compact_keeps_the_existing_ack_and_has_no_refusal_notification() -> TestResult<()>
{
    let backend = ScriptedSessionBackend::new()?;
    let (mut client, task) = connect(&backend).await?;
    let thread = start(&mut client).await?;
    send(
        &mut client,
        json!(2),
        "thread/compact/start",
        json!({"threadId":thread}),
    )
    .await?;
    assert_eq!(frame(&mut client).await?, json!({"id":2,"result":{}}));
    assert_eq!(history(&mut client, &thread).await?, json!([]));
    assert_eq!(backend.compact_commands.lock().expect("test lock").len(), 1);
    assert!(
        backend
            .prompt_commands
            .lock()
            .expect("test lock")
            .is_empty()
    );
    client.close(None).await?;
    tokio::time::timeout(BOUND, task).await???;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Wire assertions; fixture errors propagate."
)]
async fn other_compact_failures_keep_rpc_errors_and_emit_no_control_notification() -> TestResult<()>
{
    for (failure, code) in [
        (CommandFailure::UnauthorizedActor, -32000),
        (CommandFailure::ProviderFailure, -32000),
        (CommandFailure::Busy, -32600),
        (CommandFailure::SessionNotFound, -32002),
    ] {
        let backend = ScriptedSessionBackend::new()?;
        *backend.compact_failure.lock().expect("test lock") = Some(failure);
        let (mut client, task) = connect(&backend).await?;
        let thread = start(&mut client).await?;
        send(
            &mut client,
            json!(2),
            "thread/compact/start",
            json!({"threadId":thread}),
        )
        .await?;
        let response = frame(&mut client).await?;
        assert_eq!(response["id"], 2);
        assert_eq!(response["error"]["code"], code);
        assert!(response.get("result").is_none());
        assert_eq!(history(&mut client, &thread).await?, json!([]));
        assert!(backend.history.lock().expect("test lock").is_empty());
        client.close(None).await?;
        tokio::time::timeout(BOUND, task).await???;
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Wire assertions; fixture errors propagate."
)]
async fn invalid_or_missing_compact_target_is_refused_before_command_dispatch() -> TestResult<()> {
    let backend = ScriptedSessionBackend::new()?;
    *backend.compact_failure.lock().expect("test lock") = Some(CommandFailure::Unsupported);
    let (mut client, task) = connect(&backend).await?;
    let thread = start(&mut client).await?;
    for (params, code) in [(json!({}), -32602), (json!({"threadId":"missing"}), -32002)] {
        send(&mut client, json!(2), "thread/compact/start", params).await?;
        let response = frame(&mut client).await?;
        assert_eq!(response["error"]["code"], code);
        assert!(response.get("result").is_none());
        assert_eq!(history(&mut client, &thread).await?, json!([]));
    }
    assert!(
        backend
            .compact_commands
            .lock()
            .expect("test lock")
            .is_empty()
    );
    client.close(None).await?;
    tokio::time::timeout(BOUND, task).await???;
    Ok(())
}
