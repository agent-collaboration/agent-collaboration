use super::*;
/// The owner's rule: anyone may take the lead role or hand it on, with no
/// precondition; the replaced lead's assignment ends.
#[tokio::test]
async fn anyone_takes_the_lead_and_a_lead_hands_it_over() {
    let f = Fixture::new().await;
    let area = f.area(1040).await;
    let project = f.project(1041, area).await;
    let creator = lead_identity(&f, &project).await;
    let mut taker_session = session(&f.caller);
    taker_session.session_id = "lead-taker".to_owned().try_into().unwrap();
    let taker = collaboration_central_core::WorkspaceCaller::new(
        f.caller.connection().clone(),
        m::Caller::Session(taker_session.clone()),
    )
    .unwrap();
    let person = collaboration_central_core::WorkspaceCaller::new(
        f.caller.connection().clone(),
        m::Caller::Person {
            machine_id: f.caller.caller().machine_id().clone(),
            person_id: id(1044),
        },
    )
    .unwrap();
    let mut successor = session(&f.caller);
    successor.session_id = "lead-successor".to_owned().try_into().unwrap();
    let grant = |holder: m::ActorRef| m::WorkspaceOperation::GrantProjectRole {
        project_id: project.clone(),
        role: m::ProjectRole::Lead,
        holder,
    };

    // Act: a non-lead agent session takes the lead for itself.
    let taken = call(
        &f.host,
        &taker,
        Some(f.workspace.clone()),
        1042,
        grant(m::ActorRef::Me),
    )
    .await;
    assert!(
        matches!(
            taken.result,
            m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
        ),
        "{taken:?}"
    );
    assert_eq!(
        lead_identity(&f, &project).await,
        m::Identity::AgentSession {
            session: taker_session
        }
    );

    // Act: a person, who never held the lead, takes it.
    let by_person = call(
        &f.host,
        &person,
        Some(f.workspace.clone()),
        1043,
        grant(m::ActorRef::Me),
    )
    .await;
    assert!(
        matches!(
            by_person.result,
            m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
        ),
        "{by_person:?}"
    );
    assert_eq!(
        lead_identity(&f, &project).await,
        m::Identity::Person {
            person_id: id(1044)
        }
    );

    // Act: the person, now lead, hands the lead to another session.
    let handed = call(
        &f.host,
        &person,
        Some(f.workspace.clone()),
        1045,
        grant(m::ActorRef::Session(successor.clone())),
    )
    .await;
    assert!(
        matches!(
            handed.result,
            m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
        ),
        "{handed:?}"
    );
    assert_eq!(
        lead_identity(&f, &project).await,
        m::Identity::AgentSession { session: successor }
    );
    assert_ne!(creator, lead_identity(&f, &project).await);

    // Assert: each replaced lead's assignment ended; exactly one is active.
    let runtime = f.host.central(f.caller.organization_id()).await.unwrap();
    let rows = runtime
        .inspect_workspace(
            &f.workspace,
            Statement {
                sql: "SELECT holder,ended_at FROM project_roles WHERE project_id=? AND role='lead'",
                args: vec![text(&project)],
            },
        )
        .await
        .unwrap();
    assert_eq!(rows.rows.len(), 4, "creator, agent, person, successor");
    assert_eq!(
        rows.rows
            .iter()
            .filter(|row| row[1] == SqlValue::Null)
            .count(),
        1
    );
    f.host.close().await;
}

async fn lead_identity(f: &Fixture, project: &m::ProjectId) -> m::Identity {
    let shown = f
        .call(
            0,
            m::WorkspaceOperation::Show {
                item: m::ItemRef::Project(project.clone()),
            },
        )
        .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Item {
        item: m::ItemView::Project(view),
    }) = &shown.result
    else {
        panic!("project: {shown:?}");
    };
    shown
        .actors
        .iter()
        .find(|actor| actor.actor_identity_id == view.lead)
        .map(|actor| actor.identity.clone())
        .unwrap()
}
