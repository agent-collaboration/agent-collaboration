//! Native notice decisions and durable drain coordination.
use super::{SqlValue, Statement, support::*};
use collaboration_central_core::WorkspaceCaller;
use std::sync::atomic::Ordering;
use workspace_model as m;

async fn joined_observer(
    f: &Fixture,
    thread: &m::ThreadId,
) -> (WorkspaceCaller, m::ActorIdentityId) {
    let mut observer = f.caller.clone();
    let mut address = session(&observer);
    address.session_id = "notice-observer".to_owned().try_into().unwrap();
    observer.caller = m::Caller::Session(address);
    let joined = call(
        &f.host,
        &observer,
        Some(f.workspace.clone()),
        7100,
        m::WorkspaceOperation::JoinThread {
            thread_id: thread.clone(),
            role: m::MemberRole::Observer,
        },
    )
    .await;
    assert!(matches!(
        joined.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    let runtime = f.host.central(&f.caller.organization_id).await;
    let actors = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT actor_identity_id,identity FROM actor_identities",
            args: vec![],
        })
        .await
        .unwrap();
    let actor = actors
        .rows
        .iter()
        .find_map(|row| match (&row[0], &row[1]) {
            (SqlValue::Text(id), SqlValue::Text(identity))
                if identity.contains("notice-observer") =>
            {
                Some(id.parse().unwrap())
            }
            _ => None,
        })
        .unwrap();
    (observer, actor)
}

fn post(thread: &m::ThreadId, addressees: Vec<m::Addressee>) -> m::WorkspaceOperation {
    m::WorkspaceOperation::ThreadPost {
        thread_id: thread.clone(),
        body: "Notice activity".to_owned().try_into().unwrap(),
        addressees,
    }
}

async fn posted_notice_item(
    f: &Fixture,
    request: u32,
    thread: &m::ThreadId,
    author: &m::ActorIdentityId,
    addressees: Vec<m::Addressee>,
) -> collaboration_protocol::central_mail::NoticeItem {
    let reply = f.call(request, post(thread, addressees)).await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { item, position }) = reply.result
    else {
        panic!("post refused: {reply:?}")
    };
    let runtime = f.host.central(&f.caller.organization_id).await;
    let rows = runtime
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
    let SqlValue::Text(change_id) = &rows.rows[0][0] else {
        panic!("change id")
    };
    collaboration_protocol::central_mail::NoticeItem {
        workspace_id: f.workspace.clone(),
        thread_id: Some(thread.clone()),
        change_id: change_id.parse().unwrap(),
        position,
        item,
        actor: author.clone(),
        event: m::ChangeEvent::Posted,
    }
}

#[tokio::test]
async fn first_activity_fixes_window_and_addressed_posts_join_the_session_batch() {
    let f = Fixture::new().await;
    let area = f.area(7101).await;
    let thread = f.thread(7102, area).await;
    let (_, actor) = joined_observer(&f, &thread).await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    let actors = runtime
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
    let first_time =
        chrono::DateTime::from_timestamp_millis(f.clock.0.load(Ordering::SeqCst)).unwrap();
    let mut expected = vec![posted_notice_item(&f, 7103, &thread, &author, vec![]).await];
    let read_window = || Statement {
        sql: "SELECT first_activity_at,due_at,(SELECT COUNT(*) FROM notification_window_items i WHERE i.window_id=w.window_id) FROM notification_windows w WHERE recipient_actor=? AND window_id IN (SELECT window_id FROM notification_window_items WHERE thread_id=?)",
        args: vec![text(&actor), text(&thread)],
    };
    let first = runtime.statement_host().read(read_window()).await.unwrap();
    assert_eq!(first.rows.len(), 1);
    let (SqlValue::Text(start), SqlValue::Text(due)) = (&first.rows[0][0], &first.rows[0][1])
    else {
        panic!("window timestamps")
    };
    let start = chrono::DateTime::parse_from_rfc3339(start).unwrap();
    let due = chrono::DateTime::parse_from_rfc3339(due).unwrap();
    assert_eq!(start.timestamp_millis(), first_time.timestamp_millis());
    assert_eq!(due - start, chrono::Duration::minutes(15));
    assert_eq!(first.rows[0][2], SqlValue::Integer(1.try_into().unwrap()));
    f.clock.0.fetch_add(300_000, Ordering::SeqCst);
    expected.push(posted_notice_item(&f, 7104, &thread, &author, vec![]).await);
    let second = runtime.statement_host().read(read_window()).await.unwrap();
    assert_eq!(&second.rows[0][..2], &first.rows[0][..2]);
    assert_eq!(second.rows[0][2], SqlValue::Integer(2.try_into().unwrap()));
    f.clock.0.fetch_add(420_000, Ordering::SeqCst);
    expected.push(posted_notice_item(&f, 7106, &thread, &author, vec![]).await);
    let third = runtime.statement_host().read(read_window()).await.unwrap();
    assert_eq!(&third.rows[0][..2], &first.rows[0][..2]);
    assert_eq!(third.rows[0][2], SqlValue::Integer(3.try_into().unwrap()));
    expected.push(
        posted_notice_item(
            &f,
            7105,
            &thread,
            &author,
            vec![m::Addressee::Actor {
                actor_identity_id: actor.clone(),
            }],
        )
        .await,
    );
    let after_addressed = runtime.statement_host().read(read_window()).await.unwrap();
    assert_eq!(
        after_addressed.rows[0][2],
        SqlValue::Integer(4.try_into().unwrap())
    );
    let messages = runtime.statement_host().read(Statement { sql: "SELECT COUNT(*) FROM messages WHERE recipient_actor=? AND source_thread_id=? AND message_kind='notice' AND state='queued'", args: vec![text(&actor), text(&thread)] }).await.unwrap();
    assert_eq!(
        messages.rows,
        vec![vec![SqlValue::Integer(0.try_into().unwrap())]]
    );
    assert_eq!(runtime.on_alarm().await.unwrap().windows_fired, 0);
    f.clock.0.fetch_add(180_000, Ordering::SeqCst);
    // The observer's join also opened a window for the thread creator.
    assert_eq!(runtime.on_alarm().await.unwrap().windows_fired, 2);
    assert_eq!(runtime.on_alarm().await.unwrap().windows_fired, 0);
    let messages = runtime.statement_host().read(Statement { sql: "SELECT sender,body,source_change_id FROM messages WHERE recipient_actor=? AND source_thread_id=? AND message_kind='notice' AND source_change_id=?", args: vec![text(&actor), text(&thread), text(&expected[0].change_id)] }).await.unwrap();
    assert_eq!(messages.rows.len(), 1);
    let (SqlValue::Text(sender), SqlValue::Text(body)) =
        (&messages.rows[0][0], &messages.rows[0][1])
    else {
        panic!("notice body")
    };
    assert_eq!(
        serde_json::from_str::<collaboration_protocol::central_mail::MessageSender>(sender)
            .unwrap(),
        collaboration_protocol::central_mail::MessageSender::Central
    );
    assert_eq!(
        serde_json::from_str::<collaboration_protocol::central_mail::MessageBody>(body).unwrap(),
        collaboration_protocol::central_mail::MessageBody::Notice {
            items: expected.clone()
        }
    );
    assert_eq!(messages.rows[0][2], text(&expected[0].change_id));
    f.host.close().await;
}

#[tokio::test]
async fn leave_notifications_off_and_resolution_cancel_notices_but_keep_direct_mail() {
    use collaboration_protocol::central_mail::{
        CentralMailOperation, CentralMailRequest, CentralMailResult,
    };
    for action in 0..3 {
        let f = Fixture::new().await;
        let area = f.area(7120).await;
        let thread = f.thread(7121, area).await;
        let (observer, actor) = joined_observer(&f, &thread).await;
        created(f.call(7122, post(&thread, vec![])).await);
        created(
            f.call(
                7123,
                post(
                    &thread,
                    vec![m::Addressee::Actor {
                        actor_identity_id: actor.clone(),
                    }],
                ),
            )
            .await,
        );
        let sent = f
            .host
            .request_central(
                &f.caller,
                CentralMailRequest {
                    request_id: id(7124),
                    operation: CentralMailOperation::SendMessage {
                        to: session(&observer),
                        body: "Direct message".to_owned().try_into().unwrap(),
                    },
                },
            )
            .await
            .unwrap();
        assert!(matches!(sent, CentralMailResult::Done(_)));
        let runtime = f.host.central(&f.caller.organization_id).await;
        let before = runtime.statement_host().read(Statement { sql: "SELECT (SELECT COUNT(*) FROM notification_window_items i JOIN notification_windows w ON w.window_id=i.window_id WHERE w.recipient_actor=? AND i.thread_id=?), (SELECT COUNT(*) FROM messages WHERE recipient_actor=? AND source_thread_id=? AND message_kind='notice' AND state='queued'), (SELECT COUNT(*) FROM messages WHERE message_kind='direct' AND state='queued')", args: vec![text(&actor), text(&thread), text(&actor), text(&thread)] }).await.unwrap();
        assert_eq!(
            before.rows,
            vec![vec![
                SqlValue::Integer(2.try_into().unwrap()),
                SqlValue::Integer(0.try_into().unwrap()),
                SqlValue::Integer(1.try_into().unwrap())
            ]]
        );
        let (caller, operation) = match action {
            0 => (
                &observer,
                m::WorkspaceOperation::LeaveThread {
                    thread_id: thread.clone(),
                },
            ),
            1 => (
                &observer,
                m::WorkspaceOperation::SetNotifications {
                    thread_id: thread.clone(),
                    setting: m::NotificationSetting::Off,
                },
            ),
            _ => (
                &f.caller,
                m::WorkspaceOperation::ResolveThread {
                    thread_id: thread.clone(),
                },
            ),
        };
        let result = call(&f.host, caller, Some(f.workspace.clone()), 7125, operation).await;
        assert!(matches!(
            result.result,
            m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
        ));
        let after = runtime.statement_host().read(Statement { sql: "SELECT (SELECT COUNT(*) FROM notification_windows WHERE recipient_actor=? AND window_id IN (SELECT window_id FROM notification_window_items WHERE thread_id=?)), (SELECT COUNT(*) FROM notification_window_items i JOIN notification_windows w ON w.window_id=i.window_id WHERE w.recipient_actor=? AND i.thread_id=?), (SELECT COUNT(*) FROM messages WHERE recipient_actor=? AND source_thread_id=? AND message_kind='notice' AND state='cancelled'), (SELECT COUNT(*) FROM messages WHERE message_kind='direct' AND state='queued')", args: vec![text(&actor), text(&thread), text(&actor), text(&thread), text(&actor), text(&thread)] }).await.unwrap();
        assert_eq!(
            after.rows,
            vec![vec![
                SqlValue::Integer(0.try_into().unwrap()),
                SqlValue::Integer(0.try_into().unwrap()),
                SqlValue::Integer(0.try_into().unwrap()),
                SqlValue::Integer(1.try_into().unwrap())
            ]]
        );
        f.host.close().await;
    }
}

#[tokio::test]
async fn session_cadence_sets_nondefault_window_deadline() {
    use collaboration_protocol::central_mail::{
        CentralMailAnswer, CentralMailOperation, CentralMailRequest, CentralMailResult,
    };
    let f = Fixture::new().await;
    let area = f.area(7140).await;
    let thread = f.thread(7141, area).await;
    let (observer, actor) = joined_observer(&f, &thread).await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    let defaults = runtime.statement_host().read(Statement {
        sql: "SELECT notice_cadence,last_notice_delivery_at FROM actor_identities WHERE actor_identity_id=?", args: vec![text(&actor)],
    }).await.unwrap();
    assert_eq!(defaults.rows, vec![vec![text("m15"), SqlValue::Null]]);
    let changed = f
        .host
        .request_central(
            &observer,
            CentralMailRequest {
                request_id: id(7143),
                operation: CentralMailOperation::SetNoticeCadence {
                    cadence: m::NoticeCadence::M25,
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(
        changed,
        CentralMailResult::Done(CentralMailAnswer::NoticeCadenceSet {
            cadence: m::NoticeCadence::M25
        })
    );
    let first_time =
        chrono::DateTime::from_timestamp_millis(f.clock.0.load(Ordering::SeqCst)).unwrap();
    created(f.call(7142, post(&thread, vec![])).await);
    let rows = runtime.statement_host().read(Statement {
        sql: "SELECT first_activity_at,due_at FROM notification_windows WHERE recipient_actor=?", args: vec![text(&actor)],
    }).await.unwrap();
    assert_eq!(rows.rows.len(), 1);
    assert_eq!(
        rows.rows[0],
        vec![
            text(first_time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
            text(
                (first_time + chrono::Duration::minutes(25))
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
            )
        ]
    );
    f.host.close().await;
}

mod roles;

mod concurrency;
mod corruption;
mod identities;
mod queued_cancel;
mod recovery;
