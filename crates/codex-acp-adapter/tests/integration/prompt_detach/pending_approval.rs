//! Native refusal responds on the callback before ending the exact turn.

use super::{AcceptingConversationRecorder, fixture_schemas, read_native_request, reply_native};
use codex_acp_adapter::{
    AcpSchemaCatalog, AcpSessionBinding, PromptTaskInputs, SessionSetupInputs, run_prompt_task,
};
use codex_native_integration::NativeProtocolConnection;
use futures_util::SinkExt as _;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, protocol::Role},
};
use tokio_util::sync::CancellationToken;

const THREAD: &str = "thread-approval";

#[expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "Any malformed local fixture or reply fails the scripted wire proof."
)]
async fn exercise_refusal(
    method: &'static str,
    expected: Value,
    detached: bool,
    interrupt_rejected: bool,
) {
    let (client, server) = tokio::net::UnixStream::pair().unwrap();
    let backend = tokio::spawn(async move {
        let mut wire = WebSocketStream::from_raw_socket(server, Role::Server, None).await;
        let resume = read_native_request(&mut wire).await;
        assert_eq!(resume["method"], "thread/resume");
        reply_native(&mut wire, &resume, json!({
            "cwd":"/work", "thread":{"id":THREAD,"cwd":"/work","status":{"type":"idle"},"turns":[]}
        })).await;
        for turn_id in ["refused-turn", "next-turn"] {
            let activity = read_native_request(&mut wire).await;
            assert_eq!(activity["method"], "thread/read");
            reply_native(
                &mut wire,
                &activity,
                json!({"thread":{"id":THREAD,"status":{"type":"idle"}}}),
            )
            .await;
            let start = read_native_request(&mut wire).await;
            assert_eq!(start["method"], "turn/start");
            reply_native(&mut wire, &start, json!({"turn":{"id":turn_id}})).await;
            if turn_id == "refused-turn" {
                wire.send(Message::Text(
                    json!({"id":901,"method":method,"params":{
                        "threadId":THREAD,"turnId":turn_id,"itemId":"fixture"
                    }})
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
                assert_eq!(
                    read_native_request(&mut wire).await,
                    json!({"id":901,"result":expected})
                );
                let interrupt = read_native_request(&mut wire).await;
                assert_eq!(interrupt["method"], "turn/interrupt");
                assert_eq!(
                    interrupt["params"],
                    json!({"threadId":THREAD,"turnId":turn_id})
                );
                wire.send(Message::Text(
                    json!({"method":"turn/completed","params":{
                        "threadId":THREAD,"turn":{"id":turn_id,"status":"interrupted"}
                    }})
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
                if interrupt_rejected {
                    wire.send(Message::Text(json!({
                        "id":interrupt["id"],"error":{"code":-32600,"message":"no active turn to interrupt"}
                    }).to_string().into())).await.unwrap();
                    let read = read_native_request(&mut wire).await;
                    assert_eq!(read["method"], "thread/read");
                    assert_eq!(
                        read["params"],
                        json!({"threadId":THREAD,"includeTurns":true})
                    );
                    reply_native(
                        &mut wire,
                        &read,
                        json!({"thread":{
                            "id":THREAD,"status":{"type":"idle"},
                            "turns":[{"id":turn_id,"status":"interrupted"}]
                        }}),
                    )
                    .await;
                } else {
                    reply_native(&mut wire, &interrupt, json!({})).await;
                }
            } else {
                wire.send(Message::Text(
                    json!({"method":"turn/completed","params":{
                        "threadId":THREAD,"turn":{"id":turn_id,"status":"completed"}
                    }})
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
                let receipt = read_native_request(&mut wire).await;
                assert_eq!(receipt["method"], "thread/read");
                reply_native(&mut wire, &receipt, json!({"thread":{
                    "id":THREAD,"model":"fixture","reasoningEffort":"medium","createdAt":1,"updatedAt":1
                }})).await;
            }
        }
    });
    let scratch_scopes =
        Arc::new(crate::recording_scratch_scopes::RecordingScratchScopes::default());
    let (mut session, _) = AcpSessionBinding::load_existing(
        &mut AcpSchemaCatalog::load().unwrap(),
        SessionSetupInputs {
            scratch_scopes: scratch_scopes.clone(),
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
        },
    )
    .await
    .unwrap();
    for first in [true, false] {
        let (output, _frames) = codex_acp_adapter::bounded_acp_output(CancellationToken::new());
        let (commands, receiver) = tokio::sync::mpsc::channel(1);
        let commands = if first && detached {
            drop(commands);
            None
        } else {
            Some(commands)
        };
        let completion = run_prompt_task(PromptTaskInputs {
            session,
            request_id: json!("prompt"),
            params: json!({"sessionId":THREAD,"prompt":[{"type":"text","text":"fixture"}]}),
            commands: receiver,
            output,
            retired: CancellationToken::new(),
        })
        .await;
        if first && detached {
            assert!(completion.terminal.is_none());
        } else {
            let terminal = completion.terminal.expect("terminal prompt result");
            if first {
                assert_eq!(terminal["result"]["stopReason"], "cancelled");
                assert_eq!(
                    terminal["result"]["_meta"]["codexRouter"]["reasonCode"],
                    "approvalsUnavailableOnBuild"
                );
                assert_eq!(
                    terminal["result"]["_meta"]["codexRouter"]["reason"],
                    "approvals are not available on this build"
                );
            } else {
                assert_eq!(terminal["result"]["stopReason"], "end_turn");
            }
        }
        assert!(completion.cancellation_barrier.is_none());
        session = completion
            .binding
            .expect("the same session can prompt again");
        drop(commands);
    }
    assert!(scratch_scopes.events().is_empty());
    backend.await.unwrap();
}

#[tokio::test]
async fn every_managed_native_callback_refuses_then_allows_the_next_prompt() {
    for (method, response) in [
        (
            "item/commandExecution/requestApproval",
            json!({"decision":"cancel"}),
        ),
        (
            "item/fileChange/requestApproval",
            json!({"decision":"cancel"}),
        ),
        (
            "item/permissions/requestApproval",
            json!({"permissions":{},"scope":"turn"}),
        ),
        ("item/tool/requestUserInput", json!({"answers":{}})),
    ] {
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            exercise_refusal(method, response, false, false),
        )
        .await
        .expect("refusal and next prompt settle");
    }
}

#[tokio::test]
async fn native_refusal_still_reaches_the_provider_after_frontend_detach() {
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        exercise_refusal(
            "item/commandExecution/requestApproval",
            json!({"decision":"cancel"}),
            true,
            false,
        ),
    )
    .await
    .expect("detached refusal settles");
}

#[tokio::test]
async fn native_refusal_releases_the_binding_when_the_turn_ends_before_interrupt() {
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        exercise_refusal(
            "item/fileChange/requestApproval",
            json!({"decision":"cancel"}),
            false,
            true,
        ),
    )
    .await
    .expect("terminal native evidence settles the refused turn");
}
