use super::*;
#[tokio::test]
async fn list_deliverables_computes_empty_on_hold_and_closed_and_pages_by_position() {
    let f = Fixture::new().await;
    let area = f.area(850).await;
    let project = f.project(851, area).await;
    let empty = f.deliverable(852, project.clone()).await;
    let on_hold = f.deliverable(853, project.clone()).await;
    let closed = f.deliverable(854, project.clone()).await;
    for (request, deliverable, status) in [
        (855, on_hold.clone(), m::WorkStatus::OnHold),
        (857, closed.clone(), m::WorkStatus::Closed),
    ] {
        let m::ItemRef::Issue(issue) = created(
            f.call(
                request,
                m::WorkspaceOperation::CreateIssue {
                    deliverable_id: deliverable,
                    title: "Issue".to_owned().try_into().unwrap(),
                },
            )
            .await,
        ) else {
            panic!("issue");
        };
        f.call(
            request + 1,
            m::WorkspaceOperation::SetIssueStatus {
                issue_id: issue,
                status,
            },
        )
        .await;
    }
    let first = f
        .call(
            0,
            m::WorkspaceOperation::List {
                query: m::ListQuery::Deliverables {
                    project_id: project.clone(),
                },
                after: m::ChangePosition::ZERO,
                limit: 2.try_into().unwrap(),
            },
        )
        .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Page {
        items,
        next_after_position: Some(cursor),
    }) = first.result
    else {
        panic!("first page");
    };
    assert!(
        matches!(&items[..],[m::ItemView::Deliverable{view:first,..},m::ItemView::Deliverable{view:second,..}] if first.deliverable.deliverable_id==empty && first.status==m::DeliverableStatus::Open&&second.deliverable.deliverable_id==on_hold&&second.status==m::DeliverableStatus::Open)
    );
    let second = f
        .call(
            0,
            m::WorkspaceOperation::List {
                query: m::ListQuery::Deliverables {
                    project_id: project.clone(),
                },
                after: cursor,
                limit: 2.try_into().unwrap(),
            },
        )
        .await;
    assert!(
        matches!(second.result,m::WorkspaceResult::Done(m::WorkspaceAnswer::Page{items,next_after_position:None}) if matches!(&items[..],[m::ItemView::Deliverable{view,..}] if view.deliverable.deliverable_id==closed&&view.status==m::DeliverableStatus::Closed))
    );
    let refused = f
        .call(
            859,
            m::WorkspaceOperation::SetProjectStatus {
                project_id: project,
                status: m::WorkStatus::Closed,
            },
        )
        .await;
    assert!(
        matches!(refused.result,m::WorkspaceResult::Refused(m::RefusalReason::ChildrenNotClosed{blockers}) if blockers.iter().any(|row|row.item==m::ItemRef::Deliverable(empty.clone())))
    );
    f.host.close().await;
}
#[tokio::test]
async fn work_log_redaction_is_null_in_reopened_store_and_retains_original_order() {
    let mut f = Fixture::new().await;
    let area = f.area(870).await;
    let project = f.project(871, area).await;
    let mut entries = vec![];
    for request in [872, 873, 874] {
        entries.push(created(
            f.call(
                request,
                m::WorkspaceOperation::WorkLogAdd {
                    project_id: project.clone(),
                    body: format!("Entry {request}").try_into().unwrap(),
                    tags: vec![],
                },
            )
            .await,
        ));
    }
    let m::ItemRef::WorkLogEntry(second) = &entries[1] else {
        panic!("entry");
    };
    f.call(
        875,
        m::WorkspaceOperation::WorkLogRedact {
            entry_id: second.clone(),
        },
    )
    .await;
    f.reopen().await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    let stored = runtime
        .inspect_workspace(
            &f.workspace,
            Statement {
                sql: "SELECT body,redacted FROM work_log_entries WHERE entry_id=?",
                args: vec![text(second)],
            },
        )
        .await
        .unwrap();
    assert_eq!(stored.rows[0][0], SqlValue::Null);
    let listed = f
        .call(
            0,
            m::WorkspaceOperation::List {
                query: m::ListQuery::WorkLog {
                    project_id: project,
                },
                after: m::ChangePosition::ZERO,
                limit: 10.try_into().unwrap(),
            },
        )
        .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Page { items, .. }) = listed.result else {
        panic!("list");
    };
    assert_eq!(
        items.iter().map(m::ItemView::item_ref).collect::<Vec<_>>(),
        entries
    );
    assert!(
        matches!(&items[1],m::ItemView::WorkLogEntry(entry) if entry.body==m::WorkLogBody::Redacted)
    );
    f.host.close().await;
}
