use super::*;
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, StatementBatch,
};

#[tokio::test]
async fn held_drain_serializes_second_drain_and_source_index_rejects_duplicate() {
    let f = Fixture::new().await;
    let area = f.area(7110).await;
    let thread = f.thread(7111, area).await;
    let (_, actor) = joined_observer(&f, &thread).await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    let pause = serde_json::json!({"workspace":f.workspace,"point":"beforeDrainCommit"});
    let _: bool = runtime.control("hold", &pause).await.unwrap();
    let first = tokio::spawn({
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
                        request_id: Some(id(7112)),
                        workspace_id: Some(workspace),
                        operation,
                    },
                )
                .await
                .unwrap()
        }
    });
    let _: bool = runtime.control("until-held", &pause).await.unwrap();
    let second = tokio::spawn({
        let runtime = runtime.clone();
        let workspace = f.workspace.clone();
        async move { runtime.drain_workspace(&workspace).await.unwrap() }
    });
    let _: bool = runtime
        .control(
            "until-waiting",
            &serde_json::json!({"workspace":f.workspace,"count":1}),
        )
        .await
        .unwrap();
    assert!(!second.is_finished());
    let _: bool = runtime.control("release", &pause).await.unwrap();
    assert!(matches!(
        first.await.unwrap().result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { .. })
    ));
    assert_eq!(second.await.unwrap().notices_queued, 0);
    f.clock.0.fetch_add(900_000, Ordering::SeqCst);
    runtime.on_alarm().await.unwrap();
    let rows = runtime.read(Statement { sql: "SELECT COUNT(*) FROM messages WHERE recipient_actor=? AND source_thread_id=? AND message_kind='notice'", args: vec![text(&actor), text(&thread)] }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![SqlValue::Integer(1.try_into().unwrap())]]
    );
    let duplicate = runtime.run_batch(StatementBatch { statements: vec![GuardedStatement {
        statement: Statement { sql: "INSERT INTO messages(message_id,sender,target_machine_id,target_session,message_kind,body,state,failure_reason,source_change_id,recipient_actor,source_workspace_id,source_thread_id,deliver_after,created_at,updated_at) SELECT ?,sender,target_machine_id,target_session,message_kind,body,state,failure_reason,source_change_id,recipient_actor,source_workspace_id,source_thread_id,deliver_after,created_at,updated_at FROM messages WHERE message_kind='notice' AND source_thread_id=? AND recipient_actor=?", args: vec![text(id::<m::WorkspaceId>(7113)), text(&thread), text(&actor)] },
        expect: RowExpectation::Exactly(1),
    }], alarm: AlarmUpdate::Unchanged }).await;
    assert!(
        matches!(duplicate, Ok(BatchOutcome::ConstraintViolated { .. })),
        "the source/recipient index must reject a duplicate"
    );
    f.host.close().await;
}

#[tokio::test]
async fn sustained_other_workspace_writes_do_not_postpone_pending_drain_alarm() {
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
    let runtime = f.host.central(&f.caller.organization_id).await;
    assert!(matches!(
        runtime
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
    let pause = serde_json::json!({"workspace":f.workspace,"point":"afterStoreApplied"});
    let _: bool = runtime.control("hold", &pause).await.unwrap();
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
    let _: bool = runtime.control("until-held", &pause).await.unwrap();
    let read_due = || Statement {
        sql: "SELECT drain_due_at FROM workspace_drains WHERE workspace_id=?",
        args: vec![text(&f.workspace)],
    };
    let original = runtime.read(read_due()).await.unwrap();
    let SqlValue::Text(due) = &original.rows[0][0] else {
        panic!("missing drain deadline")
    };
    let due = chrono::DateTime::parse_from_rfc3339(due)
        .unwrap()
        .timestamp_millis();
    assert_eq!(due, f.clock.0.load(Ordering::SeqCst) + 5_000);
    for second in 1..=60 {
        f.clock.0.fetch_add(1_000, Ordering::SeqCst);
        let other = call(
            &f.host,
            &f.caller,
            Some(second_workspace.clone()),
            7300 + second,
            m::WorkspaceOperation::CreateArea {
                name: name(&format!("Activity {second}")),
            },
        )
        .await;
        assert!(matches!(
            other.result,
            m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { .. })
        ));
        assert_eq!(runtime.read(read_due()).await.unwrap().rows, original.rows);
        let alarm: Option<i64> = runtime.control("raw-alarm-time", &()).await.unwrap();
        assert!(alarm.is_some_and(|at| at <= due));
    }
    let _: bool = runtime.control("release", &pause).await.unwrap();
    assert!(matches!(
        pending.await.unwrap().result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { .. })
    ));
    f.host.close().await;
}
