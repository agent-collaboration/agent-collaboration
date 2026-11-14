use super::*;
use collaboration_protocol::central_mail::{MessageBody, MessageSender, NoticeItem};
use router_clock::ClockInstant;
use std::time::Duration;

#[tokio::test]
async fn first_activity_deadline_collates_three_ordered_posts_at_fifteen_minutes() {
    let f = Fixture::new().await;
    let area = f.area(7250).await;
    let thread = f.thread(7251, area).await;
    let (_, recipient) = joined_observer(&f, &thread).await;
    let runtime = f.host.central(&f.caller.organization_id).await.unwrap();
    let actors = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT actor_identity_id,identity FROM actor_identities",
            args: vec![],
        })
        .await
        .unwrap();
    let author: m::ActorIdentityId = actors
        .rows
        .iter()
        .find_map(|row| match (&row[0], &row[1]) {
            (SqlValue::Text(id), SqlValue::Text(identity)) if identity.contains("test-session") => {
                Some(id.parse().unwrap())
            }
            _ => None,
        })
        .unwrap();
    let first_time = f.clock.utc_now();
    let mut expected = Vec::new();
    let mut initial_deadline = None;
    for (request, advance) in [(7252, 0), (7253, 300), (7254, 420)] {
        if advance != 0 {
            f.clock.advance(Duration::from_secs(advance)).unwrap();
        }
        let reply = f.call(request, post(&thread, vec![])).await;
        let m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { item, position }) = reply.result
        else {
            panic!("post refused: {reply:?}")
        };
        let change = runtime
            .inspect_workspace(
                &f.workspace,
                Statement {
                    sql: "SELECT change_id FROM changes WHERE position=?",
                    args: vec![SqlValue::Integer(
                        i64::try_from(position.get()).unwrap().try_into().unwrap(),
                    )],
                },
            )
            .await
            .unwrap();
        let SqlValue::Text(change_id) = &change.rows[0][0] else {
            panic!("change id")
        };
        expected.push(NoticeItem {
            change_id: change_id.parse().unwrap(),
            position,
            item,
            actor: author.clone(),
            event: m::ChangeEvent::Posted,
        });
        let window = runtime.statement_host().read(Statement { sql: "SELECT first_activity_at,due_at,(SELECT COUNT(*) FROM notification_window_items i WHERE i.window_id=w.window_id) FROM notification_windows w WHERE recipient_actor=? AND thread_id=?", args: vec![text(&recipient), text(&thread)] }).await.unwrap();
        assert_eq!(window.rows.len(), 1);
        let (SqlValue::Text(start), SqlValue::Text(due)) = (&window.rows[0][0], &window.rows[0][1])
        else {
            panic!("window timestamps")
        };
        let start = chrono::DateTime::parse_from_rfc3339(start).unwrap();
        let due = chrono::DateTime::parse_from_rfc3339(due).unwrap();
        assert_eq!(start.timestamp_millis(), first_time.timestamp_millis());
        assert_eq!(due - start, chrono::Duration::minutes(15));
        if let Some(original) = &initial_deadline {
            assert_eq!(&window.rows[0][1], original);
        } else {
            initial_deadline = Some(window.rows[0][1].clone());
        }
        assert_eq!(
            window.rows[0][2],
            SqlValue::Integer(i64::try_from(expected.len()).unwrap().try_into().unwrap())
        );
        let queued = runtime.statement_host().read(Statement { sql: "SELECT COUNT(*) FROM messages WHERE recipient_actor=? AND source_thread_id=? AND message_kind='notice'", args: vec![text(&recipient), text(&thread)] }).await.unwrap();
        assert_eq!(
            queued.rows,
            vec![vec![SqlValue::Integer(0.try_into().unwrap())]]
        );
    }
    let mut fired = runtime
        .pause_points()
        .hold_central(PausePoint::AfterTimerCommit);
    f.clock
        .not_due_check_now(ClockInstant::ZERO.after(Duration::from_secs(900)))
        .await;
    f.clock.advance(Duration::from_secs(180)).unwrap();
    fired.until_held().await;
    let messages = runtime.statement_host().read(Statement { sql: "SELECT sender,body,source_change_id FROM messages WHERE recipient_actor=? AND source_thread_id=? AND message_kind='notice' AND state='queued'", args: vec![text(&recipient), text(&thread)] }).await.unwrap();
    assert_eq!(messages.rows.len(), 1);
    let (SqlValue::Text(sender), SqlValue::Text(body)) =
        (&messages.rows[0][0], &messages.rows[0][1])
    else {
        panic!("notice encoding")
    };
    assert_eq!(
        serde_json::from_str::<MessageSender>(sender).unwrap(),
        MessageSender::Central
    );
    assert_eq!(
        serde_json::from_str::<MessageBody>(body).unwrap(),
        MessageBody::Notice {
            items: expected.clone()
        }
    );
    assert_eq!(messages.rows[0][2], text(&expected[0].change_id));
    let remaining = runtime.statement_host().read(Statement { sql: "SELECT (SELECT COUNT(*) FROM notification_windows WHERE recipient_actor=? AND thread_id=?), (SELECT COUNT(*) FROM notification_window_items i JOIN notification_windows w ON w.window_id=i.window_id WHERE w.recipient_actor=? AND w.thread_id=?)", args: vec![text(&recipient), text(&thread), text(&recipient), text(&thread)] }).await.unwrap();
    assert_eq!(
        remaining.rows,
        vec![vec![
            SqlValue::Integer(0.try_into().unwrap()),
            SqlValue::Integer(0.try_into().unwrap())
        ]]
    );
    fired.release();
    f.host.close().await;
}
