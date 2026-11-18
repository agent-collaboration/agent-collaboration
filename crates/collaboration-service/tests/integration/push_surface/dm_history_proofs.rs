//! Local direct-message reads: unread inbox, correspondent history, mark-read,
//! retention and the short preview line, over the native interaction store.
use super::*;
use chrono::{DateTime, Duration, Months, Utc};
use collaboration_protocol::{DeliveryReceipt, PushRecordNotice};
use interaction_storage::InteractionStore;
use native_push_fixture::setup_with_retention;

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
async fn stored_delivered_dm(
    pushes: &InteractionStore,
    origin: &SessionRef,
    target: &SessionRef,
    created_at: DateTime<Utc>,
    body: &str,
) -> PushId {
    let push_id = PushId::try_from(uuid::Uuid::now_v7().to_string()).expect("UUIDv7 push ID");
    pushes
        .insert_push_record(PushRecordDraft {
            push_id: push_id.clone(),
            kind: PushKind::DirectMessage,
            origin: PushOrigin::Session(origin.clone()),
            origin_router_ref: None,
            target: target.clone(),
            reply_to_push_id: None,
            header_facts: PushHeaderFacts::DirectMessage {
                sender_display_name: None,
            },
            body: Some(body.to_owned()),
            activity: None,
            mode: Some(MessageDelivery::Auto),
            guard: None,
            created_at,
        })
        .await
        .expect("persist a typed DM in the native store");
    pushes
        .mark_push_attempted(&push_id)
        .await
        .expect("mark stored DM delivery attempted");
    pushes
        .settle_push_record(
            &push_id,
            DeliveryReceipt {
                outcome: DeliveryOutcome::PeerMessageWritten,
                reachability: None,
                client: None,
            },
            created_at,
        )
        .await
        .expect("settle stored DM through its delivery transition");
    push_id
}

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
fn notices(response: Value) -> Vec<PushRecordNotice> {
    serde_json::from_value(
        response
            .pointer("/result/records")
            .expect("real Control returned notice records")
            .clone(),
    )
    .expect("decode published notice types")
}

fn notice_ids(records: &[PushRecordNotice]) -> Vec<PushId> {
    records
        .iter()
        .map(|record| record.push_id.clone())
        .collect()
}

#[tokio::test]
async fn inbox_and_history_use_literal_short_preview_while_show_keeps_full_body() {
    let (_directory, store, delivery, mut control) = setup().await;
    let sender = session("claude-local", "preview-sender");
    let caller = session("codex-local", "preview-recipient");
    let body = "🪿".repeat(1_340);
    let sent = control
        .call("message/send", send_params(&caller, &sender, &body))
        .await;
    let push_id = PushId::try_from(
        sent.pointer("/result/pushId")
            .and_then(Value::as_str)
            .expect("message send returns push id")
            .to_owned(),
    )
    .expect("valid push id");
    let link = sent
        .pointer("/result/link")
        .and_then(Value::as_str)
        .expect("message send returns full link")
        .to_owned();
    let expected_tail = format!(
        "\"{}…\" (+1240) 📖 Load full message → {link}",
        "🪿".repeat(100)
    );

    let inbox = notices(
        control
            .call("message/inbox", json!({"caller":caller,"limit":50}))
            .await,
    );
    let inbox_notice = inbox
        .iter()
        .find(|record| record.push_id == push_id)
        .expect("inbox retains the push notice");
    assert!(inbox_notice.line.ends_with(&expected_tail));
    assert!(inbox_notice.line.ends_with(&link));

    let history = notices(
        control
            .call(
                "message/history",
                json!({"caller":caller,"with":sender,"limit":50}),
            )
            .await,
    );
    let history_notice = history
        .iter()
        .find(|record| record.push_id == push_id)
        .expect("history retains the same push notice");
    assert_eq!(history_notice.line, inbox_notice.line);
    assert!(history_notice.line.ends_with(&expected_tail));

    let shown = control
        .call("router/show", json!({"caller":caller,"reference":link}))
        .await;
    assert_eq!(shown.pointer("/result/record/body"), Some(&json!(body)));
    assert!(shown.pointer("/result/record/preview").is_none());
    drop(store);
    drop(delivery);
    control.close().await;
}

#[tokio::test]
async fn control_inbox_and_history_order_retained_dms_and_omit_expired_records() {
    let (_directory, pushes, delivery, mut control, retention) = setup_with_retention().await;
    // Maintenance accepts an injected wall time; no real-time wait drives expiry.
    let now = DateTime::parse_from_rfc3339("2026-09-01T12:00:00Z")
        .expect("fixed retention clock")
        .with_timezone(&Utc);
    let cutoff = now
        .checked_sub_months(Months::new(2))
        .expect("two-month retention cutoff");
    let caller = session("codex-local", "history-recipient");
    let correspondent = session("claude-local", "history-correspondent");
    let other = session("cursor-local", "unrelated-correspondent");
    let expired = stored_delivered_dm(
        &pushes,
        &correspondent,
        &caller,
        cutoff - Duration::milliseconds(1),
        "expired body",
    )
    .await;
    let boundary = stored_delivered_dm(
        &pushes,
        &correspondent,
        &caller,
        cutoff,
        "exact two-month boundary",
    )
    .await;
    let older = stored_delivered_dm(
        &pushes,
        &correspondent,
        &caller,
        now - Duration::days(2),
        "older unread",
    )
    .await;
    let read = stored_delivered_dm(
        &pushes,
        &correspondent,
        &caller,
        now - Duration::days(1),
        "read conversation message",
    )
    .await;
    let outbound = stored_delivered_dm(
        &pushes,
        &caller,
        &correspondent,
        now - Duration::hours(3),
        "outgoing conversation message",
    )
    .await;
    let newest = stored_delivered_dm(
        &pushes,
        &correspondent,
        &caller,
        now - Duration::hours(2),
        "newest incoming",
    )
    .await;
    let unrelated = stored_delivered_dm(
        &pushes,
        &other,
        &caller,
        now - Duration::hours(1),
        "other correspondent",
    )
    .await;
    let shown = control
        .call("router/show", json!({"caller":caller,"reference":read}))
        .await;
    assert!(
        shown
            .pointer("/result/record/readAt")
            .is_some_and(|value| !value.is_null())
    );
    assert_eq!(
        retention
            .prune_batch(now.timestamp_millis())
            .await
            .expect("real retained-content maintenance"),
        1
    );
    assert!(
        pushes
            .get_push_record(&expired)
            .await
            .expect("inspect expired ID")
            .is_none()
    );
    let inbox = notices(
        control
            .call("message/inbox", json!({"caller":caller,"limit":50}))
            .await,
    );
    assert_eq!(
        notice_ids(&inbox),
        vec![unrelated, newest.clone(), older.clone(), boundary.clone()]
    );
    assert!(inbox.iter().all(|record| record.read_at.is_none()
        && record.target == caller
        && record.created_at >= cutoff));
    let history = notices(
        control
            .call(
                "message/history",
                json!({"caller":caller,"with":correspondent,"limit":50}),
            )
            .await,
    );
    assert_eq!(
        notice_ids(&history),
        vec![newest, outbound, read, older, boundary]
    );
    assert!(history.iter().all(|record| record.created_at >= cutoff));
    assert!(history.iter().any(|record| record.target == correspondent));
    assert!(history.iter().any(|record| record.read_at.is_some()));
    // The owner closes only once no test handle shares its store.
    drop(retention);
    drop(pushes);
    drop(delivery);
    control.close().await;
}
