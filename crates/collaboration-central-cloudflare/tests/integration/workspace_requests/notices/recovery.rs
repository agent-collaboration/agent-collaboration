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
