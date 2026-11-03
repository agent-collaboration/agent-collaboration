//! Existing stored-message access and unverified-owner distinctions.
use super::*;
#[tokio::test]
async fn router_show_enforces_participants_and_marks_read_only_for_the_target() {
    let (_directory, store, delivery, mut control) = setup().await;
    let sender = session("claude-local", "sender-session");
    let target = session("codex-local", "target-session");
    let third_party = session("cursor-local", "third-party");
    let response = control
        .call(
            "message/send",
            send_params(&target, &sender, "private body"),
        )
        .await;
    let result = response.get("result").expect("message send accepted");
    let push_id = result["pushId"].as_str().expect("push id").to_owned();
    let link = result["link"].as_str().expect("push link").to_owned();

    let inbox_before_read = control
        .call("message/inbox", json!({"caller":target.clone(),"limit":50}))
        .await;
    assert_eq!(
        inbox_before_read["result"]["records"]
            .as_array()
            .expect("inbox records")
            .len(),
        1
    );
    assert!(
        inbox_before_read["result"]["records"][0]["line"]
            .as_str()
            .expect("one-line notice")
            .contains(&link)
    );
    let history = control
        .call(
            "message/history",
            json!({"caller":target.clone(),"with":sender.clone(),"limit":50}),
        )
        .await;
    assert_eq!(
        history["result"]["records"]
            .as_array()
            .expect("history")
            .len(),
        1
    );

    let sender_show = control
        .call("router/show", json!({"caller":sender,"reference":link}))
        .await;
    assert_eq!(sender_show["result"]["record"]["body"], "private body");
    assert!(sender_show["result"]["record"]["readAt"].is_null());
    assert!(
        store
            .get_push_record(&PushId::try_from(push_id.clone()).expect("push id"))
            .await
            .expect("read sender view")
            .expect("record exists")
            .read_at
            .is_none()
    );

    let denied = control
        .call(
            "router/show",
            json!({"caller":third_party,"reference":link}),
        )
        .await;
    assert_eq!(denied["error"]["data"]["kind"], "notPermitted");

    let target_show = control
        .call(
            "router/show",
            json!({"caller":target.clone(),"reference":push_id}),
        )
        .await;
    assert_eq!(target_show["result"]["record"]["body"], "private body");
    assert!(!target_show["result"]["record"]["readAt"].is_null());

    let inbox = control
        .call("message/inbox", json!({"caller":target,"limit":50}))
        .await;
    assert_eq!(
        inbox["result"]["records"]
            .as_array()
            .expect("inbox records")
            .len(),
        0
    );
    drop(store);
    drop(delivery);
    control.close().await;
}

#[tokio::test]
async fn unverified_owner_message_is_compact_and_has_no_owner_show_bypass() {
    let (_directory, store, delivery, mut control) = setup().await;
    let target = session("codex-local", "target-session");
    let third_party = session("cursor-local", "third-party");
    let response = control
        .call(
            "message/send",
            serde_json::to_value(SessionMessageSendParams {
                item_id: None,
                target: target.clone(),
                message: MessageContent::HumanUser {
                    text: "human text".to_owned().try_into().expect("message text"),
                },
                mode: MessageDelivery::Auto,
                generation_guard: None,
            })
            .expect("human request JSON"),
        )
        .await;
    let result = response.get("result").expect("owner message accepted");
    let push_id = PushId::try_from(result["pushId"].as_str().expect("push id").to_owned())
        .expect("valid push id");
    let record = store
        .get_push_record(&push_id)
        .await
        .expect("read stored owner message")
        .expect("owner message record");
    assert_eq!(record.origin, PushOrigin::OwnerUnverified);
    assert_eq!(result["receipt"]["outcome"]["kind"], "peerMessageWritten");
    {
        let requests = delivery
            .requests
            .lock()
            .expect("prepared delivery requests");
        let delivered = &requests[0];
        assert_eq!(delivered.target, target);
        assert_eq!(delivered.mode, MessageDelivery::Auto);
        assert!(matches!(
            delivered.precondition,
            DeliveryPrecondition::Unpinned
        ));
        assert_eq!(delivered.correlation.as_str(), push_id.as_str());
        assert_eq!(delivered.payload.push_id, push_id);
        let text = &delivered.payload.line;
        assert!(text.as_str().starts_with("🧑 Owner (unverified)"));
        assert!(text.as_str().contains("\"human text\""));
        assert!(
            text.as_str()
                .contains(&format!("router://{SERVICE_ID}/push/{}", push_id.as_str()))
        );
        assert!(!text.as_str().contains("Self-declared sender:"));
    }

    let denied = control
        .call(
            "router/show",
            json!({"caller":third_party,"reference":push_id.as_str()}),
        )
        .await;
    assert_eq!(denied["error"]["data"]["kind"], "notPermitted");
    let owner_reply = control
        .call(
            "message/reply",
            json!({"caller":target,"reference":push_id.as_str(),"text":"answer"}),
        )
        .await;
    assert_eq!(
        owner_reply["error"]["data"]["kind"],
        "ownerReplyUnsupported"
    );
    drop(store);
    drop(delivery);
    control.close().await;
}

#[tokio::test]
async fn reply_rejects_missing_reference_and_non_dm_records() {
    let (_directory, store, delivery, mut control) = setup().await;
    let target = session("codex-local", "target-session");
    let missing_reference = control
        .call("message/reply", json!({"caller":target,"text":"answer"}))
        .await;
    // The API refuses arguments that do not decode before any operation runs, with its
    // structured validation failure (Control's decoder answered a typed `invalidField`).
    assert!(
        missing_reference["error"].is_object(),
        "{missing_reference}"
    );
    assert_eq!(
        missing_reference["error"]["data"]["kind"], "protocolViolation",
        "{missing_reference}"
    );
    assert_eq!(
        missing_reference["error"]["data"]["stage"], "validation",
        "{missing_reference}"
    );
    assert_eq!(
        missing_reference["error"]["data"]["effect"], "none",
        "{missing_reference}"
    );

    let range = PushActivityRange {
        thread_id: message_board::ThreadId::generate(),
        project_id: message_board::ProjectId::generate(),
        from: message_board::FeedCursor {
            epoch: message_board::EpochNumber::try_from(1).expect("epoch"),
            position: message_board::FeedPosition::ZERO,
        },
        through: message_board::FeedCursor {
            epoch: message_board::EpochNumber::try_from(1).expect("epoch"),
            position: message_board::FeedPosition::ZERO,
        },
    };
    let non_dm = PushRecordDraft {
        mode: None,
        guard: None,
        push_id: PushId::try_from(uuid::Uuid::now_v7().to_string()).expect("push id"),
        kind: PushKind::SubscriptionActivity,
        origin: PushOrigin::Router(PushKind::SubscriptionActivity),
        origin_router_ref: Some(
            RouterOriginRef::SubscriptionActivity {
                target: target.clone(),
                batch_id: message_board::BatchId::generate(),
            }
            .canonical_string()
            .expect("canonical subscription batch origin"),
        ),
        target: target.clone(),
        reply_to_push_id: None,
        header_facts: PushHeaderFacts::SubscriptionActivity {
            root_count: 1,
            message_count: 0,
            held_since: None,
            thread_resolved: false,
        },
        body: None,
        activity: Some(PushActivitySnapshot {
            ranges: vec![range],
            held: false,
            draining: false,
        }),
        created_at: chrono::Utc::now(),
    };
    let record = store
        .insert_push_record(non_dm)
        .await
        .expect("store activity record");
    let response = control
        .call(
            "message/reply",
            json!({"caller":target,"reference":record.push_id.as_str(),"text":"answer"}),
        )
        .await;
    assert_eq!(response["error"]["data"]["kind"], "notDirectMessage");
    drop(store);
    drop(delivery);
    control.close().await;
}
