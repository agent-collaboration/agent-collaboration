//! Client confirmation through the real websocket dispatch and event loop.
use super::test_support::ScriptedSessionBackend;
use super::*;
use collaboration_contracts::CommandFailure;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::Ordering;
use tokio::time::{Duration, timeout};
use tokio_tungstenite::{WebSocketStream, client_async};

async fn next_frame(client: &mut WebSocketStream<UnixStream>) -> Value {
    let message = timeout(Duration::from_secs(5), client.next())
        .await
        .expect("frame deadline")
        .expect("open websocket")
        .expect("frame");
    serde_json::from_str(message.to_text().expect("text frame")).expect("JSON frame")
}

async fn send_request(
    client: &mut WebSocketStream<UnixStream>,
    id: u64,
    method: &str,
    params: Value,
) {
    client
        .send(Message::Text(
            json!({"id":id,"method":method,"params":params})
                .to_string()
                .into(),
        ))
        .await
        .expect("send request");
}

/// Oracle: the CLI commits pending input by clientId, including identical text.
/// The scripted port models the actor's ordered receipts; the ACP integration
/// regression separately checks that the actual actor produces those events.
#[tokio::test]
async fn native_user_receipts_confirm_start_and_injected_steer_but_not_refusal() {
    // Arrange: attach to a session; keep its original turn open until interrupt.
    let root = tempfile::tempdir().expect("socket root");
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private socket directory");
    let socket = root.path().join("provider.sock");
    let backend = ScriptedSessionBackend::new().expect("backend");
    backend.publish_user_receipts.store(true, Ordering::Release);
    let context = Arc::new(RouterSessionAppServerContext::new(
        backend.endpoint.clone(),
        ScriptedSessionBackend::actor().expect("actor"),
        backend.clone(),
        backend.clone(),
        super::test_support::model_catalog(tokio::sync::watch::channel(Vec::new()).1),
    ));
    let listener = RouterSessionAppServerListener::bind(&socket, context).expect("bind");
    let shutdown = CancellationToken::new();
    let server = tokio::spawn(listener.run(shutdown.clone()));
    let (mut client, _) = client_async(
        "ws://localhost/rpc",
        UnixStream::connect(socket).await.expect("connect"),
    )
    .await
    .expect("websocket");
    send_request(&mut client, 1, "thread/start", json!({"cwd":root.path()})).await;
    let thread = loop {
        let frame = next_frame(&mut client).await;
        if frame["id"] == 1 {
            break frame["result"]["thread"]["id"].clone();
        }
    };
    send_request(
        &mut client,
        2,
        "turn/start",
        json!({"threadId":thread,
        "clientUserMessageId":"start-client", "input":[{"type":"text","text":"same text"}]}),
    )
    .await;
    let mut frames = Vec::new();
    let mut start_reply = false;
    while !start_reply
        || !frames
            .iter()
            .any(|frame: &Value| frame["method"] == "item/completed")
    {
        let frame = next_frame(&mut client).await;
        start_reply |= frame["id"] == 2;
        frames.push(frame);
    }

    // Act: a refusal, then an injected steer, before the original turn settles.
    *backend.steer_failure.lock().expect("failure") = Some(CommandFailure::Busy);
    send_request(
        &mut client,
        3,
        "turn/steer",
        json!({"threadId":thread,"expectedTurnId":"turn-1",
        "clientUserMessageId":"refused-client","input":[{"type":"text","text":"refused text"}]}),
    )
    .await;
    loop {
        let frame = next_frame(&mut client).await;
        let refused = frame["id"] == 3;
        if refused {
            assert!(frame.get("error").is_some());
        }
        frames.push(frame);
        if refused {
            break;
        }
    }
    let receipt_gate = Arc::new(tokio::sync::Notify::new());
    *backend.steer_receipt_gate.lock().expect("receipt gate") = Some(receipt_gate.clone());
    send_request(
        &mut client,
        4,
        "turn/steer",
        json!({"threadId":thread,"expectedTurnId":"turn-1",
        "clientUserMessageId":"steer-client","input":[{"type":"text","text":"same "},{"type":"image","url":"data:image/png;base64,AA=="},{"type":"text","text":"text"}]}),
    )
    .await;
    loop {
        let frame = next_frame(&mut client).await;
        assert_ne!(frame["id"], 4, "receipt precedes the held steer reply");
        let committed = frame["method"] == "item/completed";
        frames.push(frame);
        if committed {
            break;
        }
    }
    receipt_gate.notify_one();
    loop {
        let frame = next_frame(&mut client).await;
        let replied = frame["id"] == 4;
        frames.push(frame);
        if replied {
            break;
        }
    }
    send_request(&mut client, 6, "turn/steer", json!({"threadId":thread,"expectedTurnId":"turn-1",
        "clientUserMessageId":"image-client","input":[{"type":"image","url":"data:image/png;base64,AA=="}]})).await;
    loop {
        let frame = next_frame(&mut client).await;
        let replied = frame["id"] == 6;
        frames.push(frame);
        if replied {
            break;
        }
    }
    send_request(
        &mut client,
        5,
        "turn/interrupt",
        json!({"threadId":thread,"turnId":"turn-1"}),
    )
    .await;
    let mut interrupted = false;
    let mut completed = false;
    while !interrupted || !completed {
        let frame = next_frame(&mut client).await;
        interrupted |= frame["id"] == 5;
        completed |= frame["method"] == "turn/completed";
        frames.push(frame);
    }

    // Assert: one started/completed pair per accepted input before turn end.
    let end = frames
        .iter()
        .position(|frame| frame["method"] == "turn/completed")
        .expect("turn ends");
    let receipts: Vec<_> = frames
        .iter()
        .enumerate()
        .filter(|(_, frame)| frame["params"]["item"]["type"] == "userMessage")
        .collect();
    assert_eq!(
        receipts.len(),
        6,
        "exactly three committed user messages: {frames:?}"
    );
    for (client_id, expected_method) in [
        ("start-client", "item/started"),
        ("start-client", "item/completed"),
        ("steer-client", "item/started"),
        ("steer-client", "item/completed"),
        ("image-client", "item/started"),
        ("image-client", "item/completed"),
    ] {
        let matching: Vec<_> = receipts
            .iter()
            .filter(|(_, frame)| {
                frame["method"] == expected_method
                    && frame["params"]["item"]["clientId"] == client_id
            })
            .collect();
        assert_eq!(matching.len(), 1, "receipt for {client_id}: {frames:?}");
        let (position, frame) = matching[0];
        assert!(*position < end);
        assert_eq!(
            frame["params"]["item"]["content"][0]["text"],
            if client_id == "image-client" {
                ""
            } else {
                "same text"
            }
        );
        assert_eq!(frame["params"]["turnId"], "turn-1");
    }
    shutdown.cancel();
    server.await.expect("server task").expect("server shutdown");
}
