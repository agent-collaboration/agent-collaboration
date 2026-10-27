#![allow(clippy::expect_used)]
#[path = "support/native_delivery.rs"]
mod native_delivery;

#[path = "support/board_fixture.rs"]
pub mod board_fixture;

use automation_storage::AutomationStore;
use collaboration_contracts::DeliveryPrecondition;
use collaboration_protocol::{
    DeliveryOutcome, EndpointId, EndpointRef, MessageContent, MessageDelivery, PushActivityRange,
    PushActivitySnapshot, PushDeliveryState, PushHeaderFacts, PushId, PushKind, PushOrigin,
    PushRecordDraft, RouterOriginRef, SessionId, SessionMessageSendParams, SessionRef,
    UuidIdentity,
};
use collaboration_service::ServiceIdentity;
use serde_json::{Value, json};
use std::sync::{Arc, atomic::Ordering};
use tokio::sync::Mutex as TokioMutex;

const SERVICE_ID: &str = "00000000-0000-4000-8000-000000000001";

#[path = "push_surface/board_and_history_proofs.rs"]
mod board_and_history_proofs;
#[path = "push_surface/dm_read_proofs.rs"]
mod dm_read_proofs;
#[path = "support/served_api.rs"]
mod served_api;
#[path = "push_surface/subscription_expansion_proofs.rs"]
mod subscription_expansion_proofs;

use board_fixture::native_mail_owner;
#[path = "push_surface/native_push_fixture.rs"]
mod native_push_fixture;
use native_push_fixture::{
    ControlHarness, PushFixtureLifetime, RecordingDelivery, RunningPresence, setup,
    setup_without_push_store,
};

fn session(endpoint_id: &str, session_id: &str) -> SessionRef {
    SessionRef {
        endpoint: EndpointRef {
            service_id: UuidIdentity::try_from(SERVICE_ID.to_owned()).expect("service id"),
            endpoint_id: EndpointId::try_from(endpoint_id.to_owned()).expect("endpoint id"),
        },
        session_id: SessionId::try_from(session_id.to_owned()).expect("session id"),
    }
}

fn send_params(target: &SessionRef, sender: &SessionRef, text: &str) -> Value {
    serde_json::to_value(SessionMessageSendParams {
        item_id: None,
        target: target.clone(),
        message: MessageContent::Agent {
            sender: sender.clone(),
            text: text.to_owned().try_into().expect("message text"),
        },
        mode: MessageDelivery::Auto,
        generation_guard: None,
    })
    .expect("message request JSON")
}

#[tokio::test]
async fn message_send_persists_before_delivery_and_submits_one_linked_line() {
    let (_directory, store, delivery, mut control) = setup().await;
    let sender = session("claude-local", "sender-session");
    let target = session("codex-local", "target-session");

    let response = control
        .call(
            "message/send",
            send_params(&target, &sender, "snapshot body"),
        )
        .await;

    let result = response.get("result").expect("message send accepted");
    assert_eq!(result["receipt"]["outcome"]["kind"], "peerMessageWritten");
    let push_id = PushId::try_from(
        result["pushId"]
            .as_str()
            .expect("push id in delivery result")
            .to_owned(),
    )
    .expect("valid push id");
    assert_eq!(
        result["link"],
        format!("router://{SERVICE_ID}/push/{}", push_id.as_str())
    );
    assert!(delivery.stored_before_delivery.load(Ordering::SeqCst));

    let stored = store
        .get_push_record(&push_id)
        .await
        .expect("read stored message")
        .expect("message snapshot exists");
    assert_eq!(stored.body.as_deref(), Some("snapshot body"));
    assert_eq!(stored.origin, PushOrigin::Session(sender));
    assert_eq!(stored.delivery_state, PushDeliveryState::Delivered);

    {
        let requests = delivery
            .requests
            .lock()
            .expect("prepared delivery requests");
        assert_eq!(requests.len(), 1);
        let prepared = &requests[0];
        assert_eq!(prepared.payload.push_id, push_id);
        assert_eq!(
            prepared.payload.load_policy,
            collaboration_contracts::LoadPolicy::MayLoad
        );
        assert_eq!(prepared.correlation.as_str(), push_id.as_str());
        let text = &prepared.payload.line;
        assert_eq!(prepared.target, target);
        assert_eq!(text.as_str().lines().count(), 1);
        assert!(text.as_str().contains("snapshot body"));
        assert!(
            text.as_str()
                .contains(&result["link"].as_str().expect("link").to_owned())
        );
        assert!(!text.as_str().contains("Agent communication"));
        assert!(!text.as_str().contains("Self-declared sender:"));
    }
    drop(store);
    drop(delivery);
    control.close().await;
}

#[tokio::test]
async fn truncated_dispatch_cue_links_to_the_original_body_through_public_show() {
    let (_directory, store, delivery, mut control) = setup().await;
    let sender = session("claude-local", "sender-session");
    let target = session("codex-local", "target-session");
    let body = format!("{}\"\\\n{}", "🪿".repeat(97), "界".repeat(1_240));
    assert_eq!(body.chars().count(), 1_340);

    let sent = control
        .call("message/send", send_params(&target, &sender, &body))
        .await;
    let result = sent.get("result").expect("message send accepted");
    let push_id = result["pushId"].as_str().expect("stored push id");
    let link = format!("router://{SERVICE_ID}/push/{push_id}");
    assert_eq!(result["link"], link);

    // RecordingDelivery observes the production dispatch boundary, not peer/native IO.
    let dispatched = {
        let requests = delivery
            .requests
            .lock()
            .expect("prepared delivery requests");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].target, target);
        assert_eq!(requests[0].payload.push_id.as_str(), push_id);
        requests[0].payload.line.as_str().to_owned()
    };
    let expected_line = format!(
        r#"✉️ ✳️ claude-local/sender-s @push-surface-test → you · "{}\"\\⏎…" (+1240) 📖 Load full message → {link}"#,
        "🪿".repeat(97),
    );
    assert_eq!(dispatched, expected_line);
    assert!(dispatched.len() <= 1_024);
    assert_eq!(dispatched.lines().count(), 1);
    assert!(!dispatched.contains('界'));

    let shown = control
        .call("router/show", json!({"caller":target,"reference":link}))
        .await;
    assert_eq!(shown.pointer("/result/record/body"), Some(&json!(body)));
    assert_eq!(shown.pointer("/result/link"), Some(&json!(link)));
    assert!(shown.pointer("/result/record/preview").is_none());
    assert!(!shown["result"]["record"]["readAt"].is_null());
    drop(store);
    drop(delivery);
    control.close().await;
}

#[tokio::test]
async fn show_returns_stored_rejected_and_unknown_delivery_outcomes() {
    for (outcome, expected_state, expected_kind, expected_claims) in [
        (
            DeliveryOutcome::Rejected(collaboration_protocol::DeliveryRejection {
                reason: collaboration_protocol::DeliveryRejectionReason::LiveElsewhere,
                next_action: collaboration_protocol::DeliveryNextAction::InspectTarget,
                client_code: None,
                detail: Some("two terminals claim this session".to_owned()),
                claims: Some(vec![
                    collaboration_protocol::DeliveryPeerClaim {
                        pid: 52304,
                        name: Some("terminal-one".to_owned()),
                        cwd: Some("/workspace/one".to_owned()),
                    },
                    collaboration_protocol::DeliveryPeerClaim {
                        pid: 68833,
                        name: None,
                        cwd: None,
                    },
                ]),
                native_failure: None,
            }),
            "rejected",
            "rejected",
            Some(json!([
                {"pid":52304,"name":"terminal-one","cwd":"/workspace/one"},
                {"pid":68833,"name":null,"cwd":null}
            ])),
        ),
        (DeliveryOutcome::Unknown, "outcome-unknown", "unknown", None),
    ] {
        let (_directory, store, delivery, mut control) = setup().await;
        delivery.set_outcome(outcome);
        let sender = session("claude-local", "sender-session");
        let target = session("codex-local", "target-session");

        let sent = control
            .call(
                "message/send",
                send_params(&target, &sender, "stored outcome"),
            )
            .await;
        let result = sent.get("result").expect("send returns typed receipt");
        let link = result["link"].as_str().expect("push link").to_owned();
        assert_eq!(
            result
                .pointer("/receipt/outcome/kind")
                .and_then(Value::as_str),
            Some(expected_kind)
        );
        if let Some(expected_claims) = expected_claims.as_ref() {
            assert_eq!(
                result.pointer("/receipt/outcome/claims"),
                Some(expected_claims)
            );
        }

        let shown = control
            .call("router/show", json!({"caller":target,"reference":link}))
            .await;
        assert_eq!(
            shown
                .pointer("/result/record/deliveryState")
                .and_then(Value::as_str),
            Some(expected_state)
        );
        assert_eq!(
            shown
                .pointer("/result/record/lastOutcome/outcome/kind")
                .and_then(Value::as_str),
            Some(expected_kind)
        );
        if expected_kind == "rejected" {
            assert_eq!(
                shown
                    .pointer("/result/record/lastOutcome/outcome/detail")
                    .and_then(Value::as_str),
                Some("two terminals claim this session")
            );
            assert_eq!(
                shown.pointer("/result/record/lastOutcome/outcome/claims"),
                expected_claims.as_ref()
            );
        }
        drop(store);
        drop(delivery);
        control.close().await;
    }
}

#[tokio::test]
async fn missing_push_store_rejects_without_attempting_delivery() {
    let (_directory, stand_in_store, delivery, mut control) = setup_without_push_store().await;
    let sender = session("claude-local", "sender-session");
    let target = session("codex-local", "target-session");

    let response = control
        .call(
            "message/send",
            send_params(&target, &sender, "must not send"),
        )
        .await;

    assert_eq!(response["error"]["data"]["kind"], "unavailable");
    assert!(
        delivery
            .requests
            .lock()
            .expect("delivery requests")
            .is_empty()
    );
    assert!(
        stand_in_store
            .list_direct_message_inbox(&interaction_storage::PushInboxQuery { target, limit: 10 })
            .await
            .expect("read stand-in store")
            .is_empty()
    );
    drop(delivery);
    control.close().await;
    Arc::try_unwrap(stand_in_store)
        .unwrap_or_else(|_| panic!("stand-in native store retained"))
        .close()
        .await
        .expect("close stand-in native store");
}

#[tokio::test]
async fn foreign_and_missing_links_have_distinct_errors() {
    let (_directory, store, delivery, mut control) = setup().await;
    let caller = session("codex-local", "caller");
    let push_id = uuid::Uuid::now_v7().to_string();
    let foreign = control
        .call(
            "router/show",
            json!({
                "caller":caller.clone(),
                "reference":format!("router://11111111-1111-7111-8111-111111111111/push/{push_id}")
            }),
        )
        .await;
    assert_eq!(foreign["error"]["data"]["kind"], "foreignMachine");
    assert!(
        foreign["error"]["message"]
            .as_str()
            .expect("foreign message")
            .contains("cross-machine fetch not available yet")
    );

    let missing = control
        .call("router/show", json!({"caller":caller,"reference":push_id}))
        .await;
    assert_eq!(missing["error"]["data"]["kind"], "notFound");
    assert!(
        missing["error"]["message"]
            .as_str()
            .expect("not-found message")
            .contains("expired after 30 days")
    );
    drop(store);
    drop(delivery);
    control.close().await;
}

#[tokio::test]
async fn reply_uses_the_selected_message_id_after_a_later_dm_arrives() {
    let (_directory, store, delivery, mut control) = setup().await;
    let target = session("codex-local", "recipient-session");
    let first_sender = session("claude-local", "first-sender");
    let second_sender = session("cursor-local", "second-sender");
    let first = control
        .call("message/send", send_params(&target, &first_sender, "first"))
        .await;
    let first_id = first["result"]["pushId"]
        .as_str()
        .expect("first id")
        .to_owned();
    control
        .call(
            "message/send",
            send_params(&target, &second_sender, "second"),
        )
        .await;

    let reply = control
        .call(
            "message/reply",
            json!({"caller":target,"reference":first_id,"text":"reply to first"}),
        )
        .await;

    assert!(reply.get("result").is_some(), "reply response: {reply}");
    assert_eq!(
        reply["result"]["target"],
        serde_json::to_value(&first_sender).expect("target JSON")
    );
    let reply_id = PushId::try_from(
        reply["result"]["pushId"]
            .as_str()
            .expect("reply push id")
            .to_owned(),
    )
    .expect("reply id");
    let stored_reply = store
        .get_push_record(&reply_id)
        .await
        .expect("read reply")
        .expect("reply is stored");
    assert_eq!(
        stored_reply.reply_to_push_id.as_ref().map(PushId::as_str),
        Some(first_id.as_str())
    );
    assert_eq!(stored_reply.origin, PushOrigin::Session(target));
    {
        let requests = delivery
            .requests
            .lock()
            .expect("prepared delivery requests");
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[2].target, first_sender);
        assert_eq!(requests[2].payload.push_id, reply_id);
        let text = &requests[2].payload.line;
        assert!(text.as_str().contains("reply to first"));
    }
    drop(store);
    drop(delivery);
    control.close().await;
}
