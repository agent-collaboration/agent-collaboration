use super::*;
use collaboration_protocol::central_mail::{
    CentralMailAnswer, CentralMailOperation, CentralMailRequest, CentralMailResult, MessageBody,
    MessageSender,
};
use router_clock::ClockInstant;
use std::time::Duration;

#[tokio::test]
async fn leaving_cancels_the_fired_collated_notice_and_retains_direct_message() {
    let f = Fixture::new().await;
    let area = f.area(7400).await;
    let thread = f.thread(7401, area).await;
    let (observer, recipient) = joined_observer(&f, &thread).await;
    let runtime = f.host.central(&f.caller.organization_id).await.unwrap();
    let posted = f.call(7402, post(&thread, vec![])).await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { position, .. }) = posted.result
    else {
        panic!("post")
    };
    let sent = f
        .host
        .request_central(
            &f.caller,
            CentralMailRequest {
                request_id: id(7403),
                operation: CentralMailOperation::SendMessage {
                    to: session(&observer),
                    body: "Direct stays ready".to_owned().try_into().unwrap(),
                },
            },
        )
        .await
        .unwrap();
    let CentralMailResult::Done(CentralMailAnswer::Queued {
        message_id: direct_id,
    }) = sent
    else {
        panic!("direct mail")
    };
    let mut fired = runtime
        .pause_points()
        .hold_central(PausePoint::AfterTimerCommit);
    f.clock
        .not_due_check_now(ClockInstant::ZERO.after(Duration::from_secs(60)))
        .await;
    f.clock.advance(Duration::from_secs(900)).unwrap();
    fired.until_held().await;
    let rows = runtime.statement_host().read(Statement {
        sql: "SELECT message_id,state,sender,body FROM messages WHERE recipient_actor=? AND source_thread_id=? AND message_kind='notice'",
        args: vec![text(&recipient), text(&thread)],
    }).await.unwrap();
    assert_eq!(rows.rows.len(), 1);
    let (SqlValue::Text(notice_id), SqlValue::Text(sender), SqlValue::Text(body)) =
        (&rows.rows[0][0], &rows.rows[0][2], &rows.rows[0][3])
    else {
        panic!("notice encoding")
    };
    assert_eq!(rows.rows[0][1], text("ready"));
    assert_eq!(
        serde_json::from_str::<MessageSender>(sender).unwrap(),
        MessageSender::Central
    );
    let MessageBody::Notice { items } = serde_json::from_str::<MessageBody>(body).unwrap() else {
        panic!("notice body")
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].position, position);
    let notice_id = notice_id.clone();
    let windows = runtime.statement_host().read(Statement { sql: "SELECT (SELECT COUNT(*) FROM notification_windows WHERE recipient_actor=? AND window_id IN (SELECT window_id FROM notification_window_items WHERE thread_id=?)), (SELECT COUNT(*) FROM notification_window_items WHERE change_id=?)", args: vec![text(&recipient), text(&thread), text(&items[0].change_id)] }).await.unwrap();
    assert_eq!(
        windows.rows,
        vec![vec![
            SqlValue::Integer(0.try_into().unwrap()),
            SqlValue::Integer(0.try_into().unwrap())
        ]]
    );
    fired.release();
    let left = call(
        &f.host,
        &observer,
        Some(f.workspace.clone()),
        7404,
        m::WorkspaceOperation::LeaveThread {
            thread_id: thread.clone(),
        },
    )
    .await;
    assert!(matches!(
        left.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    let after = runtime.statement_host().read(Statement { sql: "SELECT message_id,state FROM messages WHERE recipient_actor=? AND source_thread_id=? AND message_kind='notice'", args: vec![text(&recipient), text(&thread)] }).await.unwrap();
    assert_eq!(after.rows, vec![vec![text(&notice_id), text("cancelled")]]);
    let direct = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT message_id,state,message_kind FROM messages WHERE message_id=?",
            args: vec![text(&direct_id)],
        })
        .await
        .unwrap();
    assert_eq!(
        direct.rows,
        vec![vec![text(&direct_id), text("ready"), text("direct")]]
    );
    f.host.close().await;
}
