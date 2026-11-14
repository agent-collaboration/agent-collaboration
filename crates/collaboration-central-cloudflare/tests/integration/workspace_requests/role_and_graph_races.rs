use super::*;
use collaboration_central_core::WorkspaceCaller;
async fn held_pair(
    f: &Fixture,
    first: m::WorkspaceOperation,
    second: m::WorkspaceOperation,
    second_caller: WorkspaceCaller,
) -> (m::WorkspaceReply, m::WorkspaceReply) {
    let runtime = f.host.central(&f.caller.organization_id).await;
    let target = serde_json::json!({"workspace":f.workspace,"point":"afterReads"});
    let _: bool = runtime.control("hold", &target).await.unwrap();
    let request = m::WorkspaceRequest {
        request_id: Some(id(700)),
        workspace_id: Some(f.workspace.clone()),
        operation: first,
    };
    let first = tokio::spawn({
        let runtime = runtime.clone();
        let caller = f.caller.clone();
        async move { runtime.request(&caller, request).await.unwrap() }
    });
    let _: bool = runtime.control("until-held", &target).await.unwrap();
    let witness = serde_json::json!({"workspace":f.workspace,"count":1});
    let _: bool = runtime.control("guard-next-read", &witness).await.unwrap();
    let request = m::WorkspaceRequest {
        request_id: Some(id(701)),
        workspace_id: Some(f.workspace.clone()),
        operation: second,
    };
    let second = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.request(&second_caller, request).await.unwrap() }
    });
    assert!(
        runtime
            .control::<bool>("waiting-before-read", &witness)
            .await
            .unwrap(),
        "second request read while first owned its turn"
    );
    let _: bool = runtime.control("release", &target).await.unwrap();
    (first.await.unwrap(), second.await.unwrap())
}
#[tokio::test]
async fn opposite_dependency_walks_cannot_both_apply() {
    let f = Fixture::new().await;
    let area = f.area(600).await;
    let project = f.project(601, area).await;
    let first = f.deliverable(602, project.clone()).await;
    let second = f.deliverable(603, project).await;
    let (forward, reverse) = held_pair(
        &f,
        m::WorkspaceOperation::AddDependency {
            blocking: first.clone(),
            blocked: second.clone(),
        },
        m::WorkspaceOperation::AddDependency {
            blocking: second.clone(),
            blocked: first.clone(),
        },
        f.caller.clone(),
    )
    .await;
    assert!(matches!(
        forward.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    assert_eq!(
        reverse.result,
        m::WorkspaceResult::Refused(m::RefusalReason::WouldCycle {
            path: vec![second.clone(), first.clone(), second.clone()]
        })
    );
    let shown = f
        .call(
            0,
            m::WorkspaceOperation::Show {
                item: m::ItemRef::Deliverable(second),
            },
        )
        .await;
    assert!(
        matches!(shown.result,m::WorkspaceResult::Done(m::WorkspaceAnswer::Item{item:m::ItemView::Deliverable{blocked_by,..}}) if blocked_by==vec![first])
    );
    f.host.close().await;
}
#[tokio::test]
async fn lead_grants_serialize_and_recheck_the_new_lead_with_one_active_assignment() {
    let f = Fixture::new().await;
    let area = f.area(610).await;
    let project = f.project(611, area).await;
    let mut second = f.caller.clone();
    let mut next_session = session(&second);
    next_session.session_id = "next-lead".to_owned().try_into().unwrap();
    second.caller = m::Caller::Session(next_session);
    let mut third = session(&second);
    third.session_id = "last-lead".to_owned().try_into().unwrap();
    let (first, second) = held_pair(
        &f,
        m::WorkspaceOperation::GrantProjectRole {
            project_id: project.clone(),
            role: m::ProjectRole::Lead,
            holder: m::ActorRef::Session(session(&second)),
        },
        m::WorkspaceOperation::GrantProjectRole {
            project_id: project.clone(),
            role: m::ProjectRole::Lead,
            holder: m::ActorRef::Session(third),
        },
        second,
    )
    .await;
    for reply in [first, second] {
        assert!(
            matches!(
                reply.result,
                m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
            ),
            "{reply:?}"
        );
    }
    let runtime = f.host.central(&f.caller.organization_id).await;
    let rows = runtime
        .inspect_workspace(
            &f.workspace,
            Statement {
                sql: "SELECT ended_at FROM project_roles WHERE project_id=? AND role='lead'",
                args: vec![text(project)],
            },
        )
        .await
        .unwrap();
    assert_eq!(rows.rows.len(), 3);
    assert_eq!(
        rows.rows
            .iter()
            .filter(|row| row[0] == SqlValue::Null)
            .count(),
        1
    );
    f.host.close().await;
}
#[tokio::test]
async fn implementer_grants_serialize_and_leave_other_deliverable_untouched() {
    let f = Fixture::new().await;
    let area = f.area(620).await;
    let project = f.project(621, area).await;
    let target = f.deliverable(622, project.clone()).await;
    let untouched = f.deliverable(623, project).await;
    f.call(
        624,
        m::WorkspaceOperation::GrantDeliverableRole {
            deliverable_id: untouched.clone(),
            role: m::DeliverableRole::Implementer,
            holder: m::ActorRef::Me,
        },
    )
    .await;
    let mut second = session(&f.caller);
    second.session_id = "first-implementer".to_owned().try_into().unwrap();
    let mut third = second.clone();
    third.session_id = "second-implementer".to_owned().try_into().unwrap();
    let (first, second) = held_pair(
        &f,
        m::WorkspaceOperation::GrantDeliverableRole {
            deliverable_id: target.clone(),
            role: m::DeliverableRole::Implementer,
            holder: m::ActorRef::Session(second),
        },
        m::WorkspaceOperation::GrantDeliverableRole {
            deliverable_id: target.clone(),
            role: m::DeliverableRole::Implementer,
            holder: m::ActorRef::Session(third),
        },
        f.caller.clone(),
    )
    .await;
    for reply in [first, second] {
        assert!(
            matches!(
                reply.result,
                m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
            ),
            "{reply:?}"
        );
    }
    let runtime = f.host.central(&f.caller.organization_id).await;
    let rows=runtime.inspect_workspace(&f.workspace,Statement{sql:"SELECT deliverable_id,ended_at FROM deliverable_roles WHERE role='implementer' ORDER BY deliverable_id",args:vec![]}).await.unwrap();
    assert_eq!(
        rows.rows
            .iter()
            .filter(|row| row[0] == text(target.clone()) && row[1] == SqlValue::Null)
            .count(),
        1
    );
    assert_eq!(
        rows.rows
            .iter()
            .filter(|row| row[0] == text(untouched.clone()))
            .count(),
        1
    );
    f.host.close().await;
}
