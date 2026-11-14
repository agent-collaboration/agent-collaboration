use super::*;
#[tokio::test]
async fn role_history_unknown_actors_and_dependency_graph_decisions_use_current_rows() {
    let f = Fixture::new().await;
    let area = f.area(200).await;
    let project = f.project(201, area.clone()).await;
    let first = f.deliverable(202, project.clone()).await;
    let second = f.deliverable(203, project.clone()).await;
    let third = f.deliverable(204, project.clone()).await;
    for (request, blocking, blocked) in [
        (205, first.clone(), second.clone()),
        (206, second.clone(), third.clone()),
    ] {
        let answer = f
            .call(
                request,
                m::WorkspaceOperation::AddDependency { blocking, blocked },
            )
            .await;
        assert!(
            matches!(
                answer.result,
                m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
            ),
            "{answer:?}"
        );
    }
    let cycle = f
        .call(
            207,
            m::WorkspaceOperation::AddDependency {
                blocking: third.clone(),
                blocked: first.clone(),
            },
        )
        .await;
    assert_eq!(
        cycle.result,
        m::WorkspaceResult::Refused(m::RefusalReason::WouldCycle {
            path: vec![third.clone(), first.clone(), second.clone(), third]
        })
    );
    let duplicate = f
        .call(
            208,
            m::WorkspaceOperation::AddDependency {
                blocking: first.clone(),
                blocked: second.clone(),
            },
        )
        .await;
    assert_eq!(
        duplicate.result,
        m::WorkspaceResult::Refused(m::RefusalReason::DuplicateDependency {
            blocking: first.clone(),
            blocked: second.clone()
        })
    );
    let other_project = f.project(209, area).await;
    let outside = f.deliverable(210, other_project).await;
    assert_eq!(
        f.call(
            211,
            m::WorkspaceOperation::AddDependency {
                blocking: first.clone(),
                blocked: outside.clone()
            }
        )
        .await
        .result,
        m::WorkspaceResult::Refused(m::RefusalReason::CrossProject {
            blocking: first.clone(),
            blocked: outside
        })
    );
    let runtime = f.host.central(&f.caller.organization_id).await.unwrap();
    let edges=runtime.inspect_workspace(&f.workspace,Statement{sql:"SELECT dependency_id FROM deliverable_dependencies WHERE blocking_deliverable_id=? AND blocked_deliverable_id=?",args:vec![text(first),text(second)]}).await.unwrap();
    let SqlValue::Text(edge) = &edges.rows[0][0] else {
        panic!("edge");
    };
    let dependency_id = edge.parse().unwrap();
    let remove = m::WorkspaceOperation::RemoveDependency { dependency_id };
    let removed = f.call(212, remove.clone()).await;
    assert!(
        matches!(
            removed.result,
            m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
        ),
        "{removed:?}"
    );
    assert_eq!(removed, f.call(212, remove.clone()).await);
    let missing = f.call(213, remove.clone()).await;
    assert!(matches!(
        missing.result,
        m::WorkspaceResult::Refused(m::RefusalReason::DependencyNotFound { .. })
    ));
    assert_eq!(missing, f.call(213, remove).await);
    let unknown = m::WorkspaceOperation::GrantProjectRole {
        project_id: project.clone(),
        role: m::ProjectRole::Advisor,
        holder: m::ActorRef::Actor(id(900)),
    };
    assert_eq!(
        f.call(214, unknown).await.result,
        m::WorkspaceResult::Refused(m::RefusalReason::UnknownActor {
            actor_identity_id: id(900)
        })
    );
    for (request, role) in [
        (215, m::DeliverableRole::Implementer),
        (216, m::DeliverableRole::Reviewer),
        (217, m::DeliverableRole::Verifier),
    ] {
        let operation = m::WorkspaceOperation::GrantDeliverableRole {
            deliverable_id: f.deliverable(220 + request, project.clone()).await,
            role,
            holder: m::ActorRef::Me,
        };
        assert!(matches!(
            f.call(request, operation).await.result,
            m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
        ));
    }
    let grant = m::WorkspaceOperation::GrantProjectRole {
        project_id: project.clone(),
        role: m::ProjectRole::Advisor,
        holder: m::ActorRef::Me,
    };
    assert!(matches!(
        f.call(218, grant.clone()).await.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    let no_op = f.call(219, grant.clone()).await;
    assert_eq!(
        no_op.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Unchanged {
            item: m::ItemRef::Project(project.clone())
        })
    );
    assert!(matches!(
        f.call(
            220,
            m::WorkspaceOperation::ReleaseProjectRole {
                project_id: project.clone(),
                role: m::ProjectRole::Advisor
            }
        )
        .await
        .result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    assert_eq!(
        no_op,
        f.call(219, grant).await,
        "cached no-op is not re-decided after release"
    );
    assert!(matches!(
        f.call(
            221,
            m::WorkspaceOperation::ReleaseProjectRole {
                project_id: project,
                role: m::ProjectRole::Lead
            }
        )
        .await
        .result,
        m::WorkspaceResult::Refused(m::RefusalReason::LeadRequired { .. })
    ));
    f.host.close().await;
}
