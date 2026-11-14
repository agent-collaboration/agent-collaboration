use super::*;
use central_storage::{
    AlarmUpdate, BatchOutcome, CorruptReason, GuardedStatement, RowExpectation, StatementBatch,
    StoreFailure,
};

#[tokio::test]
async fn r21_close_cannot_link_an_existing_item_from_another_workspace() {
    let f = Fixture::new().await;
    let area = f.area(2100).await;
    let item = created(
        f.call(
            2101,
            m::WorkspaceOperation::AddBacklogItem {
                area_id: area,
                title: name("Original"),
                description: "Details".to_owned().try_into().unwrap(),
            },
        )
        .await,
    );
    let m::ItemRef::BacklogItem(item_id) = &item else {
        panic!("backlog");
    };
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceCreated { workspace: other }) = call(
        &f.host,
        &f.caller,
        None,
        2104,
        m::WorkspaceOperation::CreateWorkspace {
            name: name("Other workspace"),
        },
    )
    .await
    .result
    else {
        panic!("second workspace");
    };
    let other = other.workspace_id;
    let runtime = f.host.central(&f.caller.organization_id).await;
    assert!(matches!(
        runtime
            .statement_host()
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement {
                        sql: "UPDATE machines SET allowed_workspaces=? WHERE machine_id=?",
                        args: vec![
                            text(serde_json::to_string(&[&f.workspace, &other]).unwrap()),
                            text(f.caller.caller.machine_id())
                        ],
                    },
                    expect: RowExpectation::Exactly(1)
                }],
                alarm: AlarmUpdate::Unchanged,
            })
            .await
            .unwrap(),
        BatchOutcome::Applied { .. }
    ));
    let foreign = created(
        call(
            &f.host,
            &f.caller,
            Some(other),
            2102,
            m::WorkspaceOperation::CreateArea {
                name: name("Foreign area"),
            },
        )
        .await,
    );
    let before = f
        .call(0, m::WorkspaceOperation::Show { item: item.clone() })
        .await;
    let refused = f
        .call(
            2103,
            m::WorkspaceOperation::CloseBacklogItem {
                item_id: item_id.clone(),
                note: "Move".to_owned().try_into().unwrap(),
                link: Some(foreign.clone()),
            },
        )
        .await;
    assert_eq!(
        refused.result,
        m::WorkspaceResult::NotFound {
            workspace_id: f.workspace.clone(),
            item: foreign
        }
    );
    assert_eq!(
        f.call(0, m::WorkspaceOperation::Show { item }).await,
        before
    );
    let effects = runtime.inspect_workspace(&f.workspace, Statement {
        sql: "SELECT (SELECT COUNT(*) FROM item_references),(SELECT COUNT(*) FROM changes WHERE change_kind='stateChanged')", args: vec![],
    }).await.unwrap();
    assert_eq!(
        effects.rows,
        vec![vec![
            central_storage::SqlValue::Integer(central_storage::SafeInteger::new(0).unwrap()),
            central_storage::SqlValue::Integer(central_storage::SafeInteger::new(0).unwrap())
        ]]
    );
    f.host.close().await;
}

#[tokio::test]
async fn r21_invalid_stored_status_and_closing_note_fail_show_and_list_decode() {
    let f = Fixture::new().await;
    let area = f.area(2120).await;
    let item = created(
        f.call(
            2121,
            m::WorkspaceOperation::AddBacklogItem {
                area_id: area.clone(),
                title: name("Backlog"),
                description: "Details".to_owned().try_into().unwrap(),
            },
        )
        .await,
    );
    let m::ItemRef::BacklogItem(item_id) = &item else {
        panic!("backlog");
    };
    for (status, note, expected_column) in [
        ("done", None, "backlog_items"),
        (
            "open",
            Some("Invalid open note"),
            "backlog_items.closing_note",
        ),
        ("closed", None, "backlog_items.closing_note"),
        ("closed", Some(" \t "), "backlog_items.closing_note"),
    ] {
        f.workspace_batch(
            "UPDATE backlog_items SET status=?,closing_note=? WHERE item_id=?",
            vec![
                text(status),
                note.map_or(SqlValue::Null, text),
                text(item_id),
            ],
        )
        .await;
        for operation in [
            m::WorkspaceOperation::Show { item: item.clone() },
            m::WorkspaceOperation::List {
                query: m::ListQuery::Backlog {
                    area_id: area.clone(),
                },
                after: m::ChangePosition::ZERO,
                limit: 10.try_into().unwrap(),
            },
        ] {
            let result = f
                .host
                .request(
                    &f.caller,
                    m::WorkspaceRequest {
                        request_id: None,
                        workspace_id: Some(f.workspace.clone()),
                        operation,
                    },
                )
                .await;
            assert!(
                matches!(result, Err(StoreFailure::Corrupt { reason: CorruptReason::BadValue { column } }) if column==expected_column)
            );
        }
    }
    f.host.close().await;
}
