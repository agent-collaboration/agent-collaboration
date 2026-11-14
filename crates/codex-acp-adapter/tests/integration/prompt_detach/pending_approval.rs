//! A native approval waiting on its approver outlives the frontend detaching:
//! its decision still reaches the native callback (R6). Cancel and backend
//! retirement still end the wait, with the turn interrupted or the connection
//! gone, never a callback left unanswered on a live turn.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
)]

use super::{
    AcceptingConversationRecorder, EmptyCatalog, TEST_SCRATCH, TestBindingHolder, fixture_schemas,
    read_native_request, reply_native,
};
use codex_acp_adapter::{
    AcpConnectionInputs, ApprovalBroker, ApprovalBrokerError, ApprovalRoute,
    BrokeredApprovalOutcome, BrokeredApprovalRequest, serve_acp_connection,
};
use futures_util::SinkExt;
use serde_json::{Value, json};
use std::{future::Future, os::unix::fs::PermissionsExt, pin::Pin, sync::Arc, sync::Mutex};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::oneshot,
};
use tokio_tungstenite::tungstenite::Message;

/// An approver the test answers by hand. `requested` fires when the Router
/// asks; the wait holds `decision`, so the test sees the wait dropped when
/// that channel closes.
struct HandAnsweredApprovals {
    requested: Mutex<Option<oneshot::Sender<()>>>,
    decision: Mutex<Option<oneshot::Receiver<BrokeredApprovalOutcome>>>,
}

impl HandAnsweredApprovals {
    fn new() -> (
        Arc<Self>,
        oneshot::Receiver<()>,
        oneshot::Sender<BrokeredApprovalOutcome>,
    ) {
        let (requested_tx, requested_rx) = oneshot::channel();
        let (decision_tx, decision_rx) = oneshot::channel();
        (
            Arc::new(Self {
                requested: Mutex::new(Some(requested_tx)),
                decision: Mutex::new(Some(decision_rx)),
            }),
            requested_rx,
            decision_tx,
        )
    }
}

impl ApprovalBroker for HandAnsweredApprovals {
    fn register_route(
        &self,
        _route: ApprovalRoute,
    ) -> Pin<Box<dyn Future<Output = Result<(), ApprovalBrokerError>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }

    fn request(
        &self,
        _request: BrokeredApprovalRequest,
    ) -> Pin<
        Box<dyn Future<Output = Result<BrokeredApprovalOutcome, ApprovalBrokerError>> + Send + '_>,
    > {
        let requested = self.requested.lock().ok().and_then(|mut slot| slot.take());
        let decision = self.decision.lock().ok().and_then(|mut slot| slot.take());
        Box::pin(async move {
            if let Some(requested) = requested {
                let _sent = requested.send(());
            }
            decision
                .ok_or(ApprovalBrokerError::Unavailable)?
                .await
                .map_err(|_| ApprovalBrokerError::Unavailable)
        })
    }

    fn route(
        &self,
        _thread_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<ApprovalRoute>, ApprovalBrokerError>> + Send + '_>>
    {
        Box::pin(async { Ok(None) })
    }
}

/// How the pending approval ends in each case.
#[derive(Clone, Copy, Debug)]
enum WhilePending {
    FrontendDetaches,
    FrontendCancels,
    BackendRetires,
}

async fn send_frontend(
    write: &mut tokio::net::unix::OwnedWriteHalf,
    id: &str,
    method: &str,
    params: Value,
) {
    write
        .write_all(
            format!(
                "{}\n",
                json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
            )
            .as_bytes(),
        )
        .await
        .unwrap();
}

async fn read_frontend(read: &mut BufReader<tokio::net::unix::OwnedReadHalf>) -> Value {
    let mut line = String::new();
    read.read_line(&mut line).await.unwrap();
    serde_json::from_str(&line).unwrap()
}

async fn approval_pending_then(case: WhilePending) {
    std::fs::create_dir_all(TEST_SCRATCH).unwrap();
    std::fs::set_permissions(TEST_SCRATCH, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = std::env::temp_dir().join(format!(
        "pending-approval-{case:?}-{}.sock",
        std::process::id()
    ));
    let _stale = std::fs::remove_file(&socket);
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let (approval_sent_tx, approval_sent_rx) = oneshot::channel();
    let backend = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        let mut wire = tokio_tungstenite::accept_async(stream).await?;
        for method in ["initialize", "initialized", "thread/resume"] {
            let request = read_native_request(&mut wire).await;
            assert_eq!(request["method"], method);
            if method == "initialized" {
                continue;
            }
            let result = if method == "initialize" {
                json!({})
            } else {
                json!({"cwd":"/work","model":"gpt-5.6-sol","thread":{"id":"thread-approval","cwd":"/work","status":{"type":"idle"},"turns":[]}})
            };
            reply_native(&mut wire, &request, result).await;
        }
        let activity = read_native_request(&mut wire).await;
        assert_eq!(activity["method"], "thread/read");
        reply_native(
            &mut wire,
            &activity,
            json!({"thread":{"id":"thread-approval","status":{"type":"idle"},"turns":[]}}),
        )
        .await;
        let start = read_native_request(&mut wire).await;
        assert_eq!(start["method"], "turn/start");
        reply_native(
            &mut wire,
            &start,
            json!({"turn":{"id":"turn-approval","status":"inProgress"}}),
        )
        .await;
        wire.send(Message::Text(json!({"id":901,"method":"item/commandExecution/requestApproval","params":{"threadId":"thread-approval","turnId":"turn-approval","itemId":"tool-approval","command":"touch /tmp/outside","availableDecisions":["accept","decline"]}}).to_string().into())).await?;
        let _sent = approval_sent_tx.send(());
        match case {
            WhilePending::FrontendDetaches => {
                // Assert: the decision made after the detach reaches native.
                let answer = read_native_request(&mut wire).await;
                assert_eq!(answer, json!({"id":901,"result":{"decision":"accept"}}));
                wire.send(Message::Text(json!({"method":"turn/completed","params":{"threadId":"thread-approval","turn":{"id":"turn-approval","status":"completed"}}}).to_string().into())).await?;
                let read = read_native_request(&mut wire).await;
                assert_eq!(read["method"], "thread/read");
                reply_native(&mut wire, &read, json!({"thread":{"id":"thread-approval","model":"gpt-5.6-sol","reasoningEffort":"medium","createdAt":1,"updatedAt":1}})).await;
            }
            WhilePending::FrontendCancels => {
                // Assert: Cancel interrupts the turn, which ends the native request.
                let interrupt = read_native_request(&mut wire).await;
                assert_eq!(interrupt["method"], "turn/interrupt");
                assert_eq!(
                    interrupt["params"],
                    json!({"threadId":"thread-approval","turnId":"turn-approval"})
                );
                reply_native(&mut wire, &interrupt, json!({})).await;
                wire.send(Message::Text(json!({"method":"turn/completed","params":{"threadId":"thread-approval","turn":{"id":"turn-approval","status":"interrupted"}}}).to_string().into())).await?;
            }
            WhilePending::BackendRetires => {}
        }
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    });

    let (approvals, requested, mut decision) = HandAnsweredApprovals::new();
    let retired = tokio_util::sync::CancellationToken::new();
    let inputs = AcpConnectionInputs {
        scratch_scopes: Arc::new(
            crate::recording_scratch_scopes::RecordingScratchScopes::default(),
        ),
        backend_path: socket.clone(),
        generation: serde_json::from_value(
            json!({"serviceEpoch":"00000000-0000-4000-8000-000000000001","generation":1}),
        )
        .unwrap(),
        schemas: fixture_schemas(),
        stored_sessions: Arc::new(EmptyCatalog),
        approval_broker: approvals,
        holder: Arc::new(TestBindingHolder::default()),
        recorder: Arc::new(AcceptingConversationRecorder),
        retired: retired.clone(),
    };
    let (client, server) = tokio::net::UnixStream::pair().unwrap();
    let serving = tokio::spawn(serve_acp_connection(server, inputs));
    let (read, mut write) = client.into_split();
    let mut read = BufReader::new(read);
    send_frontend(
        &mut write,
        "initialize",
        "initialize",
        json!({"protocolVersion":1}),
    )
    .await;
    assert!(read_frontend(&mut read).await.get("result").is_some());
    send_frontend(
        &mut write,
        "load",
        "session/load",
        json!({"sessionId":"thread-approval","cwd":"/work","mcpServers":[]}),
    )
    .await;
    let loaded = read_frontend(&mut read).await;
    assert!(loaded.get("result").is_some(), "load failed: {loaded}");
    send_frontend(
        &mut write,
        "prompt",
        "session/prompt",
        json!({"sessionId":"thread-approval","prompt":[{"type":"text","text":"work"}]}),
    )
    .await;
    approval_sent_rx.await.unwrap();
    requested.await.unwrap();

    match case {
        WhilePending::FrontendDetaches => {
            // Act: the frontend leaves while the approver has not decided.
            write.shutdown().await.unwrap();
            drop(write);
            drop(read);
            // Let the prompt actor see its closed command channel; on this
            // single-threaded runtime it runs before the test resumes.
            for _ in 0..16 {
                tokio::task::yield_now().await;
            }
            // Assert: the approval wait survived the detach.
            assert!(
                !decision.is_closed(),
                "the frontend detaching dropped the pending approval wait"
            );
            decision
                .send(BrokeredApprovalOutcome::Selected {
                    option_id: "native-accept".to_owned(),
                })
                .unwrap();
        }
        WhilePending::FrontendCancels => {
            // Act: the frontend cancels the turn while the approval waits.
            // `session/cancel` is an ACP notification: it carries no id.
            write
                .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"session/cancel\",\"params\":{\"sessionId\":\"thread-approval\"}}\n")
                .await
                .unwrap();
            // Assert: the wait ends (its approval is recorded cancelled).
            decision.closed().await;
            // Assert: the prompt answers cancelled once native confirmed the interrupt.
            let cancelled = read_frontend(&mut read).await;
            assert_eq!(cancelled["id"], "prompt", "{cancelled}");
            assert_eq!(
                cancelled["result"]["stopReason"], "cancelled",
                "{cancelled}"
            );
            write.shutdown().await.unwrap();
        }
        WhilePending::BackendRetires => {
            // Act: the native backend binding retires.
            retired.cancel();
            // Assert: the wait ends with the retired connection.
            decision.closed().await;
            // The retired connection may already have closed the frontend socket.
            let _closed = write.shutdown().await;
        }
    }
    serving.await.unwrap().unwrap();
    backend.await.unwrap().unwrap();
    std::fs::remove_file(socket).unwrap();
}

/// Oracle: R6 (an action is refused only after its approval ends) and the
/// Lead's repair commission: a decision made after the frontend detached
/// still reaches the native callback and the turn completes.
#[tokio::test]
async fn approval_decided_after_frontend_detach_reaches_native() {
    approval_pending_then(WhilePending::FrontendDetaches).await;
}

/// Oracle: Cancel still ends a pending approval and interrupts the turn.
#[tokio::test]
async fn cancel_while_approval_pending_ends_the_wait_and_interrupts() {
    approval_pending_then(WhilePending::FrontendCancels).await;
}

/// Oracle: backend retirement still ends a pending approval.
#[tokio::test]
async fn retirement_while_approval_pending_ends_the_wait() {
    approval_pending_then(WhilePending::BackendRetires).await;
}
