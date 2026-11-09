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

/// Same-epoch replay must finish the exact image inputs whose starts were live.
#[tokio::test]
async fn resync_preserves_two_image_steer_receipts_without_cross_stamping_or_duplicates() {
    // Arrange: attach and start an original turn; only steer completions are deferred.
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
        "clientUserMessageId":"original-client","input":[{"type":"text","text":"original"}]}),
    )
    .await;
    let mut replied = false;
    let mut committed = false;
    while !replied || !committed {
        let frame = next_frame(&mut client).await;
        replied |= frame["id"] == 2;
        committed |= frame["method"] == "item/completed";
    }
    backend
        .defer_user_receipt_completion
        .store(true, Ordering::Release);

    // Act: two rapid identical media inputs, with distinct submission identities.
    for (id, client_id) in [(3, "image-first"), (4, "image-second")] {
        send_request(&mut client, id, "turn/steer", json!({"threadId":thread,"expectedTurnId":"turn-1",
            "clientUserMessageId":client_id,"input":[{"type":"image","url":"data:image/png;base64,AA=="}]})).await;
    }
    let mut frames = Vec::new();
    let mut replies = 0;
    let mut starts = 0;
    while replies < 2 || starts < 2 {
        let frame = next_frame(&mut client).await;
        replies += usize::from(frame["id"] == 3 || frame["id"] == 4);
        starts += usize::from(frame["method"] == "item/started");
        frames.push(frame);
    }
    let started_ids: Vec<_> = frames
        .iter()
        .filter(|frame| frame["method"] == "item/started")
        .map(|frame| {
            frame["params"]["item"]["id"]
                .as_str()
                .expect("item ID")
                .to_owned()
        })
        .collect();
    {
        let mut history = backend.history.lock().expect("history");
        for item_id in &started_ids {
            let sequence = history.last().expect("history event").sequence + 1;
            history.push(HubEvent {
                sequence,
                event: SessionEvent::ItemCompleted {
                    item_id: item_id.clone(),
                },
            });
        }
        let sequence = history.last().expect("history event").sequence + 1;
        history.push(HubEvent {
            sequence,
            event: SessionEvent::TurnEnded {
                turn_id: "turn-1".into(),
                outcome: session_event_model::TurnOutcome::Ended {
                    stop_reason: session_event_model::StopReason::EndTurn,
                    local_cause: None,
                },
            },
        });
    }
    // Completions exist only in the snapshot. The receiver is explicitly invalidated.
    backend
        .events
        .send(HubEvent {
            sequence: 0,
            event: SessionEvent::ResyncRequired { replay_epoch: 0 },
        })
        .expect("resync");
    loop {
        let frame = next_frame(&mut client).await;
        let ended = frame["method"] == "turn/completed";
        frames.push(frame);
        if ended {
            break;
        }
    }

    // Assert: exactly one started/completed pair per input, with its own original ID.
    let user_frames: Vec<_> = frames
        .iter()
        .filter(|frame| frame["params"]["item"]["type"] == "userMessage")
        .collect();
    assert_eq!(user_frames.len(), 4, "no replay duplicates: {frames:?}");
    for (index, client_id) in ["image-first", "image-second"].into_iter().enumerate() {
        for method in ["item/started", "item/completed"] {
            let matches: Vec<_> = user_frames
                .iter()
                .filter(|frame| {
                    frame["method"] == method && frame["params"]["item"]["id"] == started_ids[index]
                })
                .collect();
            assert_eq!(matches.len(), 1, "one {method} for {client_id}");
            assert_eq!(
                matches[0]["params"]["item"]["clientId"], client_id,
                "original correlation survives resync"
            );
            assert_eq!(matches[0]["params"]["item"]["content"][0]["text"], "");
        }
    }
    assert_eq!(backend.attachment_count.load(Ordering::Acquire), 2);

    // Replaying once more must not duplicate committed frames or borrow their IDs.
    backend
        .events
        .send(HubEvent {
            sequence: 0,
            event: SessionEvent::ResyncRequired { replay_epoch: 0 },
        })
        .expect("second resync");
    timeout(Duration::from_secs(5), async {
        while backend.attachment_count.load(Ordering::Acquire) < 3 {
            backend.attachment_created.notified().await;
        }
    })
    .await
    .expect("second reattach");
    let sequence = backend
        .history
        .lock()
        .expect("history")
        .last()
        .expect("last event")
        .sequence
        + 1;
    backend
        .events
        .send(HubEvent {
            sequence,
            event: SessionEvent::TurnStarted {
                turn_id: "uncorrelated-turn".into(),
                input_id: session_event_model::InputId::generate(),
            },
        })
        .expect("new turn barrier");
    loop {
        let frame = next_frame(&mut client).await;
        assert_ne!(
            frame["params"]["item"]["type"], "userMessage",
            "no duplicate committed receipt"
        );
        if frame["method"] == "turn/started" && frame["params"]["turn"]["id"] == "uncorrelated-turn"
        {
            break;
        }
    }
    let foreign_item_id = Uuid::now_v7().to_string();
    backend
        .events
        .send(HubEvent {
            sequence: sequence + 1,
            event: SessionEvent::ItemStarted {
                item: session_event_model::SessionItem {
                    item_id: foreign_item_id.clone(),
                    kind: session_event_model::SessionItemKind::UserMessage,
                    text: Some("".into()),
                },
            },
        })
        .expect("foreign item");
    backend
        .events
        .send(HubEvent {
            sequence: sequence + 2,
            event: SessionEvent::ItemCompleted {
                item_id: foreign_item_id.clone(),
            },
        })
        .expect("foreign completion");
    for method in ["item/started", "item/completed"] {
        let frame = next_frame(&mut client).await;
        assert_eq!(frame["method"], method);
        assert_eq!(frame["params"]["item"]["id"], foreign_item_id);
        assert!(
            frame["params"]["item"].get("clientId").is_none(),
            "unrelated input must not borrow correlation"
        );
    }
    shutdown.cancel();
    server.await.expect("server task").expect("server shutdown");
}

/// The selected overflow policy drops oldest bookkeeping, never input admission.
#[test]
fn receipt_capacity_evicts_oldest_and_delivery_frees_capacity_without_cross_stamping() {
    // Arrange: use a small limit while exercising the production registration policy.
    let mut receipts = crate::native_user_receipts::NativeUserReceipts::with_capacity(
        std::num::NonZeroUsize::new(2).expect("nonzero test limit"),
    );
    let input_ids: Vec<_> = (0..4)
        .map(|_| session_event_model::InputId::generate())
        .collect();
    let register = |receipts: &mut crate::native_user_receipts::NativeUserReceipts,
                    index: usize| {
        receipts.register(&json!({"params":{"threadId":"thread","clientUserMessageId":format!("client-{index}")}}), input_ids[index].clone());
    };
    for index in 0..3 {
        register(&mut receipts, index);
    }

    // Act/Assert: only the oldest record was evicted; newer inputs still correlate.
    let mut oldest = json!({"id":"oldest"});
    receipts.accept("thread", &input_ids[0]);
    receipts.stamp("thread", "oldest", true, &mut oldest, None);
    assert!(oldest.get("clientId").is_none());
    let mut second = json!({"id":"second"});
    receipts.accept("thread", &input_ids[1]);
    let mut unrelated_update = json!({"id":"earlier-item"});
    receipts.stamp("thread", "earlier-item", false, &mut unrelated_update, None);
    assert!(unrelated_update.get("clientId").is_none());
    receipts.stamp("thread", "second", true, &mut second, None);
    assert_eq!(second["clientId"], "client-1");
    receipts.complete_turn("thread");
    receipts.accept("thread", &input_ids[1]);
    let mut different_item = json!({"id":"different-item"});
    receipts.stamp("thread", "different-item", true, &mut different_item, None);
    assert!(
        different_item.get("clientId").is_none(),
        "one input cannot claim a different item"
    );
    let mut replay = json!({"id":"second"});
    receipts.accept("thread", &input_ids[1]);
    receipts.stamp("thread", "second", true, &mut replay, None);
    assert_eq!(replay["clientId"], "client-1");

    // Successful completion releases a slot, preserving the next older record.
    receipts.delivered(&json!({"method":"item/completed","params":{"threadId":"thread","item":{"type":"userMessage","id":"second"}}}));
    register(&mut receipts, 3);
    let mut third = json!({"id":"third"});
    receipts.accept("thread", &input_ids[2]);
    receipts.stamp("thread", "third", true, &mut third, None);
    assert_eq!(
        third["clientId"], "client-2",
        "delivery freed capacity instead of evicting this input"
    );
    let mut delivered_replay = json!({"id":"second"});
    receipts.accept("thread", &input_ids[1]);
    receipts.stamp("thread", "second", true, &mut delivered_replay, None);
    assert!(
        delivered_replay.get("clientId").is_none(),
        "delivered record was pruned"
    );
}
