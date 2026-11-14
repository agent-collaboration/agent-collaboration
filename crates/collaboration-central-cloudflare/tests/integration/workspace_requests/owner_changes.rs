use super::*;

async fn add_backlog(f: &Fixture, request: u32, area_id: m::AreaId) -> m::BacklogItemId {
    let m::ItemRef::BacklogItem(item) = created(
        f.call(
            request,
            m::WorkspaceOperation::AddBacklogItem {
                area_id,
                title: name("Backlog"),
                description: "Details".to_owned().try_into().unwrap(),
            },
        )
        .await,
    ) else {
        panic!("backlog item");
    };
    item
}

async fn show_backlog(f: &Fixture, item_id: &m::BacklogItemId) -> m::BacklogItem {
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Item {
        item: m::ItemView::BacklogItem(item),
    }) = f
        .call(
            0,
            m::WorkspaceOperation::Show {
                item: m::ItemRef::BacklogItem(item_id.clone()),
            },
        )
        .await
        .result
    else {
        panic!("backlog readback");
    };
    item
}

fn close(
    item_id: &m::BacklogItemId,
    note: &str,
    link: Option<m::ItemRef>,
) -> m::WorkspaceOperation {
    m::WorkspaceOperation::CloseBacklogItem {
        item_id: item_id.clone(),
        note: note.to_owned().try_into().unwrap(),
        link,
    }
}

#[tokio::test]
async fn r21_close_persists_note_reference_and_change_and_refuses_closed_edits() {
    // Arrange: a closed parent is allowed as a reference destination, not a new child.
    let mut f = Fixture::new().await;
    let area = f.area(2000).await;
    let project = f.project(2001, area.clone()).await;
    let deliverable = f.deliverable(2002, project.clone()).await;
    let target = m::ItemRef::Deliverable(deliverable.clone());
    let m::ItemRef::Issue(issue) = created(
        f.call(
            2010,
            m::WorkspaceOperation::CreateIssue {
                deliverable_id: deliverable,
                title: "Delivered issue".to_owned().try_into().unwrap(),
            },
        )
        .await,
    ) else {
        panic!("issue");
    };
    f.call(
        2011,
        m::WorkspaceOperation::SetIssueStatus {
            issue_id: issue,
            status: m::WorkStatus::Closed,
        },
    )
    .await;
    assert!(matches!(
        f.call(
            2012,
            m::WorkspaceOperation::SetProjectStatus {
                project_id: project,
                status: m::WorkStatus::Closed
            }
        )
        .await
        .result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    let first = add_backlog(&f, 2003, area.clone()).await;
    let second = add_backlog(&f, 2004, area.clone()).await;
    assert_eq!(
        show_backlog(&f, &first).await.status,
        m::BacklogStatus::Open
    );
    assert_eq!(show_backlog(&f, &first).await.closing_note, None);

    // Act: optional reference, status, note and change are the one request's effect.
    let operation = close(&first, "Delivered", Some(target.clone()));
    let reply = f.call(2005, operation.clone()).await;
    assert!(matches!(
        reply.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    assert_eq!(
        f.call(2005, operation).await,
        reply,
        "same request replays, not double close"
    );
    assert!(matches!(
        f.call(2006, close(&second, "No longer needed", None))
            .await
            .result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    f.reopen().await;

    // Assert through public Show/List and actual owning-store effect counts.
    let item = show_backlog(&f, &first).await;
    assert_eq!(item.status, m::BacklogStatus::Closed);
    assert_eq!(item.closing_note.as_deref(), Some("Delivered"));
    let listed = f
        .call(
            0,
            m::WorkspaceOperation::List {
                query: m::ListQuery::Backlog { area_id: area },
                after: m::ChangePosition::ZERO,
                limit: 100.try_into().unwrap(),
            },
        )
        .await;
    assert!(
        matches!(listed.result, m::WorkspaceResult::Done(m::WorkspaceAnswer::Page { items, .. })
        if matches!(&items[..], [m::ItemView::BacklogItem(a), m::ItemView::BacklogItem(b)]
            if a.status==m::BacklogStatus::Closed && a.closing_note.as_deref()==Some("Delivered")
                && b.status==m::BacklogStatus::Closed && b.closing_note.as_deref()==Some("No longer needed")))
    );
    let references = f
        .call(
            0,
            m::WorkspaceOperation::List {
                query: m::ListQuery::ReferencesFrom {
                    item: m::ItemRef::BacklogItem(first.clone()),
                },
                after: m::ChangePosition::ZERO,
                limit: 100.try_into().unwrap(),
            },
        )
        .await;
    assert!(
        matches!(references.result, m::WorkspaceResult::Done(m::WorkspaceAnswer::Page { items, .. })
        if matches!(&items[..], [m::ItemView::Reference(reference)] if reference.source==m::ReferenceSource::BacklogItem(first.clone()) && reference.target.as_item_ref()==&target))
    );
    let runtime = f.host.central(&f.caller.organization_id).await;
    let effects = runtime.inspect_workspace(&f.workspace, Statement {
        sql: "SELECT (SELECT COUNT(*) FROM changes WHERE item_kind='backlogItem' AND item_id=?1 AND change_kind='stateChanged'),(SELECT COUNT(*) FROM item_references WHERE source_kind='backlogItem' AND source_id=?2)",
        args: vec![text(&first), text(&second)],
    }).await.unwrap();
    assert_eq!(
        effects.rows,
        vec![vec![
            central_storage::SqlValue::Integer(central_storage::SafeInteger::new(1).unwrap()),
            central_storage::SqlValue::Integer(central_storage::SafeInteger::new(0).unwrap())
        ]]
    );
    for (request, operation) in [
        (2007, close(&first, "Again", None)),
        (
            2008,
            m::WorkspaceOperation::UpdateBacklogItem {
                item_id: first.clone(),
                title: name("Changed"),
                description: "Updated".to_owned().try_into().unwrap(),
            },
        ),
    ] {
        assert_eq!(
            f.call(request, operation).await.result,
            m::WorkspaceResult::Refused(m::RefusalReason::BacklogItemClosed {
                item_id: first.clone()
            })
        );
    }
    assert_eq!(show_backlog(&f, &first).await, item);
    f.host.close().await;
}

#[tokio::test]
async fn r21_invalid_close_and_late_reference_failure_leave_all_effects_unapplied() {
    let f = Fixture::new().await;
    let area = f.area(2020).await;
    let project = f.project(2021, area.clone()).await;
    let target = m::ItemRef::Deliverable(f.deliverable(2022, project).await);
    let item = add_backlog(&f, 2023, area).await;
    let original = show_backlog(&f, &item).await;
    assert_eq!(
        f.call(2024, close(&item, " \t ", None)).await.result,
        m::WorkspaceResult::Refused(m::RefusalReason::EmptyDescription)
    );
    assert!(matches!(
        f.call(
            2025,
            close(&item, "Missing target", Some(m::ItemRef::Issue(id(9999))))
        )
        .await
        .result,
        m::WorkspaceResult::NotFound { .. }
    ));
    assert_eq!(show_backlog(&f, &item).await, original);
    let runtime = f.host.central(&f.caller.organization_id).await;
    let snapshot = || Statement {
        sql: "SELECT status,closing_note,(SELECT COUNT(*) FROM item_references),(SELECT COUNT(*) FROM changes) FROM backlog_items WHERE item_id=?",
        args: vec![text(&item)],
    };
    let before = runtime
        .inspect_workspace(&f.workspace, snapshot())
        .await
        .unwrap();
    // This fails the second statement, after the status/note UPDATE has run.
    f.workspace_execute("CREATE TRIGGER reject_closing_reference BEFORE INSERT ON item_references BEGIN SELECT RAISE(ABORT,'fixture late reference failure'); END", vec![], central_storage::RowExpectation::Any).await;
    let operation = close(&item, "Delivered", Some(target));
    let failed = f.call(2026, operation.clone()).await;
    assert!(matches!(
        failed.result,
        m::WorkspaceResult::Conflict | m::WorkspaceResult::CentralUnavailable
    ));
    assert_eq!(
        runtime
            .inspect_workspace(&f.workspace, snapshot())
            .await
            .unwrap(),
        before
    );
    let logged = runtime
        .inspect_workspace(
            &f.workspace,
            Statement {
                sql: "SELECT request_id FROM request_log WHERE request_id=?",
                args: vec![text(id::<m::RequestId>(2026))],
            },
        )
        .await
        .unwrap();
    assert!(
        logged.rows.is_empty(),
        "late batch failure cannot retain success"
    );
    f.workspace_execute(
        "DROP TRIGGER reject_closing_reference",
        vec![],
        central_storage::RowExpectation::Any,
    )
    .await;
    assert!(matches!(
        f.call(2026, operation).await.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    f.host.close().await;
}

#[tokio::test]
async fn r32_self_join_roles_and_promotion_allow_posts_but_adding_others_names_creator() {
    let f = Fixture::new().await;
    let area = f.area(2040).await;
    let thread = f.thread(2041, area).await;
    let mut other = f.caller.clone();
    let mut address = session(&other);
    address.session_id = "owner-change-self-join".to_owned().try_into().unwrap();
    other.caller = m::Caller::Session(address);
    let join = |role| m::WorkspaceOperation::JoinThread {
        thread_id: thread.clone(),
        role,
    };
    let first = call(
        &f.host,
        &other,
        Some(f.workspace.clone()),
        2042,
        join(m::MemberRole::Observer),
    )
    .await;
    assert!(matches!(
        first.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    let post = || m::WorkspaceOperation::ThreadPost {
        thread_id: thread.clone(),
        body: "Participation".to_owned().try_into().unwrap(),
        addressees: vec![],
    };
    assert_eq!(
        call(&f.host, &other, Some(f.workspace.clone()), 2043, post())
            .await
            .result,
        m::WorkspaceResult::Refused(m::RefusalReason::NotParticipant {
            thread_id: thread.clone()
        })
    );
    let promoted = call(
        &f.host,
        &other,
        Some(f.workspace.clone()),
        2044,
        join(m::MemberRole::Participant),
    )
    .await;
    assert!(matches!(
        promoted.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    created(call(&f.host, &other, Some(f.workspace.clone()), 2045, post()).await);
    let runtime = f.host.central(&f.caller.organization_id).await;
    let creator = runtime
        .inspect_workspace(
            &f.workspace,
            Statement {
                sql: "SELECT creator FROM threads WHERE thread_id=?",
                args: vec![text(&thread)],
            },
        )
        .await
        .unwrap();
    let SqlValue::Text(creator_id) = &creator.rows[0][0] else {
        panic!("creator");
    };
    let members = runtime.inspect_workspace(&f.workspace, Statement {
        sql: "SELECT actor_identity_id FROM thread_members WHERE thread_id=? AND actor_identity_id<>? AND left_at IS NULL",
        args: vec![text(&thread), text(creator_id)],
    }).await.unwrap();
    let SqlValue::Text(target) = &members.rows[0][0] else {
        panic!("member");
    };
    let target: m::ActorIdentityId = target.parse().unwrap();
    // The known creator is a distinct target, so a noncreator Add never becomes self-join.
    for (request, role) in [
        (2046, m::MemberRole::Observer),
        (2047, m::MemberRole::Participant),
    ] {
        assert_eq!(
            call(
                &f.host,
                &other,
                Some(f.workspace.clone()),
                request,
                m::WorkspaceOperation::AddThreadMember {
                    thread_id: thread.clone(),
                    actor: creator_id.parse().unwrap(),
                    role,
                }
            )
            .await
            .result,
            m::WorkspaceResult::Refused(m::RefusalReason::NotCreator {
                creator: creator_id.parse().unwrap()
            })
        );
    }
    f.call(
        2048,
        m::WorkspaceOperation::AddThreadMember {
            thread_id: thread.clone(),
            actor: target.clone(),
            role: m::MemberRole::Observer,
        },
    )
    .await;
    let mut fresh = other.clone();
    let mut address = session(&fresh);
    address.session_id = "fresh-self-participant".to_owned().try_into().unwrap();
    fresh.caller = m::Caller::Session(address);
    // Register the third actor through an ordinary read, before it is a member.
    call(
        &f.host,
        &fresh,
        Some(f.workspace.clone()),
        0,
        m::WorkspaceOperation::Show {
            item: m::ItemRef::Thread(thread.clone()),
        },
    )
    .await;
    let actors = runtime.statement_host().read(Statement {
        sql: "SELECT actor_identity_id FROM actor_identities WHERE json_extract(identity,'$.session.sessionId')=?",
        args: vec![text("fresh-self-participant")],
    }).await.unwrap();
    let SqlValue::Text(fresh_actor) = &actors.rows[0][0] else {
        panic!("third actor");
    };
    assert_eq!(
        call(
            &f.host,
            &other,
            Some(f.workspace.clone()),
            2054,
            m::WorkspaceOperation::AddThreadMember {
                thread_id: thread.clone(),
                actor: fresh_actor.parse().unwrap(),
                role: m::MemberRole::Participant,
            }
        )
        .await
        .result,
        m::WorkspaceResult::Refused(m::RefusalReason::NotCreator {
            creator: creator_id.parse().unwrap()
        })
    );
    assert!(matches!(
        f.call(
            2055,
            m::WorkspaceOperation::AddThreadMember {
                thread_id: thread.clone(),
                actor: fresh_actor.parse().unwrap(),
                role: m::MemberRole::Observer,
            }
        )
        .await
        .result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    call(
        &f.host,
        &fresh,
        Some(f.workspace.clone()),
        2056,
        m::WorkspaceOperation::LeaveThread {
            thread_id: thread.clone(),
        },
    )
    .await;
    assert!(matches!(
        call(
            &f.host,
            &fresh,
            Some(f.workspace.clone()),
            2049,
            join(m::MemberRole::Participant)
        )
        .await
        .result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    created(call(&f.host, &fresh, Some(f.workspace.clone()), 2050, post()).await);
    assert_eq!(
        call(
            &f.host,
            &fresh,
            Some(f.workspace.clone()),
            2051,
            m::WorkspaceOperation::AddThreadMember {
                thread_id: thread.clone(),
                actor: target.clone(),
                role: m::MemberRole::Participant,
            }
        )
        .await
        .result,
        m::WorkspaceResult::Refused(m::RefusalReason::NotCreator {
            creator: creator_id.parse().unwrap()
        })
    );
    assert!(matches!(
        f.call(
            2052,
            m::WorkspaceOperation::AddThreadMember {
                thread_id: thread.clone(),
                actor: target,
                role: m::MemberRole::Participant,
            }
        )
        .await
        .result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    created(call(&f.host, &other, Some(f.workspace.clone()), 2053, post()).await);
    f.host.close().await;
}

mod merged_drains;
