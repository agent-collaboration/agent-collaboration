use super::*;
use collaboration_central_core::PausePoint;
#[tokio::test]
async fn project_close_keeps_its_turn_until_apply_and_new_child_reads_the_closed_parent() {
    let f = Fixture::new().await;
    let area = f.area(300).await;
    let project = f.project(301, area).await;
    let deliverable = f.deliverable(302, project.clone()).await;
    let m::ItemRef::Issue(issue) = created(
        f.call(
            303,
            m::WorkspaceOperation::CreateIssue {
                deliverable_id: deliverable.clone(),
                title: "Existing".to_owned().try_into().unwrap(),
            },
        )
        .await,
    ) else {
        panic!("issue");
    };
    f.call(
        304,
        m::WorkspaceOperation::SetIssueStatus {
            issue_id: issue,
            status: m::WorkStatus::Closed,
        },
    )
    .await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    let seam = f.workspace.clone();
    let mut held = runtime.pause_points().hold(&seam, PausePoint::AfterReads);
    let request = m::WorkspaceRequest {
        request_id: Some(id(305)),
        workspace_id: Some(f.workspace.clone()),
        operation: m::WorkspaceOperation::SetProjectStatus {
            project_id: project.clone(),
            status: m::WorkStatus::Closed,
        },
    };
    let first = tokio::spawn({
        let runtime = runtime.clone();
        let caller = f.caller.clone();
        async move { runtime.request(&caller, request).await.unwrap() }
    });
    held.until_held().await;
    let request = m::WorkspaceRequest {
        request_id: Some(id(306)),
        workspace_id: Some(f.workspace.clone()),
        operation: m::WorkspaceOperation::CreateIssue {
            deliverable_id: deliverable,
            title: "Racing child".to_owned().try_into().unwrap(),
        },
    };
    let second = tokio::spawn({
        let runtime = runtime.clone();
        let caller = f.caller.clone();
        async move { runtime.request(&caller, request).await.unwrap() }
    });
    runtime.workspace_turns().until_waiting(&seam, 1).await;
    assert!(
        !second.is_finished(),
        "the second request cannot inspect a stale parent"
    );
    held.release();
    assert!(matches!(
        first.await.unwrap().result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    assert!(matches!(
        second.await.unwrap().result,
        m::WorkspaceResult::Refused(m::RefusalReason::ParentClosed { .. })
    ));
    f.host.close().await;
}
#[tokio::test]
async fn resolved_thread_is_immutable_after_held_turn_and_refusal_replay_ignores_later_state() {
    let f = Fixture::new().await;
    let area = f.area(320).await;
    let thread = f.thread(321, area).await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    let seam = f.workspace.clone();
    let mut held = runtime
        .pause_points()
        .hold(&seam, PausePoint::AfterStoreApplied);
    let request = m::WorkspaceRequest {
        request_id: Some(id(322)),
        workspace_id: Some(f.workspace.clone()),
        operation: m::WorkspaceOperation::ResolveThread {
            thread_id: thread.clone(),
        },
    };
    let first = tokio::spawn({
        let runtime = runtime.clone();
        let caller = f.caller.clone();
        async move { runtime.request(&caller, request).await.unwrap() }
    });
    held.until_held().await;
    let post = m::WorkspaceOperation::ThreadPost {
        thread_id: thread.clone(),
        body: "Too late".to_owned().try_into().unwrap(),
        addressees: vec![],
    };
    let request = m::WorkspaceRequest {
        request_id: Some(id(323)),
        workspace_id: Some(f.workspace.clone()),
        operation: post.clone(),
    };
    let second = tokio::spawn({
        let runtime = runtime.clone();
        let caller = f.caller.clone();
        async move { runtime.request(&caller, request).await.unwrap() }
    });
    runtime.workspace_turns().until_waiting(&seam, 1).await;
    held.release();
    first.await.unwrap();
    let refusal = second.await.unwrap();
    assert_eq!(
        refusal.result,
        m::WorkspaceResult::Refused(m::RefusalReason::ThreadResolved {
            thread_id: thread.clone()
        })
    );
    // Re-open only as a corruption/fault fixture, so replay must not re-run the rule.
    f.workspace_batch(
        "UPDATE threads SET state='open' WHERE thread_id=?",
        vec![text(thread)],
    )
    .await;
    assert_eq!(refusal, f.call(323, post.clone()).await);
    assert!(matches!(
        f.call(324, post).await.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { .. })
    ));
    f.host.close().await;
}
#[tokio::test]
async fn failed_guard_has_no_ledger_entry_and_mark_read_is_monotonic() {
    let f = Fixture::new().await;
    let area = f.area(340).await;
    let thread = f.thread(341, area).await;
    let m::ItemRef::Link(link) = created(
        f.call(
            342,
            m::WorkspaceOperation::AddLink {
                source: m::ReferenceSource::Thread(thread.clone()),
                url: "https://example.test".to_owned(),
                title: "Link".to_owned(),
                details: None,
            },
        )
        .await,
    ) else {
        panic!("link");
    };
    let runtime = f.host.central(&f.caller.organization_id).await;
    let seam = f.workspace.clone();
    let mut held = runtime.pause_points().hold(&seam, PausePoint::AfterReads);
    let operation = m::WorkspaceOperation::RemoveLink {
        link_id: link.clone(),
    };
    let request = m::WorkspaceRequest {
        request_id: Some(id(343)),
        workspace_id: Some(f.workspace.clone()),
        operation: operation.clone(),
    };
    let pending = tokio::spawn({
        let runtime = runtime.clone();
        let caller = f.caller.clone();
        async move { runtime.request(&caller, request).await.unwrap() }
    });
    held.until_held().await;
    f.workspace_batch("DELETE FROM links WHERE link_id=?", vec![text(&link)])
        .await;
    held.release();
    assert_eq!(pending.await.unwrap().result, m::WorkspaceResult::Conflict);
    let logged = runtime
        .inspect_workspace(
            &f.workspace,
            Statement {
                sql: "SELECT request_id FROM request_log WHERE request_id=?",
                args: vec![text(id::<m::RequestId>(343))],
            },
        )
        .await
        .unwrap();
    assert!(logged.rows.is_empty());
    assert_eq!(
        f.call(343, operation).await.result,
        m::WorkspaceResult::NotFound {
            workspace_id: f.workspace.clone(),
            item: m::ItemRef::Link(link)
        }
    );
    let inbox = f
        .call(
            0,
            m::WorkspaceOperation::Inbox {
                after: None,
                limit: 100.try_into().unwrap(),
            },
        )
        .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Inbox { entries, .. }) = inbox.result else {
        panic!("inbox");
    };
    let last = entries.last().unwrap().position;
    for (request, through) in [(344, last), (345, m::ChangePosition::ZERO)] {
        let mark = f
            .call(
                request,
                m::WorkspaceOperation::MarkRead {
                    scope: m::ReadScope::Inbox,
                    through,
                },
            )
            .await;
        assert_eq!(
            mark.result,
            m::WorkspaceResult::Done(m::WorkspaceAnswer::Marked {
                scope: m::ReadScope::Inbox,
                through: last
            })
        );
    }
    let inbox = f
        .call(
            0,
            m::WorkspaceOperation::Inbox {
                after: None,
                limit: 100.try_into().unwrap(),
            },
        )
        .await;
    assert!(
        matches!(inbox.result,m::WorkspaceResult::Done(m::WorkspaceAnswer::Inbox{entries,read_through,..}) if entries.is_empty()&&read_through==last)
    );
    f.host.close().await;
}

#[tokio::test]
async fn idle_expiry_treats_same_content_as_new_for_both_ledgers() {
    let f = Fixture::new().await;
    let area = m::WorkspaceOperation::CreateArea {
        name: name("Same content"),
    };
    let original = f.call(360, area.clone()).await;
    let central = m::WorkspaceOperation::CreateWorkspace {
        name: name("Same central content"),
    };
    let original_central = call(&f.host, &f.caller, None, 361, central.clone()).await;
    f.clock.0.fetch_add(86_400_001, Ordering::SeqCst);
    let next = f.call(360, area).await;
    assert!(matches!(
        next.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { .. })
    ));
    assert_ne!(original, next);
    let next_central = call(&f.host, &f.caller, None, 361, central).await;
    assert!(matches!(
        next_central.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceCreated { .. })
    ));
    assert_ne!(original_central, next_central);
    f.host.close().await;
}
