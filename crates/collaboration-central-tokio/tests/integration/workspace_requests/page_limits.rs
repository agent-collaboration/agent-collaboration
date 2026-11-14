use super::*;
use central_storage::RowExpectation;
#[tokio::test]
async fn zero_page_limits_fail_at_the_typed_request_boundary() {
    let error = m::PageLimit::try_from(0).unwrap_err();
    assert_eq!(error.field, "limit");
    for operation in [
        serde_json::json!({"kind":"list","query":{"kind":"areas"},"after":0,"limit":0}),
        serde_json::json!({"kind":"inbox","after":null,"limit":0}),
    ] {
        let request = serde_json::json!({"workspaceId":id::<m::WorkspaceId>(1),"requestId":null,"operation":operation});
        let error = serde_json::from_value::<m::WorkspaceRequest>(request)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("limit") && error.contains("nonzero"),
            "{error}"
        );
    }
    let maximum = m::PageLimit::try_from(u16::MAX).unwrap();
    assert_eq!(maximum.lookahead(), 65536);
}
#[tokio::test]
async fn maximum_list_and_inbox_pages_retain_the_extra_row_and_skip_nothing() {
    let f = Fixture::new().await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    let empty = runtime
        .inspect_workspace(
            &f.workspace,
            Statement {
                sql: "SELECT area_id FROM areas",
                args: vec![],
            },
        )
        .await
        .unwrap();
    assert!(
        empty.rows.is_empty(),
        "business store ready before bulk fixture writes"
    );
    let actors = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT actor_identity_id FROM actor_identities",
            args: vec![],
        })
        .await
        .unwrap();
    let SqlValue::Text(actor) = &actors.rows[0][0] else {
        panic!("actor");
    };
    f.workspace_execute("WITH RECURSIVE numbers(n) AS (VALUES(0) UNION ALL SELECT n+1 FROM numbers WHERE n<65535) INSERT INTO areas(area_id,name,state,created_at,updated_at) SELECT printf('01890f2e-7b4c-7cc0-98c4-%012x',100000+n),'A','open','2026-11-13T00:00:00Z','2026-11-13T00:00:00Z' FROM numbers",vec![],RowExpectation::Any).await;
    f.workspace_execute("WITH RECURSIVE numbers(n) AS (VALUES(0) UNION ALL SELECT n+1 FROM numbers WHERE n<65535) INSERT INTO changes(change_id,position,change_kind,item_kind,item_id,thread_id,project_id,actor,subject_actor,role,created_at,updated_at) SELECT printf('01890f2e-7b4c-7cc0-98c4-%012x',200000+n),n+1,'created','area',printf('01890f2e-7b4c-7cc0-98c4-%012x',100000+n),NULL,NULL,?,NULL,NULL,'2026-11-13T00:00:00Z','2026-11-13T00:00:00Z' FROM numbers",vec![text(actor)],RowExpectation::Any).await;
    let query = m::ListQuery::Areas;
    let maximum = m::PageLimit::try_from(u16::MAX).unwrap();
    let first = f
        .call(
            0,
            m::WorkspaceOperation::List {
                query: query.clone(),
                after: m::ChangePosition::ZERO,
                limit: maximum,
            },
        )
        .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Page {
        items,
        next_after_position: Some(after),
    }) = first.result
    else {
        panic!("maximum list must continue");
    };
    assert_eq!(items.len(), 65535);
    assert_eq!(after.get(), 65535);
    let ids = items
        .iter()
        .map(m::ItemView::item_ref)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), 65535);
    drop(items);
    let tail = f
        .call(
            0,
            m::WorkspaceOperation::List {
                query,
                after,
                limit: maximum,
            },
        )
        .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Page {
        items,
        next_after_position: None,
    }) = tail.result
    else {
        panic!("list tail");
    };
    assert_eq!(items.len(), 1);
    assert!(!ids.contains(&items[0].item_ref()));
    assert_eq!(items[0].item_ref(), m::ItemRef::Area(id(165535)));
    drop(ids);
    drop(items);
    let thread = f.thread(1200, id(100000)).await;
    let before = 65537_u64.try_into().unwrap();
    f.workspace_execute("WITH RECURSIVE numbers(n) AS (VALUES(0) UNION ALL SELECT n+1 FROM numbers WHERE n<65535) INSERT INTO changes(change_id,position,change_kind,item_kind,item_id,thread_id,project_id,actor,subject_actor,role,created_at,updated_at) SELECT printf('01890f2e-7b4c-7cc0-98c4-%012x',300000+n),65538+n,'updated','thread',?,?,NULL,?,NULL,NULL,'2026-11-13T00:00:00Z','2026-11-13T00:00:00Z' FROM numbers",vec![text(&thread),text(&thread),text(actor)],RowExpectation::Any).await;
    let inbox = f
        .call(
            0,
            m::WorkspaceOperation::Inbox {
                after: Some(before),
                limit: maximum,
            },
        )
        .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Inbox {
        entries,
        next_after_position: Some(after),
        ..
    }) = inbox.result
    else {
        panic!("maximum inbox must continue");
    };
    assert_eq!(entries.len(), 65535);
    assert_eq!(entries.first().unwrap().position.get(), 65538);
    assert_eq!(entries.last().unwrap().position, after);
    assert_eq!(after.get(), 131072);
    drop(entries);
    let tail = f
        .call(
            0,
            m::WorkspaceOperation::Inbox {
                after: Some(after),
                limit: maximum,
            },
        )
        .await;
    assert!(
        matches!(tail.result,m::WorkspaceResult::Done(m::WorkspaceAnswer::Inbox{entries,next_after_position:None,..}) if entries.len()==1 && entries[0].position.get()==131073)
    );
    f.host.close().await;
}
