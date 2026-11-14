use super::*;
#[tokio::test]
async fn issue_lifecycle_reads_exact_status_and_on_hold_blocks_closure_with_named_children() {
    let f = Fixture::new().await;
    let area = f.area(1000).await;
    let project = f.project(1001, area).await;
    let deliverable = f.deliverable(1002, project.clone()).await;
    let m::ItemRef::Issue(issue) = created(
        f.call(
            1003,
            m::WorkspaceOperation::CreateIssue {
                deliverable_id: deliverable.clone(),
                title: "Named issue".to_owned().try_into().unwrap(),
            },
        )
        .await,
    ) else {
        panic!("issue");
    };
    let show = |issue: m::IssueId| m::WorkspaceOperation::Show {
        item: m::ItemRef::Issue(issue),
    };
    let initial = f.call(0, show(issue.clone())).await;
    assert!(
        matches!(initial.result,m::WorkspaceResult::Done(m::WorkspaceAnswer::Item{item:m::ItemView::Issue(row)}) if row.status==m::WorkStatus::Backlog)
    );
    for (request, status) in [
        (1004, m::WorkStatus::Open),
        (1005, m::WorkStatus::OnHold),
        (1006, m::WorkStatus::Closed),
        (1007, m::WorkStatus::Open),
        (1008, m::WorkStatus::OnHold),
    ] {
        assert!(matches!(
            f.call(
                request,
                m::WorkspaceOperation::SetIssueStatus {
                    issue_id: issue.clone(),
                    status
                }
            )
            .await
            .result,
            m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
        ));
        let shown = f.call(0, show(issue.clone())).await;
        assert!(
            matches!(shown.result,m::WorkspaceResult::Done(m::WorkspaceAnswer::Item{item:m::ItemView::Issue(row)}) if row.status==status),
            "requested exact status {status:?}"
        );
    }
    let close = m::WorkspaceOperation::SetProjectStatus {
        project_id: project.clone(),
        status: m::WorkStatus::Closed,
    };
    let refused = f.call(1009, close.clone()).await;
    assert_eq!(
        refused.result,
        m::WorkspaceResult::Refused(m::RefusalReason::ChildrenNotClosed {
            blockers: vec![
                m::Blocker {
                    item: m::ItemRef::Deliverable(deliverable),
                    title: "Deliverable 1002".to_owned(),
                    status: m::BlockerStatus::Deliverable(m::DeliverableStatus::Open)
                },
                m::Blocker {
                    item: m::ItemRef::Issue(issue.clone()),
                    title: "Named issue".to_owned(),
                    status: m::BlockerStatus::Work(m::WorkStatus::OnHold)
                }
            ]
        })
    );
    f.call(
        1010,
        m::WorkspaceOperation::SetIssueStatus {
            issue_id: issue,
            status: m::WorkStatus::Closed,
        },
    )
    .await;
    assert!(matches!(
        f.call(1011, close).await.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    f.host.close().await;
}
#[tokio::test]
async fn advisor_replacement_ends_old_assignment_and_refuses_a_nonlead_advisor_grant() {
    let f = Fixture::new().await;
    let area = f.area(1020).await;
    let project = f.project(1021, area).await;
    let mut first = session(&f.caller);
    first.session_id = "first-advisor".to_owned().try_into().unwrap();
    let mut second = first.clone();
    second.session_id = "second-advisor".to_owned().try_into().unwrap();
    for (request, holder) in [(1022, first.clone()), (1023, second.clone())] {
        assert!(matches!(
            f.call(
                request,
                m::WorkspaceOperation::GrantProjectRole {
                    project_id: project.clone(),
                    role: m::ProjectRole::Advisor,
                    holder: m::ActorRef::Session(holder)
                }
            )
            .await
            .result,
            m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
        ));
    }
    let runtime = f.host.central(&f.caller.organization_id).await.unwrap();
    let identities = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT actor_identity_id,identity FROM actor_identities",
            args: vec![],
        })
        .await
        .unwrap();
    let find = |needle: &str| {
        identities
            .rows
            .iter()
            .find_map(|row| match (&row[0], &row[1]) {
                (SqlValue::Text(id), SqlValue::Text(identity)) if identity.contains(needle) => {
                    Some(id.clone())
                }
                _ => None,
            })
            .unwrap()
    };
    let old = find("first-advisor");
    let new = find("second-advisor");
    let rows=runtime.inspect_workspace(&f.workspace,Statement{sql:"SELECT holder,ended_at FROM project_roles WHERE project_id=? AND role='advisor' ORDER BY created_at,project_role_id",args:vec![text(&project)]}).await.unwrap();
    assert_eq!(rows.rows.len(), 2);
    assert!(
        rows.rows
            .iter()
            .any(|row| row[0] == text(&old) && row[1] != SqlValue::Null)
    );
    assert_eq!(
        rows.rows
            .iter()
            .filter(|row| row[1] == SqlValue::Null)
            .collect::<Vec<_>>(),
        vec![&vec![text(&new), SqlValue::Null]]
    );
    let mut nonlead = f.caller.clone();
    nonlead.caller = m::Caller::Session(second);
    let refused = call(
        &f.host,
        &nonlead,
        Some(f.workspace.clone()),
        1024,
        m::WorkspaceOperation::GrantProjectRole {
            project_id: project.clone(),
            role: m::ProjectRole::Advisor,
            holder: m::ActorRef::Me,
        },
    )
    .await;
    let shown = f
        .call(
            0,
            m::WorkspaceOperation::Show {
                item: m::ItemRef::Project(project.clone()),
            },
        )
        .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Item {
        item: m::ItemView::Project(project),
    }) = shown.result
    else {
        panic!("project");
    };
    assert_eq!(
        refused.result,
        m::WorkspaceResult::Refused(m::RefusalReason::NotLead {
            project: m::NamedItem {
                item: m::ItemRef::Project(project.project_id),
                name: project.title.as_str().to_owned()
            },
            lead: project.lead
        })
    );
    f.host.close().await;
}
