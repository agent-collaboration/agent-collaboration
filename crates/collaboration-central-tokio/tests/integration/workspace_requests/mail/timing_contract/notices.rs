//! Session-wide cadence and per-item cancellation through owning requests.
use super::*;

pub(super) async fn exercise_notice_cadences(f: &mut Fixture) {
    let mut recipients = Vec::new();
    for (index, (cadence, seconds)) in [
        (m::NoticeCadence::M15, 900_i64),
        (m::NoticeCadence::M25, 1500),
        (m::NoticeCadence::M55, 3300),
    ]
    .into_iter()
    .enumerate()
    {
        let mut caller = f.caller.clone();
        let mut address = session(&caller);
        address.session_id = format!("cadence-{index}").try_into().unwrap();
        caller.caller = m::Caller::Session(address.clone());
        request(
            f,
            &caller,
            41000 + u32::try_from(index).unwrap(),
            Op::SetNoticeCadence { cadence },
        )
        .await;
        recipients.push((caller, address, cadence, seconds));
    }
    let created_workspace = call(
        &f.host,
        &f.caller,
        None,
        41005,
        m::WorkspaceOperation::CreateWorkspace {
            name: name("Second cadence workspace"),
        },
    )
    .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceCreated { workspace: second }) =
        created_workspace.result
    else {
        panic!("second workspace")
    };
    let r = f.host.central(&f.caller.organization_id).await.unwrap();
    use central_storage::{AlarmUpdate, GuardedStatement, RowExpectation, StatementBatch};
    assert!(matches!(
        r.statement_host()
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement {
                        sql: "UPDATE machines SET allowed_workspaces=? WHERE machine_id=?",
                        args: vec![
                            text(
                                serde_json::to_string(&vec![
                                    f.workspace.clone(),
                                    second.workspace_id.clone()
                                ])
                                .unwrap()
                            ),
                            text(f.caller.caller.machine_id())
                        ]
                    },
                    expect: RowExpectation::Exactly(1)
                }],
                alarm: AlarmUpdate::Unchanged
            })
            .await
            .unwrap(),
        central_storage::BatchOutcome::Applied { .. }
    ));
    drop(r);
    let area = f.area(41010).await;
    let second_area = created(
        call(
            &f.host,
            &f.caller,
            Some(second.workspace_id.clone()),
            41011,
            m::WorkspaceOperation::CreateArea {
                name: name("Second cadence area"),
            },
        )
        .await,
    );
    let m::ItemRef::Area(second_area) = second_area else {
        panic!("second area")
    };
    let mut threads = Vec::new();
    for index in 0..10_u32 {
        let workspace = if index < 5 {
            f.workspace.clone()
        } else {
            second.workspace_id.clone()
        };
        let area = if index < 5 {
            area.clone()
        } else {
            second_area.clone()
        };
        let thread = created(
            call(
                &f.host,
                &f.caller,
                Some(workspace.clone()),
                41020 + index,
                m::WorkspaceOperation::CreateThread {
                    area_id: area,
                    title: name(&format!("Cadence thread {index}")),
                },
            )
            .await,
        );
        let m::ItemRef::Thread(thread) = thread else {
            panic!("thread")
        };
        for (recipient_index, (caller, _, _, _)) in recipients.iter().enumerate() {
            let reply = call(
                &f.host,
                caller,
                Some(workspace.clone()),
                41100 + index * 3 + u32::try_from(recipient_index).unwrap(),
                m::WorkspaceOperation::JoinThread {
                    thread_id: thread.clone(),
                    role: m::MemberRole::Observer,
                },
            )
            .await;
            assert!(matches!(reply.result, m::WorkspaceResult::Done(_)));
        }
        threads.push((workspace, thread));
    }
    let start = now(f);
    let mut post_ids = Vec::new();
    for (index, (workspace, thread)) in threads.iter().enumerate() {
        advance_to(
            f,
            start + chrono::Duration::seconds(i64::try_from(index).unwrap() * 10),
        );
        let item = created(
            call(
                &f.host,
                &f.caller,
                Some(workspace.clone()),
                41200 + u32::try_from(index).unwrap(),
                m::WorkspaceOperation::ThreadPost {
                    thread_id: thread.clone(),
                    body: body(),
                    addressees: vec![],
                },
            )
            .await,
        );
        post_ids.push((workspace.clone(), item));
    }
    let r = f.host.central(&f.caller.organization_id).await.unwrap();
    let windows=r.statement_host().read(Statement{sql:"SELECT COUNT(*) FROM notification_windows WHERE recipient_actor IN (SELECT actor_identity_id FROM actor_identities WHERE identity LIKE '%cadence-%')",args:vec![]}).await.unwrap();
    assert_eq!(
        windows.rows,
        vec![vec![central_storage::SqlValue::Integer(
            3.try_into().unwrap()
        )]]
    );
    for (_, address, _, _) in &recipients {
        assert!(
            r.due_for_session(address, start)
                .await
                .unwrap()
                .messages
                .is_empty()
        );
    }
    let drains = r
        .statement_host()
        .read(Statement {
            sql: "SELECT COUNT(*) FROM workspace_drains WHERE drain_due_at IS NOT NULL",
            args: vec![],
        })
        .await
        .unwrap();
    assert_eq!(
        drains.rows,
        vec![vec![central_storage::SqlValue::Integer(
            0.try_into().unwrap()
        )]],
        "normal writes clear their safety preset"
    );
    let safety_mark = now(f) + chrono::Duration::seconds(5);
    let callbacks = r.alarm_completion_count();
    assert!(r.statement_host().alarm().unwrap().unix_millis() > safety_mark.timestamp_millis());
    f.clock
        .not_due_check_now(f.clock.wall_deadline(r.statement_host().alarm().unwrap()))
        .await;
    advance_to(f, safety_mark);
    assert_eq!(r.alarm_completion_count(), callbacks);
    drop(r);
    f.reopen().await;
    for (recipient_index, (_, address, _, seconds)) in recipients.iter().enumerate() {
        let due = chrono::DateTime::from_timestamp(
            (start.timestamp() + *seconds + 299).div_euclid(300) * 300,
            0,
        )
        .unwrap();
        dispatch_to(f, due).await;
        let r = f.host.central(&f.caller.organization_id).await.unwrap();
        let batch = r.due_for_session(address, due).await.unwrap();
        assert_eq!(
            batch.messages.len(),
            1,
            "one batch across ten followed threads: {batch:?}"
        );
        let MessageBody::Notice { items } = &batch.messages[0].body else {
            panic!("notice body")
        };
        assert_eq!(items.len(), [30, 20, 10][recipient_index]);
        assert!(items.iter().all(|item| {
            threads.iter().any(|(workspace, thread)| {
                &item.workspace_id == workspace && item.thread_id.as_ref() == Some(thread)
            })
        }));
        for (workspace, item) in &post_ids {
            assert_eq!(
                items
                    .iter()
                    .filter(|notice| &notice.item == item && &notice.workspace_id == workspace)
                    .count(),
                1
            );
        }
        assert_eq!(batch.messages[0].deliver_after, due);
    }
    // Change M15 to M55 while a second complete cohort is pending, then reopen.
    advance_to(f, now(f) + chrono::Duration::seconds(1));
    let mut second_posts = Vec::new();
    for (index, (workspace, thread)) in threads.iter().enumerate() {
        let post = created(
            call(
                &f.host,
                &f.caller,
                Some(workspace.clone()),
                41300 + u32::try_from(index).unwrap(),
                m::WorkspaceOperation::ThreadPost {
                    thread_id: thread.clone(),
                    body: body(),
                    addressees: vec![],
                },
            )
            .await,
        );
        second_posts.push((workspace.clone(), thread.clone(), post));
    }
    let original_last =
        chrono::DateTime::from_timestamp((start.timestamp() + 900 + 299).div_euclid(300) * 300, 0)
            .unwrap();
    let changed_due = original_last + chrono::Duration::seconds(3300);
    assert!(matches!(
        request(
            f,
            &recipients[0].0,
            41380,
            Op::SetNoticeCadence {
                cadence: m::NoticeCadence::M55
            }
        )
        .await,
        Result::Done(Answer::NoticeCadenceSet {
            cadence: m::NoticeCadence::M55
        })
    ));
    let read_policy = || Statement {
        sql: "SELECT a.notice_cadence,a.last_notice_delivery_at,w.due_at FROM actor_identities a JOIN notification_windows w ON w.recipient_actor=a.actor_identity_id WHERE a.identity LIKE '%cadence-0%'",
        args: vec![],
    };
    let r = f.host.central(&f.caller.organization_id).await.unwrap();
    let expected_policy = vec![vec![
        text("m55"),
        text(original_last.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        text(changed_due.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
    ]];
    assert_eq!(
        r.statement_host().read(read_policy()).await.unwrap().rows,
        expected_policy
    );
    drop(r);
    f.reopen().await;
    let r = f.host.central(&f.caller.organization_id).await.unwrap();
    assert_eq!(
        r.statement_host().read(read_policy()).await.unwrap().rows,
        expected_policy
    );
    assert!(
        r.due_for_session(&recipients[0].1, now(f))
            .await
            .unwrap()
            .messages
            .len()
            == 1
    );
    drop(r);
    dispatch_to(f, changed_due).await;
    let r = f.host.central(&f.caller.organization_id).await.unwrap();
    let before = notice_contents(r.due_for_session(&recipients[0].1, now(f)).await.unwrap());
    assert_eq!(before.len(), 2);
    let second_items = &before[1].2;
    assert_eq!(second_items.len(), 10);
    for (workspace, thread, post) in &second_posts {
        assert_eq!(
            second_items
                .iter()
                .filter(|item| &item.workspace_id == workspace
                    && item.thread_id.as_ref() == Some(thread)
                    && &item.item == post)
                .count(),
            1
        );
    }
    drop(r);
    // Leave, switch notifications off, and resolve each remove exactly their source
    // while every other item, message id, due mark and workspace remains intact.
    for action in 0..3_u32 {
        let (workspace, thread) = &threads[usize::try_from(action).unwrap()];
        let operation = match action {
            0 => m::WorkspaceOperation::LeaveThread {
                thread_id: thread.clone(),
            },
            1 => m::WorkspaceOperation::SetNotifications {
                thread_id: thread.clone(),
                setting: m::NotificationSetting::Off,
            },
            _ => m::WorkspaceOperation::ResolveThread {
                thread_id: thread.clone(),
            },
        };
        let caller = if action == 2 {
            &f.caller
        } else {
            &recipients[0].0
        };
        let r = f.host.central(&f.caller.organization_id).await.unwrap();
        let before = notice_contents(r.due_for_session(&recipients[0].1, now(f)).await.unwrap());
        drop(r);
        let reply = call(
            &f.host,
            caller,
            Some(workspace.clone()),
            41400 + action,
            operation,
        )
        .await;
        assert!(matches!(reply.result, m::WorkspaceResult::Done(_)));
        let r = f.host.central(&f.caller.organization_id).await.unwrap();
        let after = notice_contents(r.due_for_session(&recipients[0].1, now(f)).await.unwrap());
        assert_retained_notices(before, after, workspace, thread);
    }
}
fn notice_contents(
    batch: collaboration_protocol::central_mail::SessionDueBatch,
) -> Vec<(
    workspace_ids::MessageId,
    m::Timestamp,
    Vec<collaboration_protocol::central_mail::NoticeItem>,
)> {
    batch
        .messages
        .into_iter()
        .map(|message| {
            let MessageBody::Notice { items } = message.body else {
                panic!("unexpected non-notice in cadence session")
            };
            (message.message_id, message.deliver_after, items)
        })
        .collect()
}
fn assert_retained_notices(
    before: Vec<(
        workspace_ids::MessageId,
        m::Timestamp,
        Vec<collaboration_protocol::central_mail::NoticeItem>,
    )>,
    after: Vec<(
        workspace_ids::MessageId,
        m::Timestamp,
        Vec<collaboration_protocol::central_mail::NoticeItem>,
    )>,
    workspace: &m::WorkspaceId,
    thread: &m::ThreadId,
) {
    let expected: Vec<_> = before
        .into_iter()
        .map(|(id, due, mut items)| {
            items.retain(|item| {
                &item.workspace_id != workspace || item.thread_id.as_ref() != Some(thread)
            });
            assert!(!items.is_empty());
            (id, due, items)
        })
        .collect();
    assert!(!expected.is_empty());
    assert!(
        expected
            .iter()
            .flat_map(|(_, _, items)| items)
            .any(|item| &item.workspace_id != workspace)
    );
    assert_eq!(after, expected);
}
