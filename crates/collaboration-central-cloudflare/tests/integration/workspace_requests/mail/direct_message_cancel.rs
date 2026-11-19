//! On workerd: a direct message nobody acknowledged is cancelled at its
//! 5-minute mark by the due batch, which tells the sender (design 2026-11-17).
use super::super::{Statement, support::*};
use central_storage::SqlValue;
use std::sync::atomic::Ordering;
use workspace_model::central_mail::{
    CentralMailAnswer as Answer, CentralMailOperation as Operation, CentralMailRequest,
    CentralMailResult as Result, MessageBody, MessageCancellation, MessageDelivery, MessageOutcome,
    MessageSender,
};

fn row_text(value: &SqlValue) -> &str {
    match value {
        SqlValue::Text(text) => text,
        other => panic!("expected text, got {other:?}"),
    }
}

#[tokio::test]
async fn an_unacknowledged_direct_message_is_cancelled_at_its_mark_and_the_sender_is_told() {
    let fixture = Fixture::new().await;
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    let sender = session(&fixture.caller);
    let sent_at =
        chrono::DateTime::from_timestamp_millis(fixture.clock.0.load(Ordering::SeqCst)).unwrap();
    let answer = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(1500),
                operation: Operation::SendMessage {
                    to: sender.clone(),
                    body: "are you there".to_owned().try_into().unwrap(),
                    delivery: MessageDelivery::Auto,
                    reply_to: None,
                    sent_at,
                },
            },
        )
        .await
        .unwrap();
    let Result::Done(Answer::Queued { message_id }) = answer else {
        panic!("send refused: {answer:?}")
    };
    let mark = collaboration_central_core::message_deadline::cancel_due_at(sent_at, sent_at);
    let deadline = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT delivery_window_ends_at FROM mail_messages WHERE message_id=?",
            args: vec![text(&message_id)],
        })
        .await
        .unwrap();
    let deadline: chrono::DateTime<chrono::Utc> = row_text(&deadline.rows[0][0]).parse().unwrap();
    assert_eq!(deadline, sent_at + chrono::Duration::minutes(15));
    let state = || async {
        runtime
            .statement_host()
            .read(Statement {
                sql: "SELECT state FROM mail_messages WHERE message_id=?",
                args: vec![text(&message_id)],
            })
            .await
            .unwrap()
            .rows[0][0]
            .clone()
    };
    assert_eq!(
        runtime
            .control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap(),
        Some(mark.timestamp_millis()),
        "the accept arms the Durable Object alarm at the cancel mark"
    );

    fixture.clock.0.store(
        (mark - chrono::Duration::minutes(1)).timestamp_millis(),
        Ordering::SeqCst,
    );
    runtime.after_event().await.unwrap();
    assert_eq!(state().await, SqlValue::Text("queued".to_owned()));

    fixture
        .clock
        .0
        .store(mark.timestamp_millis(), Ordering::SeqCst);
    runtime.after_event().await.unwrap();
    assert_eq!(state().await, SqlValue::Text("cancelled".to_owned()));
    let notices = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT sender,target_session,body,cancel_due_at,created_at,delivery_window_ends_at FROM mail_messages WHERE message_kind='deliveryOutcome'",
            args: vec![],
        })
        .await
        .unwrap()
        .rows;
    let [notice] = notices.as_slice() else {
        panic!("one outcome message for the sender: {notices:?}")
    };
    assert_eq!(
        serde_json::from_str::<MessageSender>(row_text(&notice[0])).unwrap(),
        MessageSender::Central
    );
    assert_eq!(
        serde_json::from_str::<workspace_ids::SessionRef>(row_text(&notice[1])).unwrap(),
        sender
    );
    assert_eq!(
        serde_json::from_str::<MessageBody>(row_text(&notice[2])).unwrap(),
        MessageBody::DeliveryOutcome {
            message_id: message_id.clone(),
            request_id: id(1500),
            outcome: MessageOutcome::Cancelled {
                reason: MessageCancellation::NotDeliveredWithin15Minutes,
            },
        }
    );
    assert_eq!(notice[3], SqlValue::Null);
    let created: chrono::DateTime<chrono::Utc> = row_text(&notice[4]).parse().unwrap();
    let deadline: chrono::DateTime<chrono::Utc> = row_text(&notice[5]).parse().unwrap();
    assert_eq!(deadline, created + chrono::Duration::hours(24));
    assert_eq!(
        runtime
            .control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap(),
        None,
        "nothing timed is left: the outcome message never arms the alarm"
    );
}
