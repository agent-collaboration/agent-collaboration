use super::*;

#[tokio::test]
async fn drain_commit_crash_retries_through_cold_platform_alarm() {
    for (point, before) in [("beforeDrainCommit", 0), ("afterDrainCommit", 1)] {
        let f = Fixture::new().await;
        let area = f.area(7210).await;
        let thread = f.thread(7211, area).await;
        let (_, actor) = joined_observer(&f, &thread).await;
        let runtime = f.host.central(&f.caller.organization_id).await;
        let pause = serde_json::json!({"workspace":f.workspace,"point":point});
        let _: bool = runtime.control("hold", &pause).await.unwrap();
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
                            request_id: Some(id(7212)),
                            workspace_id: Some(workspace),
                            operation,
                        },
                    )
                    .await
            }
        });
        let _: bool = runtime.control("until-held", &pause).await.unwrap();
        let rows = runtime.read(Statement { sql: "SELECT COUNT(*) FROM messages WHERE recipient_actor=? AND source_thread_id=? AND message_kind='notice'", args: vec![text(&actor), text(&thread)] }).await.unwrap();
        assert_eq!(
            rows.rows,
            vec![vec![SqlValue::Integer(before.try_into().unwrap())]]
        );
        let reset: std::result::Result<bool, _> = runtime.control("crash", &()).await;
        assert!(reset.is_err());
        pending.abort();
        let _ = pending.await;
        f.clock.0.fetch_add(5_000, Ordering::SeqCst);
        let cold = f.host.cold_runtime(&f.caller.organization_id);
        cold.cold_alarm().await.unwrap();
        let rows = cold.read(Statement { sql: "SELECT COUNT(*) FROM messages WHERE recipient_actor=? AND source_thread_id=? AND message_kind='notice' AND state='queued'", args: vec![text(&actor), text(&thread)] }).await.unwrap();
        assert_eq!(
            rows.rows,
            vec![vec![SqlValue::Integer(1.try_into().unwrap())]]
        );
        f.host.close().await;
    }
}

#[tokio::test]
async fn applied_write_recovers_by_cold_alarm_without_another_request() {
    let f = Fixture::new().await;
    let area = f.area(7130).await;
    let thread = f.thread(7131, area).await;
    let (_, actor) = joined_observer(&f, &thread).await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    let pause = serde_json::json!({"workspace":f.workspace,"point":"afterStoreApplied"});
    let _: bool = runtime.control("hold", &pause).await.unwrap();
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
                        request_id: Some(id(7132)),
                        workspace_id: Some(workspace),
                        operation,
                    },
                )
                .await
        }
    });
    let _: bool = runtime.control("until-held", &pause).await.unwrap();
    let before = runtime
        .read(Statement {
            sql: "SELECT drain_due_at FROM workspace_drains WHERE workspace_id=?",
            args: vec![text(&f.workspace)],
        })
        .await
        .unwrap();
    assert!(matches!(&before.rows[0][0], SqlValue::Text(_)));
    let reset: std::result::Result<bool, _> = runtime.control("crash", &()).await;
    assert!(reset.is_err());
    pending.abort();
    let _ = pending.await;
    f.clock.0.fetch_add(5_000, Ordering::SeqCst);
    let cold = f.host.cold_runtime(&f.caller.organization_id);
    assert_eq!(cold.cold_alarm().await.unwrap().workspaces_drained, 1);
    let rows = cold.read(Statement { sql: "SELECT drain_due_at,(SELECT COUNT(*) FROM messages WHERE recipient_actor=? AND source_thread_id=? AND message_kind='notice' AND state='queued') FROM workspace_drains WHERE workspace_id=?", args: vec![text(&actor), text(&thread), text(&f.workspace)] }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![
            SqlValue::Null,
            SqlValue::Integer(1.try_into().unwrap())
        ]]
    );
    f.host.close().await;
}

#[tokio::test]
async fn restart_behind_clock_drains_by_position_and_exposes_change_in_inbox() {
    let mut f = Fixture::new().await;
    let area = f.area(7240).await;
    let thread = f.thread(7241, area.clone()).await;
    let (observer, actor) = joined_observer(&f, &thread).await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    let earlier: m::ChangeId = id(7243);
    let later: m::ChangeId = id(1);
    assert!(later.to_string() < earlier.to_string());
    f.workspace_batch(
        "UPDATE changes SET change_id=? WHERE item_kind='area' AND item_id=?",
        vec![text(&earlier), text(&area)],
    )
    .await;
    let prior = runtime
        .read(Statement {
            sql: "SELECT drained_through_position FROM workspace_drains WHERE workspace_id=?",
            args: vec![text(&f.workspace)],
        })
        .await
        .unwrap();
    f.clock.0.fetch_sub(3_600_000, Ordering::SeqCst);
    f.reopen().await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    let pause = serde_json::json!({"workspace":f.workspace,"point":"afterStoreApplied"});
    let _: bool = runtime.control("hold", &pause).await.unwrap();
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
                        request_id: Some(id(7242)),
                        workspace_id: Some(workspace),
                        operation,
                    },
                )
                .await
        }
    });
    let _: bool = runtime.control("until-held", &pause).await.unwrap();
    f.workspace_batch(
        "UPDATE changes SET change_id=? WHERE thread_id=? AND change_kind='posted'",
        vec![text(&later), text(&thread)],
    )
    .await;
    let reset: std::result::Result<bool, _> = runtime.control("crash", &()).await;
    assert!(reset.is_err());
    pending.abort();
    let _ = pending.await;
    f.clock.0.fetch_add(5_000, Ordering::SeqCst);
    let cold = f.host.cold_runtime(&f.caller.organization_id);
    assert_eq!(cold.cold_alarm().await.unwrap().workspaces_drained, 1);
    let stored = cold
        .inspect_workspace(
            &f.workspace,
            Statement {
                sql: "SELECT position FROM changes WHERE change_id=?",
                args: vec![text(&later)],
            },
        )
        .await
        .unwrap();
    let (SqlValue::Integer(previous), SqlValue::Integer(position)) =
        (&prior.rows[0][0], &stored.rows[0][0])
    else {
        panic!("positions")
    };
    assert!(position.get() > previous.get());
    let cursor = cold
        .read(Statement {
            sql: "SELECT drained_through_position FROM workspace_drains WHERE workspace_id=?",
            args: vec![text(&f.workspace)],
        })
        .await
        .unwrap();
    assert_eq!(cursor.rows[0][0], stored.rows[0][0]);
    let notice = cold.read(Statement { sql: "SELECT source_change_id FROM messages WHERE recipient_actor=? AND source_thread_id=? AND message_kind='notice'", args: vec![text(&actor), text(&thread)] }).await.unwrap();
    assert_eq!(notice.rows, vec![vec![text(&later)]]);
    let posted_position: m::ChangePosition =
        u64::try_from(position.get()).unwrap().try_into().unwrap();
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
        panic!("inbox refused")
    };
    assert!(
        entries
            .iter()
            .any(|entry| entry.position == posted_position)
    );
    f.host.close().await;
}

#[tokio::test]
async fn drain_crash_cuts_keep_three_change_positions_and_notice_identities_atomic() {
    for (point, committed) in [("beforeDrainCommit", false), ("afterDrainCommit", true)] {
        let f = Fixture::new().await;
        let area = f.area(7600).await;
        let thread = f.thread(7601, area).await;
        let (_, recipient) = joined_observer(&f, &thread).await;
        let runtime = f.host.central(&f.caller.organization_id).await;
        let cursor_query = || Statement {
            sql: "SELECT drained_through_position,drain_due_at FROM workspace_drains WHERE workspace_id=?",
            args: vec![text(&f.workspace)],
        };
        let prior = runtime.read(cursor_query()).await.unwrap();
        let SqlValue::Integer(previous) = prior.rows[0][0] else {
            panic!("cursor")
        };
        assert_eq!(prior.rows[0][1], SqlValue::Null);
        for number in 7602..7605 {
            let pause = serde_json::json!({"workspace":f.workspace,"point":"afterStoreApplied"});
            let _: bool = runtime.control("hold", &pause).await.unwrap();
            let pending = tokio::spawn({
                let runtime = runtime.clone();
                let caller = f.caller.clone();
                let workspace = f.workspace.clone();
                let operation = post(
                    &thread,
                    vec![m::Addressee::Actor {
                        actor_identity_id: recipient.clone(),
                    }],
                );
                async move {
                    runtime
                        .request(
                            &caller,
                            m::WorkspaceRequest {
                                request_id: Some(id(number)),
                                workspace_id: Some(workspace),
                                operation,
                            },
                        )
                        .await
                }
            });
            let _: bool = runtime.control("until-held", &pause).await.unwrap();
            let reset: std::result::Result<bool, _> = runtime.control("crash", &()).await;
            assert!(reset.is_err());
            pending.abort();
            let _ = pending.await;
        }
        let changes = runtime.inspect_workspace(&f.workspace, Statement { sql: "SELECT position,change_id FROM changes WHERE position>? ORDER BY position", args: vec![SqlValue::Integer(previous)] }).await.unwrap();
        assert_eq!(changes.rows.len(), 3);
        for (offset, row) in changes.rows.iter().enumerate() {
            assert_eq!(
                row[0],
                SqlValue::Integer(
                    (previous.get() + i64::try_from(offset).unwrap() + 1)
                        .try_into()
                        .unwrap()
                )
            );
        }
        let pause = serde_json::json!({"workspace":f.workspace,"point":point});
        let _: bool = runtime.control("hold", &pause).await.unwrap();
        let drain = tokio::spawn({
            let runtime = runtime.clone();
            let workspace = f.workspace.clone();
            async move { runtime.drain_workspace(&workspace).await }
        });
        let _: bool = runtime.control("until-held", &pause).await.unwrap();
        let at_cut = runtime.read(cursor_query()).await.unwrap();
        let messages_query = || Statement {
            sql: "SELECT message_id,source_change_id,recipient_actor FROM messages WHERE source_thread_id=? AND recipient_actor=? AND message_kind='notice' ORDER BY source_change_id",
            args: vec![text(&thread), text(&recipient)],
        };
        let at_cut_messages = runtime.read(messages_query()).await.unwrap();
        if committed {
            assert_eq!(at_cut.rows[0][0], changes.rows[2][0]);
            assert_eq!(at_cut.rows[0][1], SqlValue::Null);
            assert_eq!(at_cut_messages.rows.len(), 3);
        } else {
            assert_eq!(at_cut.rows[0][0], prior.rows[0][0]);
            assert!(matches!(at_cut.rows[0][1], SqlValue::Text(_)));
            assert!(at_cut_messages.rows.is_empty());
        }
        let reset: std::result::Result<bool, _> = runtime.control("crash", &()).await;
        assert!(reset.is_err());
        drain.abort();
        let _ = drain.await;
        f.clock.0.fetch_add(5_000, Ordering::SeqCst);
        let cold = f.host.cold_runtime(&f.caller.organization_id);
        let result = cold.cold_alarm().await.unwrap();
        assert_eq!(result.workspaces_drained, usize::from(!committed));
        let after = cold.read(cursor_query()).await.unwrap();
        assert_eq!(after.rows[0][0], changes.rows[2][0]);
        assert_eq!(after.rows[0][1], SqlValue::Null);
        let messages = cold.read(messages_query()).await.unwrap();
        assert_eq!(messages.rows.len(), 3);
        let mut expected_sources: Vec<_> = changes.rows.iter().map(|row| row[1].clone()).collect();
        expected_sources.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
        assert_eq!(
            messages
                .rows
                .iter()
                .map(|row| row[1].clone())
                .collect::<Vec<_>>(),
            expected_sources
        );
        assert!(messages.rows.iter().all(|row| row[2] == text(&recipient)));
        if committed {
            assert_eq!(messages.rows, at_cut_messages.rows);
        }
        cold.drain_workspace(&f.workspace).await.unwrap();
        cold.on_alarm().await.unwrap();
        assert_eq!(
            cold.read(messages_query()).await.unwrap().rows,
            messages.rows
        );
        f.host.close().await;
    }
}
