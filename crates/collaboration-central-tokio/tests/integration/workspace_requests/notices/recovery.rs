use super::*;

#[tokio::test]
async fn drain_commit_crash_cuts_replay_without_loss_or_duplicate() {
    use collaboration_protocol::central_mail::MessageBody;
    use router_clock::ClockInstant;
    use std::time::Duration;
    for point in [PausePoint::BeforeDrainCommit, PausePoint::AfterDrainCommit] {
        let mut f = Fixture::new().await;
        let area = f.area(7210).await;
        let thread = f.thread(7211, area).await;
        let (_, actor) = joined_observer(&f, &thread).await;
        let runtime = f.host.central(&f.caller.organization_id).await.unwrap();
        let workspace_id = f.workspace.clone();
        let read_cursor = || Statement {
            sql: "SELECT drained_through_position,drain_due_at FROM workspace_drains WHERE workspace_id=?",
            args: vec![text(&workspace_id)],
        };
        let initial = runtime.statement_host().read(read_cursor()).await.unwrap();
        let SqlValue::Integer(initial_position) = &initial.rows[0][0] else {
            panic!("initial cursor")
        };
        let initial_position = *initial_position;
        assert_eq!(initial.rows[0][1], SqlValue::Null);
        for request_id in 7212..=7214 {
            let mut applied = runtime
                .pause_points()
                .hold(&f.workspace, PausePoint::AfterStoreApplied);
            let pending = tokio::spawn({
                let runtime = runtime.clone();
                let caller = f.caller.clone();
                let workspace = f.workspace.clone();
                let operation = post(
                    &thread,
                    vec![m::Addressee::Actor {
                        actor_identity_id: actor.clone(),
                    }],
                );
                async move {
                    runtime
                        .request(
                            &caller,
                            m::WorkspaceRequest {
                                request_id: Some(id(request_id)),
                                workspace_id: Some(workspace),
                                operation,
                            },
                        )
                        .await
                }
            });
            applied.until_held().await;
            pending.abort();
            let _ = pending.await;
            drop(applied);
        }
        let changes = runtime.inspect_workspace(&f.workspace, Statement { sql: "SELECT change_id,position FROM changes WHERE position>? ORDER BY position", args: vec![SqlValue::Integer(initial_position)] }).await.unwrap();
        assert_eq!(changes.rows.len(), 3);
        for (offset, row) in changes.rows.iter().enumerate() {
            assert_eq!(
                row[1],
                SqlValue::Integer(
                    (initial_position.get() + i64::try_from(offset).unwrap() + 1)
                        .try_into()
                        .unwrap()
                )
            );
        }
        let ledger = runtime
            .inspect_workspace(
                &f.workspace,
                Statement {
                    sql: "SELECT COUNT(*) FROM request_log WHERE request_id IN (?,?,?)",
                    args: vec![
                        text(id::<m::RequestId>(7212)),
                        text(id::<m::RequestId>(7213)),
                        text(id::<m::RequestId>(7214)),
                    ],
                },
            )
            .await
            .unwrap();
        assert_eq!(
            ledger.rows,
            vec![vec![SqlValue::Integer(3.try_into().unwrap())]]
        );
        let before = runtime.statement_host().read(read_cursor()).await.unwrap();
        assert_eq!(before.rows[0][0], SqlValue::Integer(initial_position));
        assert!(matches!(&before.rows[0][1], SqlValue::Text(_)));
        let final_position = SqlValue::Integer((initial_position.get() + 3).try_into().unwrap());
        let mut held = runtime.pause_points().hold(&f.workspace, point);
        let pending = tokio::spawn({
            let runtime = runtime.clone();
            let workspace = f.workspace.clone();
            async move { runtime.drain_workspace(&workspace).await }
        });
        held.until_held().await;
        let read_messages = || Statement {
            sql: "SELECT i.change_id,w.recipient_actor FROM notification_window_items i JOIN notification_windows w ON w.window_id=i.window_id WHERE w.recipient_actor=? AND i.thread_id=?",
            args: vec![text(&actor), text(&thread)],
        };
        let cut_cursor = runtime.statement_host().read(read_cursor()).await.unwrap();
        let cut_messages = runtime
            .statement_host()
            .read(read_messages())
            .await
            .unwrap();
        let before_commit = matches!(point, PausePoint::BeforeDrainCommit);
        if before_commit {
            assert_eq!(cut_cursor.rows, before.rows);
            assert!(cut_messages.rows.is_empty());
        } else {
            assert_eq!(
                cut_cursor.rows,
                vec![vec![final_position.clone(), SqlValue::Null]]
            );
            assert_eq!(cut_messages.rows.len(), 3);
            let mut actual: Vec<_> = cut_messages
                .rows
                .iter()
                .map(|row| (row[0].clone(), row[1].clone()))
                .collect();
            let mut expected: Vec<_> = changes
                .rows
                .iter()
                .map(|row| (row[0].clone(), text(&actor)))
                .collect();
            actual.sort_by_key(|entry| format!("{:?}", entry.0));
            expected.sort_by_key(|entry| format!("{:?}", entry.0));
            assert_eq!(actual, expected);
        }
        pending.abort();
        let _ = pending.await;
        drop(held);
        drop(runtime);
        f.reopen().await;
        let reopened = f.host.central(&f.caller.organization_id).await.unwrap();
        assert_eq!(
            reopened
                .statement_host()
                .read(read_cursor())
                .await
                .unwrap()
                .rows,
            cut_cursor.rows
        );
        if before_commit {
            assert!(
                reopened
                    .statement_host()
                    .read(read_messages())
                    .await
                    .unwrap()
                    .rows
                    .is_empty()
            );
            let mut drained = reopened
                .pause_points()
                .hold(&f.workspace, PausePoint::AfterDrainCommit);
            f.clock
                .not_due_check_now(ClockInstant::ZERO.after(Duration::from_secs(5)))
                .await;
            f.clock.advance(Duration::from_secs(5)).unwrap();
            drained.until_held().await;
            assert_eq!(
                reopened
                    .statement_host()
                    .read(read_cursor())
                    .await
                    .unwrap()
                    .rows,
                vec![vec![final_position.clone(), SqlValue::Null]]
            );
            drained.release();
        } else {
            assert_eq!(
                reopened
                    .statement_host()
                    .read(read_messages())
                    .await
                    .unwrap()
                    .rows,
                cut_messages.rows
            );
            let replay = reopened.drain_workspace(&f.workspace).await.unwrap();
            assert_eq!(replay.notices_queued, 0);
        }
        let rows = reopened
            .statement_host()
            .read(read_messages())
            .await
            .unwrap();
        assert_eq!(rows.rows.len(), 3);
        // The recovered window is still pending; release it only at the original cadence mark.
        let due = reopened
            .statement_host()
            .read(Statement {
                sql: "SELECT due_at FROM notification_windows WHERE recipient_actor=?",
                args: vec![text(&actor)],
            })
            .await
            .unwrap();
        let SqlValue::Text(due) = &due.rows[0][0] else {
            panic!("window due")
        };
        let due: m::Timestamp = due.parse().unwrap();
        f.clock.set_utc(due);
        reopened.on_alarm().await.unwrap();
        let batch = reopened
            .due_for_session(
                &m::SessionAddress {
                    session_id: "notice-observer".to_owned().try_into().unwrap(),
                    ..session(&f.caller)
                },
                due,
            )
            .await
            .unwrap();
        let encoded = serde_json::to_vec(&batch).unwrap();
        let batch: collaboration_protocol::central_mail::SessionDueBatch =
            serde_json::from_slice(&encoded).unwrap();
        assert_eq!(batch.messages.len(), 1);
        let MessageBody::Notice { items } = &batch.messages[0].body else {
            panic!("notice body")
        };
        assert_eq!(items.len(), 3);
        let mut delivered = Vec::new();
        for item in items {
            assert_eq!(item.workspace_id, f.workspace);
            assert_eq!(item.thread_id.as_ref(), Some(&thread));
            delivered.push((item.position.get(), item.change_id.to_string()));
        }
        delivered.sort_by_key(|entry| entry.0);
        let expected: Vec<_> = changes
            .rows
            .iter()
            .map(|row| {
                let (SqlValue::Text(id), SqlValue::Integer(position)) = (&row[0], &row[1]) else {
                    panic!("change fields")
                };
                (u64::try_from(position.get()).unwrap(), id.clone())
            })
            .collect();
        assert_eq!(delivered, expected);
        assert_eq!(
            reopened
                .statement_host()
                .read(read_cursor())
                .await
                .unwrap()
                .rows,
            vec![vec![final_position, SqlValue::Null]]
        );
        f.host.close().await;
    }
}

#[tokio::test]
async fn another_workspace_cannot_extend_a_pending_drain_deadline() {
    use central_storage::{
        AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, StatementBatch,
    };
    use std::time::Duration;
    let f = Fixture::new().await;
    let created_workspace = call(
        &f.host,
        &f.caller,
        None,
        7230,
        m::WorkspaceOperation::CreateWorkspace {
            name: name("Second workspace"),
        },
    )
    .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceCreated { workspace }) =
        created_workspace.result
    else {
        panic!("workspace creation")
    };
    let second_workspace = workspace.workspace_id;
    let runtime = f.host.central(&f.caller.organization_id).await.unwrap();
    assert!(matches!(
        runtime
            .statement_host()
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement {
                        sql: "UPDATE machines SET allowed_workspaces=? WHERE machine_id=?",
                        args: vec![
                            text(
                                serde_json::to_string(&vec![
                                    f.workspace.clone(),
                                    second_workspace.clone()
                                ])
                                .unwrap()
                            ),
                            text(f.caller.caller.machine_id())
                        ]
                    },
                    expect: RowExpectation::Exactly(1),
                }],
                alarm: AlarmUpdate::Unchanged
            })
            .await
            .unwrap(),
        BatchOutcome::Applied { .. }
    ));
    let mut held = runtime
        .pause_points()
        .hold(&f.workspace, PausePoint::AfterStoreApplied);
    let pending = tokio::spawn({
        let runtime = runtime.clone();
        let caller = f.caller.clone();
        let workspace = f.workspace.clone();
        async move {
            runtime
                .request(
                    &caller,
                    m::WorkspaceRequest {
                        request_id: Some(id(7231)),
                        workspace_id: Some(workspace),
                        operation: m::WorkspaceOperation::CreateArea {
                            name: name("Pending first workspace"),
                        },
                    },
                )
                .await
                .unwrap()
        }
    });
    held.until_held().await;
    let read_due = || Statement {
        sql: "SELECT drain_due_at FROM workspace_drains WHERE workspace_id=?",
        args: vec![text(&f.workspace)],
    };
    let original = runtime.statement_host().read(read_due()).await.unwrap();
    assert!(matches!(&original.rows[0][0], SqlValue::Text(_)));
    let original_alarm = runtime
        .statement_host()
        .alarm()
        .expect("pending drain alarm");
    let SqlValue::Text(due) = &original.rows[0][0] else {
        panic!("drain deadline")
    };
    assert_eq!(
        chrono::DateTime::parse_from_rfc3339(due)
            .unwrap()
            .timestamp_millis(),
        f.clock.utc_now().timestamp_millis() + 5_000
    );
    assert_eq!(
        original_alarm.unix_millis(),
        f.clock.utc_now().timestamp_millis() + 5_000
    );
    for second in 1..=60 {
        f.clock.advance(Duration::from_secs(1)).unwrap();
        if second == 5 {
            runtime
                .workspace_turns()
                .until_waiting(&f.workspace, 1)
                .await;
        }
        let other = call(
            &f.host,
            &f.caller,
            Some(second_workspace.clone()),
            7300 + second,
            m::WorkspaceOperation::CreateArea {
                name: name(&format!("Second workspace activity {second}")),
            },
        )
        .await;
        assert!(matches!(
            other.result,
            m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { .. })
        ));
        assert_eq!(
            runtime
                .statement_host()
                .read(read_due())
                .await
                .unwrap()
                .rows,
            original.rows
        );
        assert_eq!(
            runtime.statement_host().alarm(),
            Some(original_alarm),
            "W2 moved W1's retained alarm at second {second}"
        );
    }
    let mut drained = runtime
        .pause_points()
        .hold(&f.workspace, PausePoint::AfterDrainCommit);
    pending.abort();
    let _ = pending.await;
    drop(held);
    drained.until_held().await;
    let after = runtime.statement_host().read(Statement { sql: "SELECT drained_through_position,drain_due_at FROM workspace_drains WHERE workspace_id=?", args: vec![text(&f.workspace)] }).await.unwrap();
    assert_eq!(after.rows.len(), 1);
    assert!(matches!(&after.rows[0][0], SqlValue::Integer(position) if position.get() > 0));
    assert_eq!(after.rows[0][1], SqlValue::Null);
    drained.release();
    f.host.close().await;
}

#[tokio::test]
async fn drain_advances_by_position_even_when_clock_and_change_id_move_backwards() {
    let mut f = Fixture::new().await;
    let area = f.area(7240).await;
    let thread = f.thread(7241, area).await;
    let (observer, actor) = joined_observer(&f, &thread).await;
    let runtime = f.host.central(&f.caller.organization_id).await.unwrap();
    let first = f
        .call(
            7242,
            post(
                &thread,
                vec![m::Addressee::Actor {
                    actor_identity_id: actor.clone(),
                }],
            ),
        )
        .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Created {
        item: first_item,
        position: first_position,
    }) = first.result
    else {
        panic!("first post")
    };
    let (_, first_item_id) = first_item.kind_and_id();
    let first_change = runtime.inspect_workspace(&f.workspace, Statement { sql: "SELECT change_id,created_at FROM changes WHERE item_id=? AND change_kind='posted'", args: vec![text(first_item_id)] }).await.unwrap();
    let SqlValue::Text(first_id) = &first_change.rows[0][0] else {
        panic!("first change id")
    };
    let first_id = first_id.clone();
    let prior = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT drained_through_position FROM workspace_drains WHERE workspace_id=?",
            args: vec![text(&f.workspace)],
        })
        .await
        .unwrap();
    let previous_time = f.clock.utc_now();
    f.clock.set_utc(previous_time - chrono::Duration::hours(1));
    drop(runtime);
    f.reopen().await;
    let runtime = f.host.central(&f.caller.organization_id).await.unwrap();
    assert_eq!(
        runtime
            .statement_host()
            .read(Statement {
                sql: "SELECT drained_through_position FROM workspace_drains WHERE workspace_id=?",
                args: vec![text(&f.workspace)]
            })
            .await
            .unwrap()
            .rows,
        prior.rows
    );
    let second = f
        .call(
            7243,
            post(
                &thread,
                vec![m::Addressee::Actor {
                    actor_identity_id: actor.clone(),
                }],
            ),
        )
        .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Created {
        item: second_item,
        position: second_position,
    }) = second.result
    else {
        panic!("second post")
    };
    let (_, second_item_id) = second_item.kind_and_id();
    let stored = runtime
        .inspect_workspace(
            &f.workspace,
            Statement {
                sql: "SELECT position,created_at,change_id FROM changes WHERE item_id=? AND change_kind='posted'",
                args: vec![text(second_item_id)],
            },
        )
        .await
        .unwrap();
    let SqlValue::Integer(previous_position) = &prior.rows[0][0] else {
        panic!("prior cursor")
    };
    let SqlValue::Integer(new_position) = &stored.rows[0][0] else {
        panic!("new position")
    };
    assert!(new_position.get() > previous_position.get());
    assert!(second_position > first_position);
    let SqlValue::Text(later_id) = &stored.rows[0][2] else {
        panic!("second change id")
    };
    assert!(
        later_id < &first_id,
        "clock rollback must produce a smaller change id"
    );
    let SqlValue::Text(created_at) = &stored.rows[0][1] else {
        panic!("change timestamp")
    };
    assert!(
        chrono::DateTime::parse_from_rfc3339(created_at)
            .unwrap()
            .timestamp_millis()
            < previous_time.timestamp_millis()
    );
    let cursor = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT drained_through_position FROM workspace_drains WHERE workspace_id=?",
            args: vec![text(&f.workspace)],
        })
        .await
        .unwrap();
    assert_eq!(cursor.rows[0][0], stored.rows[0][0]);
    let notice = runtime.statement_host().read(Statement { sql: "SELECT i.change_id FROM notification_window_items i JOIN notification_windows w ON w.window_id=i.window_id WHERE w.recipient_actor=? AND i.thread_id=? ORDER BY i.change_id", args: vec![text(&actor), text(&thread)] }).await.unwrap();
    assert_eq!(
        notice.rows,
        vec![vec![text(later_id)], vec![text(first_id)]]
    );
    let inbox = call(
        &f.host,
        &observer,
        Some(f.workspace.clone()),
        0,
        m::WorkspaceOperation::Inbox {
            after: None,
            limit: 100.try_into().unwrap(),
        },
    )
    .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Inbox { entries, .. }) = inbox.result else {
        panic!("observer inbox")
    };
    let post_positions: Vec<_> = entries
        .iter()
        .filter(|entry| entry.position == first_position || entry.position == second_position)
        .map(|entry| entry.position)
        .collect();
    assert_eq!(post_positions, vec![first_position, second_position]);
    f.host.close().await;
}
