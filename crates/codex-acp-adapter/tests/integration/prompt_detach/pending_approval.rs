//! A native approval in flight outlives the frontend detaching: its decision,
//! whether still awaited or already on its way to native, reaches the native
//! callback (R6). Cancel and backend retirement still end the wait.
//!
//! Each case drives the prompt actor directly and owns its command channel, so
//! the test, not the scheduler, decides when the frontend detaches. The actor
//! checks its commands before its native step on every poll, so a detach made
//! before the actor next runs is always seen first: on the defective build it
//! drops the in-flight step whatever the interleaving.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
)]

use super::{
    AcceptingConversationRecorder, TEST_SCRATCH, fixture_schemas, read_native_request, reply_native,
};
use codex_acp_adapter::{
    AcpSchemaCatalog, AcpSessionBinding, ApprovalBroker, ApprovalBrokerError, ApprovalRoute,
    BrokeredApprovalOutcome, BrokeredApprovalRequest, PromptCommand, PromptTaskInputs,
    SessionSetupInputs, run_prompt_task,
};
use codex_native_integration::NativeProtocolConnection;
use futures_util::SinkExt;
use serde_json::{Value, json};
use std::{future::Future, os::unix::fs::PermissionsExt, pin::Pin, sync::Arc, sync::Mutex};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, protocol::Role},
};

/// An approver the test answers by hand. `requested` fires when the Router
/// asks; the wait holds `decision`, so its closing shows the wait was dropped.
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

/// What happens while the approval is in flight.
#[derive(Clone, Copy, Debug)]
enum WhilePending {
    /// The frontend detaches, then the approver decides.
    DetachThenDecide,
    /// The approver decides and the frontend detaches before the actor writes
    /// that decision to native.
    DecideThenDetach,
    FrontendCancels,
    BackendRetires,
}

const THREAD: &str = "thread-approval";
const TURN: &str = "turn-approval";

/// The native peer: resume, a turn, one approval request, then the ending
/// each case expects.
async fn native_peer(
    server: tokio::net::UnixStream,
    case: WhilePending,
    approval_sent: oneshot::Sender<()>,
) {
    let mut wire = WebSocketStream::from_raw_socket(server, Role::Server, None).await;
    let resume = read_native_request(&mut wire).await;
    assert_eq!(resume["method"], "thread/resume");
    reply_native(
        &mut wire,
        &resume,
        json!({"cwd":"/work","model":"gpt-5.6-sol","thread":{"id":THREAD,"cwd":"/work","status":{"type":"idle"},"turns":[]}}),
    )
    .await;
    let activity = read_native_request(&mut wire).await;
    assert_eq!(activity["method"], "thread/read");
    reply_native(
        &mut wire,
        &activity,
        json!({"thread":{"id":THREAD,"status":{"type":"idle"},"turns":[]}}),
    )
    .await;
    let start = read_native_request(&mut wire).await;
    assert_eq!(start["method"], "turn/start");
    reply_native(
        &mut wire,
        &start,
        json!({"turn":{"id":TURN,"status":"inProgress"}}),
    )
    .await;
    wire.send(Message::Text(json!({"id":901,"method":"item/commandExecution/requestApproval","params":{"threadId":THREAD,"turnId":TURN,"itemId":"tool-approval","command":"touch /tmp/outside","availableDecisions":["accept","decline"]}}).to_string().into())).await.unwrap();
    let _sent = approval_sent.send(());
    match case {
        WhilePending::DetachThenDecide | WhilePending::DecideThenDetach => {
            // Assert: exactly the approver's decision reaches native.
            let answer = read_native_request(&mut wire).await;
            assert_eq!(answer, json!({"id":901,"result":{"decision":"accept"}}));
            wire.send(Message::Text(json!({"method":"turn/completed","params":{"threadId":THREAD,"turn":{"id":TURN,"status":"completed"}}}).to_string().into())).await.unwrap();
            let read = read_native_request(&mut wire).await;
            assert_eq!(read["method"], "thread/read");
            reply_native(&mut wire, &read, json!({"thread":{"id":THREAD,"model":"gpt-5.6-sol","reasoningEffort":"medium","createdAt":1,"updatedAt":1}})).await;
        }
        WhilePending::FrontendCancels => {
            // Assert: Cancel interrupts the turn, which ends the native request.
            let interrupt = read_native_request(&mut wire).await;
            assert_eq!(interrupt["method"], "turn/interrupt");
            assert_eq!(
                interrupt["params"],
                json!({"threadId":THREAD,"turnId":TURN})
            );
            reply_native(&mut wire, &interrupt, json!({})).await;
        }
        WhilePending::BackendRetires => {}
    }
}

async fn approval_in_flight_then(case: WhilePending) -> Option<Value> {
    std::fs::create_dir_all(TEST_SCRATCH).unwrap();
    std::fs::set_permissions(TEST_SCRATCH, std::fs::Permissions::from_mode(0o700)).unwrap();
    let (client, server) = tokio::net::UnixStream::pair().unwrap();
    let (approval_sent_tx, approval_sent_rx) = oneshot::channel();
    let backend = tokio::spawn(native_peer(server, case, approval_sent_tx));
    let (approvals, requested, mut decision) = HandAnsweredApprovals::new();
    let mut catalog = AcpSchemaCatalog::load().unwrap();
    let session = AcpSessionBinding::load_existing(
        &mut catalog,
        SessionSetupInputs {
            scratch_scopes: Arc::new(
                crate::recording_scratch_scopes::RecordingScratchScopes::default(),
            ),
            operation_id: None,
            recorder: Arc::new(AcceptingConversationRecorder),
            connection: NativeProtocolConnection::from_websocket(
                WebSocketStream::from_raw_socket(client, Role::Client, None).await,
            ),
            schemas: fixture_schemas(),
            generation: serde_json::from_value(
                json!({"serviceEpoch":"00000000-0000-4000-8000-000000000001","generation":1}),
            )
            .unwrap(),
            params: json!({"sessionId":THREAD,"cwd":"/work","mcpServers":[]}),
            approval_broker: approvals,
        },
    )
    .await
    .map(|(session, _history)| session)
    .unwrap();
    let closed = tokio_util::sync::CancellationToken::new();
    let (output, frames) = codex_acp_adapter::bounded_acp_output(closed.clone());
    drop(frames);
    let (commands, receiver) = mpsc::channel(1);
    let retired = tokio_util::sync::CancellationToken::new();
    let prompt = tokio::spawn(run_prompt_task(PromptTaskInputs {
        session,
        request_id: json!("prompt-approval"),
        params: json!({"sessionId":THREAD,"prompt":[{"type":"text","text":"work"}]}),
        commands: receiver,
        output,
        retired: retired.clone(),
    }));
    approval_sent_rx.await.unwrap();
    requested.await.unwrap();

    match case {
        WhilePending::DetachThenDecide => {
            // Act: the frontend detaches (the registry drops the command
            // sender), then the approver decides.
            drop(commands);
            closed.cancel();
            let _sent = decision.send(BrokeredApprovalOutcome::Selected {
                option_id: "native-accept".to_owned(),
            });
        }
        WhilePending::DecideThenDetach => {
            // Act: the decision arrives, and the frontend detaches before the
            // actor runs again to write it to native.
            let _sent = decision.send(BrokeredApprovalOutcome::Selected {
                option_id: "native-accept".to_owned(),
            });
            drop(commands);
            closed.cancel();
        }
        WhilePending::FrontendCancels => {
            // Act: the frontend cancels the turn while the approval waits.
            commands.send(PromptCommand::Cancel).await.unwrap();
            // Assert: the wait ends (its approval is recorded cancelled).
            decision.closed().await;
            drop(commands);
        }
        WhilePending::BackendRetires => {
            // Act: the native backend binding retires.
            retired.cancel();
            // Assert: the wait ends with the retired connection.
            decision.closed().await;
            drop(commands);
        }
    }
    let completion = prompt.await.unwrap();
    backend.await.unwrap();
    completion.terminal
}

/// Oracle: R6 (an action is refused only after its approval ends) and the
/// Lead's repair commission: a decision made after the frontend detached
/// reaches native exactly once and the turn completes.
#[tokio::test]
async fn approval_decided_after_frontend_detach_reaches_native() {
    let terminal = approval_in_flight_then(WhilePending::DetachThenDecide).await;
    assert!(terminal.is_none(), "a detached prompt sends no terminal");
}

/// Oracle: as above, for the decision-to-write step: a decision that arrived
/// just before the detach is still written to native, once.
#[tokio::test]
async fn decision_arriving_with_the_detach_is_still_written_to_native() {
    let terminal = approval_in_flight_then(WhilePending::DecideThenDetach).await;
    assert!(terminal.is_none(), "a detached prompt sends no terminal");
}

/// Oracle: Cancel still ends a pending approval and interrupts the turn.
#[tokio::test]
async fn cancel_while_approval_pending_ends_the_wait_and_interrupts() {
    let terminal = approval_in_flight_then(WhilePending::FrontendCancels)
        .await
        .unwrap();
    assert_eq!(terminal["id"], "prompt-approval", "{terminal}");
    assert_eq!(terminal["result"]["stopReason"], "cancelled", "{terminal}");
}

/// Oracle: backend retirement still ends a pending approval.
#[tokio::test]
async fn retirement_while_approval_pending_ends_the_wait() {
    let terminal = approval_in_flight_then(WhilePending::BackendRetires)
        .await
        .unwrap();
    assert_eq!(
        terminal["error"]["data"]["detail"], "Native backend connection lost",
        "{terminal}"
    );
}
