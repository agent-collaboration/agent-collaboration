use super::*;

#[tokio::test]
async fn backlog_close_drains_both_change_positions_and_replay_keeps_cursor() {
    let f = Fixture::new().await;
    let area = f.area(2200).await;
    let project = f.project(2201, area.clone()).await;
    let target = m::ItemRef::Deliverable(f.deliverable(2202, project).await);
    let item = add_backlog(&f, 2203, area).await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    let cursor_query = || Statement {
        sql: "SELECT drained_through_position,drain_due_at FROM workspace_drains WHERE workspace_id=?",
        args: vec![text(&f.workspace)],
    };
    let before = runtime.statement_host().read(cursor_query()).await.unwrap();
    let SqlValue::Integer(position) = before.rows[0][0] else {
        panic!("drain position")
    };
    assert_eq!(before.rows[0][1], SqlValue::Null);
    let operation = close(&item, "Delivered with a reference", Some(target));
    let reply = f.call(2204, operation.clone()).await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated {
        position: answer_position,
        ..
    }) = &reply.result
    else {
        panic!("close refused: {reply:?}")
    };
    assert_eq!(
        answer_position.get(),
        u64::try_from(position.get() + 1).unwrap()
    );
    let changes = runtime.inspect_workspace(&f.workspace, Statement {
        sql: "SELECT position,change_kind,item_kind FROM changes WHERE position>? ORDER BY position",
        args: vec![SqlValue::Integer(position)],
    }).await.unwrap();
    assert_eq!(
        changes.rows,
        vec![
            vec![
                SqlValue::Integer((position.get() + 1).try_into().unwrap()),
                text("stateChanged"),
                text("backlogItem")
            ],
            vec![
                SqlValue::Integer((position.get() + 2).try_into().unwrap()),
                text("created"),
                text("reference")
            ],
        ]
    );
    let after = runtime.statement_host().read(cursor_query()).await.unwrap();
    assert_eq!(
        after.rows,
        vec![vec![
            SqlValue::Integer((position.get() + 2).try_into().unwrap()),
            SqlValue::Null
        ]]
    );
    assert_eq!(f.call(2204, operation).await, reply);
    assert_eq!(
        runtime.statement_host().read(cursor_query()).await.unwrap(),
        after
    );
    assert_eq!(runtime.inspect_workspace(&f.workspace,Statement {
        sql:"SELECT position,change_kind,item_kind FROM changes WHERE position>? ORDER BY position",args:vec![SqlValue::Integer(position)],
    }).await.unwrap(),changes);
    f.host.close().await;
}
