//! A direct message nobody acknowledged is cancelled at its 5-minute mark by
//! the due batch, which tells the sender (design 2026-11-17, owner record:218).
use super::super::{Statement, support::*};
use central_storage::SqlValue;
use router_clock::RuntimeClock;
use workspace_model::central_mail::{
    CentralMailAnswer as Answer, CentralMailOperation as Operation, CentralMailRequest,
    CentralMailResult as Result, MessageBody, MessageCancellation, MessageDelivery, MessageOutcome,
    MessageSender,
};

fn row_text(value: &SqlValue) -> Option<&str> {
    match value {
        SqlValue::Text(text) => Some(text),
        _ => None,
    }
}

#[tokio::test]
async fn an_unacknowledged_direct_message_is_cancelled_at_its_mark_and_the_sender_is_told() {
    let fixture = Fixture::new().await;
    let runtime = fixture
        .host
        .central(fixture.caller.organization_id())
        .await
        .unwrap();
    let sender = session(&fixture.caller);
    let sent_at = fixture.clock.utc_now();
    let caller = fixture.fresh_caller(&fixture.caller).await;
    let answer = fixture
        .host
        .request_central(
            &caller,
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
    let stored = |sql: &'static str| {
        let runtime = &runtime;
        let message_id = message_id.clone();
        async move {
            runtime
                .statement_host()
                .read(Statement {
                    sql,
                    args: vec![text(&message_id)],
                })
                .await
                .unwrap()
                .rows
        }
    };
    let terms =
        stored("SELECT cancel_due_at,sender_request_id,state FROM messages WHERE message_id=?")
            .await;
    assert_eq!(
        terms[0].iter().map(row_text).collect::<Vec<_>>(),
        vec![
            Some(
                mark.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
                    .as_str()
            ),
            Some(id::<workspace_ids::RequestId>(1500).to_string().as_str()),
            Some("queued"),
        ],
        "accept stores the cancel mark and the sender's request id"
    );
    assert_eq!(
        runtime.statement_host().published_alarm().unwrap(),
        Some(mark),
        "the cancel is timed work: the accept arms the alarm at its mark"
    );

    // One minute before the mark is outside the batch's 50 s look-ahead.
    fixture.set_utc(mark - chrono::Duration::minutes(1)).await;
    runtime.after_event().await.unwrap();
    assert_eq!(
        stored("SELECT state FROM messages WHERE message_id=?").await[0][0],
        SqlValue::Text("queued".to_owned()),
        "never cancelled before its mark"
    );

    fixture.set_utc(mark).await;
    runtime.after_event().await.unwrap();
    assert_eq!(
        stored("SELECT state FROM messages WHERE message_id=?").await[0][0],
        SqlValue::Text("cancelled".to_owned())
    );
    let notices = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT sender,target_session,body,state,cancel_due_at,sender_request_id FROM messages WHERE message_kind='deliveryOutcome'",
            args: vec![],
        })
        .await
        .unwrap()
        .rows;
    let [notice] = notices.as_slice() else {
        panic!("one outcome message for the sender: {notices:?}")
    };
    assert_eq!(
        serde_json::from_str::<MessageSender>(row_text(&notice[0]).unwrap()).unwrap(),
        MessageSender::Central
    );
    assert_eq!(
        serde_json::from_str::<workspace_ids::SessionRef>(row_text(&notice[1]).unwrap()).unwrap(),
        sender
    );
    assert_eq!(
        serde_json::from_str::<MessageBody>(row_text(&notice[2]).unwrap()).unwrap(),
        MessageBody::DeliveryOutcome {
            message_id: message_id.clone(),
            request_id: id(1500),
            outcome: MessageOutcome::Cancelled {
                reason: MessageCancellation::NotDeliveredWithin15Minutes,
            },
        }
    );
    assert_eq!(
        notice[3..],
        [
            SqlValue::Text("queued".to_owned()),
            SqlValue::Null,
            SqlValue::Null
        ],
        "the outcome message is queued with no cancel of its own"
    );

    fixture.set_utc(mark + chrono::Duration::minutes(30)).await;
    runtime.after_event().await.unwrap();
    assert_eq!(
        runtime.statement_host().published_alarm().unwrap(),
        None,
        "nothing timed is left: the outcome message never arms the alarm"
    );
    assert_eq!(
        runtime
            .statement_host()
            .read(Statement {
                sql: "SELECT COUNT(*) FROM messages WHERE message_kind='deliveryOutcome'",
                args: vec![],
            })
            .await
            .unwrap()
            .rows[0][0],
        SqlValue::Integer(1.try_into().unwrap()),
        "a cancelled message is not cancelled twice"
    );
}
