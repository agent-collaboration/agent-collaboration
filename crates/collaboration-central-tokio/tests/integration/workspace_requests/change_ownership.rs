use super::*;
#[tokio::test]
async fn deliverable_issue_and_dependency_changes_carry_the_exact_owning_project() {
    let f = Fixture::new().await;
    let area = f.area(980).await;
    let project = f.project(981, area).await;
    let first = f.deliverable(982, project.clone()).await;
    let second = f.deliverable(983, project.clone()).await;
    let m::ItemRef::Issue(issue) = created(
        f.call(
            984,
            m::WorkspaceOperation::CreateIssue {
                deliverable_id: second.clone(),
                title: "Issue".to_owned().try_into().unwrap(),
            },
        )
        .await,
    ) else {
        panic!("issue");
    };
    f.call(
        985,
        m::WorkspaceOperation::SetIssueStatus {
            issue_id: issue.clone(),
            status: m::WorkStatus::Open,
        },
    )
    .await;
    f.call(
        986,
        m::WorkspaceOperation::AddDependency {
            blocking: first.clone(),
            blocked: second.clone(),
        },
    )
    .await;
    let runtime = f.host.central(&f.caller.organization_id).await.unwrap();
    let edge=runtime.inspect_workspace(&f.workspace,Statement{sql:"SELECT dependency_id FROM deliverable_dependencies WHERE blocked_deliverable_id=?",args:vec![text(&second)]}).await.unwrap();
    let SqlValue::Text(edge) = &edge.rows[0][0] else {
        panic!("edge");
    };
    f.call(
        987,
        m::WorkspaceOperation::RemoveDependency {
            dependency_id: edge.parse().unwrap(),
        },
    )
    .await;
    for (item, kinds) in [
        (m::ItemRef::Deliverable(first), vec!["created"]),
        (
            m::ItemRef::Deliverable(second),
            vec!["created", "updated", "updated"],
        ),
        (m::ItemRef::Issue(issue), vec!["created", "stateChanged"]),
    ] {
        let (kind, id) = item.kind_and_id();
        let rows=runtime.inspect_workspace(&f.workspace,Statement{sql:"SELECT item_kind,change_kind,thread_id,project_id FROM changes WHERE item_id=? ORDER BY position",args:vec![text(id)]}).await.unwrap();
        assert_eq!(
            rows.rows,
            kinds
                .into_iter()
                .map(|event| vec![text(kind), text(event), SqlValue::Null, text(&project)])
                .collect::<Vec<_>>()
        );
    }
    let initiative = created(
        f.call(
            988,
            m::WorkspaceOperation::CreateInitiative {
                name: name("Initiative"),
                description: "Description".to_owned().try_into().unwrap(),
            },
        )
        .await,
    );
    let m::ItemRef::Initiative(id) = initiative else {
        panic!("initiative");
    };
    f.call(
        989,
        m::WorkspaceOperation::SetInitiativeStatus {
            initiative_id: id.clone(),
            status: m::WorkStatus::Open,
        },
    )
    .await;
    let rows = runtime
        .inspect_workspace(
            &f.workspace,
            Statement {
                sql: "SELECT project_id FROM changes WHERE item_id=? ORDER BY position",
                args: vec![text(id)],
            },
        )
        .await
        .unwrap();
    assert_eq!(rows.rows, vec![vec![SqlValue::Null], vec![SqlValue::Null]]);
    f.host.close().await;
}
