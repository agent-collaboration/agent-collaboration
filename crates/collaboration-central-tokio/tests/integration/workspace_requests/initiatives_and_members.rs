use super::*;
#[tokio::test]
async fn closed_initiative_guards_reopening_and_references_never_move_projects() {
    let f = Fixture::new().await;
    let first_area = f.area(500).await;
    let second_area = f.area(501).await;
    let first = f.project(502, first_area.clone()).await;
    let second = f.project(503, second_area.clone()).await;
    let m::ItemRef::Initiative(initiative) = created(
        f.call(
            504,
            m::WorkspaceOperation::CreateInitiative {
                name: name("Initiative"),
                description: "Description".to_owned().try_into().unwrap(),
            },
        )
        .await,
    ) else {
        panic!("initiative");
    };
    let source = m::ReferenceSource::Initiative(initiative.clone());
    for (request, project) in [(505, first.clone()), (506, second.clone())] {
        created(
            f.call(
                request,
                m::WorkspaceOperation::AddReference {
                    source: source.clone(),
                    target: m::ReferenceTarget::try_from(m::ItemRef::Project(project)).unwrap(),
                    details: None,
                },
            )
            .await,
        );
    }
    let close = m::WorkspaceOperation::SetInitiativeStatus {
        initiative_id: initiative.clone(),
        status: m::WorkStatus::Closed,
    };
    assert!(
        matches!(f.call(507,close.clone()).await.result,m::WorkspaceResult::Refused(m::RefusalReason::ChildrenNotClosed{blockers}) if blockers.len()==2)
    );
    for (request, project) in [(508, first.clone()), (509, second.clone())] {
        assert!(matches!(
            f.call(
                request,
                m::WorkspaceOperation::SetProjectStatus {
                    project_id: project,
                    status: m::WorkStatus::Closed
                }
            )
            .await
            .result,
            m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
        ));
    }
    assert!(matches!(
        f.call(510, close).await.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    assert!(matches!(
        f.call(
            511,
            m::WorkspaceOperation::SetProjectStatus {
                project_id: first.clone(),
                status: m::WorkStatus::Open
            }
        )
        .await
        .result,
        m::WorkspaceResult::Refused(m::RefusalReason::ParentClosed { .. })
    ));
    assert!(matches!(
        f.call(
            512,
            m::WorkspaceOperation::CreateDeliverable {
                project_id: first.clone(),
                title: name("Refused child")
            }
        )
        .await
        .result,
        m::WorkspaceResult::Refused(m::RefusalReason::ParentClosed { .. })
    ));
    let third = f.project(513, first_area.clone()).await;
    assert!(matches!(
        f.call(
            514,
            m::WorkspaceOperation::AddReference {
                source: source.clone(),
                target: m::ReferenceTarget::try_from(m::ItemRef::Project(third.clone())).unwrap(),
                details: None
            }
        )
        .await
        .result,
        m::WorkspaceResult::Refused(m::RefusalReason::ParentClosed { .. })
    ));
    f.call(
        515,
        m::WorkspaceOperation::SetProjectStatus {
            project_id: third.clone(),
            status: m::WorkStatus::Closed,
        },
    )
    .await;
    created(
        f.call(
            516,
            m::WorkspaceOperation::AddReference {
                source,
                target: m::ReferenceTarget::try_from(m::ItemRef::Project(third)).unwrap(),
                details: None,
            },
        )
        .await,
    );
    for (project, area) in [(first, first_area), (second, second_area)] {
        let shown = f
            .call(
                0,
                m::WorkspaceOperation::Show {
                    item: m::ItemRef::Project(project),
                },
            )
            .await;
        assert!(
            matches!(shown.result,m::WorkspaceResult::Done(m::WorkspaceAnswer::Item{item:m::ItemView::Project(project)}) if project.area_id==area)
        );
    }
    f.host.close().await;
}
#[tokio::test]
async fn only_creator_adds_or_promotes_members_and_posts_require_active_participation() {
    let f = Fixture::new().await;
    let area = f.area(530).await;
    let thread = f.thread(531, area).await;
    let mut other = f.caller.clone();
    let mut other_session = session(&other);
    other_session.session_id = "other-participant".to_owned().try_into().unwrap();
    other.caller = m::Caller::Session(other_session);
    let join = m::WorkspaceOperation::JoinThread {
        thread_id: thread.clone(),
        role: m::MemberRole::Observer,
    };
    assert!(matches!(
        call(&f.host, &other, Some(f.workspace.clone()), 532, join)
            .await
            .result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    for (request, setting) in [
        (541, m::NotificationSetting::Off),
        (542, m::NotificationSetting::On),
    ] {
        let reply = call(
            &f.host,
            &other,
            Some(f.workspace.clone()),
            request,
            m::WorkspaceOperation::SetNotifications {
                thread_id: thread.clone(),
                setting,
            },
        )
        .await;
        assert!(matches!(
            reply.result,
            m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
        ));
        let inbox = call(
            &f.host,
            &other,
            Some(f.workspace.clone()),
            0,
            m::WorkspaceOperation::Inbox {
                after: None,
                limit: 100.try_into().unwrap(),
            },
        )
        .await;
        assert!(
            matches!(inbox.result,m::WorkspaceResult::Done(m::WorkspaceAnswer::Inbox{entries,..}) if entries.is_empty()==(setting==m::NotificationSetting::Off))
        );
    }
    let post = m::WorkspaceOperation::ThreadPost {
        thread_id: thread.clone(),
        body: "Post".to_owned().try_into().unwrap(),
        addressees: vec![],
    };
    assert_eq!(
        call(
            &f.host,
            &other,
            Some(f.workspace.clone()),
            533,
            post.clone()
        )
        .await
        .result,
        m::WorkspaceResult::Refused(m::RefusalReason::NotParticipant {
            thread_id: thread.clone()
        })
    );
    let runtime = f.host.central(&f.caller.organization_id).await.unwrap();
    let actors = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT actor_identity_id,identity FROM actor_identities",
            args: vec![],
        })
        .await
        .unwrap();
    let actor = actors
        .rows
        .iter()
        .find_map(|row| match (&row[0], &row[1]) {
            (SqlValue::Text(id), SqlValue::Text(identity))
                if identity.contains("other-participant") =>
            {
                Some(id.parse().unwrap())
            }
            _ => None,
        })
        .unwrap();
    let promote = m::WorkspaceOperation::AddThreadMember {
        thread_id: thread.clone(),
        actor,
        role: m::MemberRole::Participant,
    };
    assert!(matches!(
        call(
            &f.host,
            &other,
            Some(f.workspace.clone()),
            534,
            promote.clone()
        )
        .await
        .result,
        m::WorkspaceResult::Refused(m::RefusalReason::NotCreator { .. })
    ));
    f.call(535, promote).await;
    created(
        call(
            &f.host,
            &other,
            Some(f.workspace.clone()),
            536,
            post.clone(),
        )
        .await,
    );
    assert!(matches!(
        call(
            &f.host,
            &other,
            Some(f.workspace.clone()),
            537,
            m::WorkspaceOperation::ResolveThread {
                thread_id: thread.clone()
            }
        )
        .await
        .result,
        m::WorkspaceResult::Refused(m::RefusalReason::NotCreator { .. })
    ));
    call(
        &f.host,
        &other,
        Some(f.workspace.clone()),
        538,
        m::WorkspaceOperation::LeaveThread {
            thread_id: thread.clone(),
        },
    )
    .await;
    assert_eq!(
        call(&f.host, &other, Some(f.workspace.clone()), 539, post)
            .await
            .result,
        m::WorkspaceResult::Refused(m::RefusalReason::NotMember {
            thread_id: thread.clone()
        })
    );
    assert_eq!(
        f.call(
            540,
            m::WorkspaceOperation::AddThreadMember {
                thread_id: thread,
                actor: id(999),
                role: m::MemberRole::Participant
            }
        )
        .await
        .result,
        m::WorkspaceResult::Refused(m::RefusalReason::UnknownActor {
            actor_identity_id: id(999)
        })
    );
    f.host.close().await;
}
