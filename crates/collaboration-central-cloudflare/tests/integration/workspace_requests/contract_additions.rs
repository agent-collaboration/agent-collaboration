//! The Router link's contract additions on the real request path: first and
//! closing posts, renames, the workspace directory, the new lists and the
//! per-scope inbox.
use super::*;

fn post_text(value: &str) -> m::PostText {
    value.to_owned().try_into().unwrap()
}
fn start(order: m::ListOrder) -> m::ListCursor {
    m::ListCursor::start(order)
}
fn page(reply: &m::WorkspaceReply) -> (Vec<m::ItemView>, Option<m::ListCursor>) {
    match &reply.result {
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Page { items, next_cursor }) => {
            (items.clone(), *next_cursor)
        }
        other => panic!("not a page: {other:?}"),
    }
}
fn refs(items: &[m::ItemView]) -> Vec<m::ItemRef> {
    items.iter().map(m::ItemView::item_ref).collect()
}
fn bodies(items: &[m::ItemView]) -> Vec<String> {
    items
        .iter()
        .map(|item| match item {
            m::ItemView::Post(post) => post.body.as_str().to_owned(),
            other => panic!("not a post: {other:?}"),
        })
        .collect()
}
impl Fixture {
    async fn list(
        &self,
        query: m::ListQuery,
        cursor: m::ListCursor,
        limit: u16,
    ) -> m::WorkspaceReply {
        self.call(
            0,
            m::WorkspaceOperation::List {
                query,
                cursor,
                limit: limit.try_into().unwrap(),
            },
        )
        .await
    }
    async fn inbox(&self) -> Vec<m::InboxEntry> {
        let reply = self
            .call(
                0,
                m::WorkspaceOperation::Inbox {
                    after: None,
                    limit: 100.try_into().unwrap(),
                },
            )
            .await;
        match reply.result {
            m::WorkspaceResult::Done(m::WorkspaceAnswer::Inbox { entries, .. }) => entries,
            other => panic!("inbox refused: {other:?}"),
        }
    }
}

#[tokio::test]
async fn a_thread_and_its_first_post_are_one_change_set_and_the_post_reaches_the_inbox() {
    let f = Fixture::new().await;
    let area = f.area(2000).await;
    let create = m::WorkspaceOperation::CreateThread {
        area_id: area.clone(),
        title: name("Kickoff"),
        body: post_text("Opening words"),
        addressees: vec![],
    };

    let reply = f.call(2001, create.clone()).await;
    let replayed = f.call(2001, create).await;

    assert_eq!(reply, replayed);
    let m::ItemRef::Thread(thread) = created(reply) else {
        panic!("thread");
    };
    let (posts, _) = page(
        &f.list(
            m::ListQuery::Posts {
                scope: m::PostScope::Thread {
                    thread_id: thread.clone(),
                },
            },
            start(m::ListOrder::OldestFirst),
            10,
        )
        .await,
    );
    assert_eq!(bodies(&posts), vec!["Opening words".to_owned()]);
    let inbox = f.inbox().await;
    let [opened, posted] = inbox.as_slice() else {
        panic!("thread opening and first post: {inbox:?}");
    };
    assert_eq!(opened.change.event, m::ChangeEvent::Created);
    assert_eq!(posted.change.event, m::ChangeEvent::Posted);
    assert_eq!(posted.position.get(), opened.position.get() + 1);
    assert!(
        matches!(&opened.item, m::ItemView::Thread { thread, .. } if thread.title.as_str() == "Kickoff")
    );
    assert!(matches!(&posted.item, m::ItemView::Post(post) if post.thread_id == thread));
    f.host.close().await;
}

#[tokio::test]
async fn a_closing_post_lands_before_the_thread_resolves_and_nothing_posts_after() {
    let f = Fixture::new().await;
    let area = f.area(2010).await;
    let thread = f.thread(2011, area.clone()).await;

    let resolved = f
        .call(
            2012,
            m::WorkspaceOperation::ResolveThread {
                thread_id: thread.clone(),
                closing_post: Some(post_text("Closing summary")),
            },
        )
        .await;
    let late = f
        .call(
            2013,
            m::WorkspaceOperation::ThreadPost {
                thread_id: thread.clone(),
                body: post_text("Too late"),
                addressees: vec![],
            },
        )
        .await;

    assert!(matches!(
        resolved.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    assert!(matches!(
        late.result,
        m::WorkspaceResult::Refused(m::RefusalReason::ThreadResolved { .. })
    ));
    let (posts, _) = page(
        &f.list(
            m::ListQuery::Posts {
                scope: m::PostScope::Thread {
                    thread_id: thread.clone(),
                },
            },
            start(m::ListOrder::NewestFirst),
            10,
        )
        .await,
    );
    assert_eq!(
        bodies(&posts),
        vec!["Closing summary".to_owned(), "First post".to_owned()]
    );
    let inbox = f.inbox().await;
    let mut newest = inbox.iter().rev();
    let resolved = newest.next().expect("resolved change");
    let posted = newest.next().expect("closing post change");
    assert_eq!(resolved.change.event, m::ChangeEvent::Resolved);
    assert_eq!(posted.change.event, m::ChangeEvent::Posted);
    assert_eq!(resolved.position.get(), posted.position.get() + 1);
    f.host.close().await;
}

#[tokio::test]
async fn renames_change_only_the_named_area_project_and_issue() {
    let f = Fixture::new().await;
    let area = f.area(2020).await;
    let project = f.project(2021, area.clone()).await;
    let deliverable = f.deliverable(2022, project.clone()).await;
    let m::ItemRef::Issue(issue) = created(
        f.call(
            2023,
            m::WorkspaceOperation::CreateIssue {
                deliverable_id: deliverable,
                title: "Old issue".to_owned().try_into().unwrap(),
            },
        )
        .await,
    ) else {
        panic!("issue");
    };

    for (request, operation) in [
        (
            2024,
            m::WorkspaceOperation::UpdateArea {
                area_id: area.clone(),
                name: name("Renamed area"),
                description: None,
            },
        ),
        (
            2025,
            m::WorkspaceOperation::UpdateProject {
                project_id: project.clone(),
                title: name("Renamed project"),
                description: None,
            },
        ),
        (
            2026,
            m::WorkspaceOperation::UpdateIssue {
                issue_id: issue.clone(),
                title: "Renamed issue".to_owned().try_into().unwrap(),
            },
        ),
    ] {
        let reply = f.call(request, operation).await;
        assert!(
            matches!(
                reply.result,
                m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
            ),
            "{reply:?}"
        );
    }
    let missing = f
        .call(
            2027,
            m::WorkspaceOperation::UpdateIssue {
                issue_id: id(2999),
                title: "Nobody".to_owned().try_into().unwrap(),
            },
        )
        .await;

    let show = |item| m::WorkspaceOperation::Show { item };
    assert!(matches!(
        f.call(0, show(m::ItemRef::Area(area))).await.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Item { item: m::ItemView::Area(row) })
            if row.name.as_str() == "Renamed area"
    ));
    assert!(matches!(
        f.call(0, show(m::ItemRef::Project(project))).await.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Item { item: m::ItemView::Project(row) })
            if row.title.as_str() == "Renamed project"
    ));
    assert!(matches!(
        f.call(0, show(m::ItemRef::Issue(issue))).await.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Item { item: m::ItemView::Issue(row) })
            if row.title.as_str() == "Renamed issue"
    ));
    assert!(matches!(
        missing.result,
        m::WorkspaceResult::NotFound {
            item: m::ItemRef::Issue(_),
            ..
        }
    ));
    f.host.close().await;
}

#[tokio::test]
async fn the_workspace_directory_renames_archives_replays_and_lists_only_allowed_workspaces() {
    let f = Fixture::new().await;
    let directory =
        |workspace, request, operation| call(&f.host, &f.caller, workspace, request, operation);
    let list = |include_archived| m::WorkspaceOperation::List {
        query: m::ListQuery::Workspaces { include_archived },
        cursor: start(m::ListOrder::OldestFirst),
        limit: 10.try_into().unwrap(),
    };
    let listed = |reply: m::WorkspaceReply| refs(&page(&reply).0);

    let renamed = directory(
        Some(f.workspace.clone()),
        2030,
        m::WorkspaceOperation::UpdateWorkspace {
            name: name("Renamed"),
            description: "About this workspace".to_owned().try_into().unwrap(),
        },
    )
    .await;
    let second = directory(
        None,
        2031,
        m::WorkspaceOperation::CreateWorkspace {
            name: name("Second"),
        },
    )
    .await;
    let clash = directory(
        Some(f.workspace.clone()),
        2032,
        m::WorkspaceOperation::UpdateWorkspace {
            name: name(" SECOND "),
            description: "".to_owned().try_into().unwrap(),
        },
    )
    .await;
    let shown = directory(
        Some(f.workspace.clone()),
        0,
        m::WorkspaceOperation::Show {
            item: m::ItemRef::Workspace(f.workspace.clone()),
        },
    )
    .await;
    let active = directory(None, 0, list(false)).await;
    let misplaced = directory(Some(f.workspace.clone()), 0, list(false)).await;
    let archive = || {
        directory(
            Some(f.workspace.clone()),
            2033,
            m::WorkspaceOperation::ArchiveWorkspace,
        )
    };
    let archived = archive().await;
    let retried = archive().await;
    let after_archive = directory(
        Some(f.workspace.clone()),
        0,
        m::WorkspaceOperation::Show {
            item: m::ItemRef::Workspace(f.workspace.clone()),
        },
    )
    .await;

    assert!(matches!(
        &renamed.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceUpdated { workspace })
            if workspace.name.as_str() == "Renamed" && workspace.description.as_str() == "About this workspace"
    ));
    assert!(matches!(
        second.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceCreated { .. })
    ));
    assert!(matches!(
        clash.result,
        m::WorkspaceResult::Refused(m::RefusalReason::WorkspaceNameInUse { .. })
    ));
    assert!(matches!(
        &shown.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Item { item: m::ItemView::Workspace(workspace) })
            if workspace.description.as_str() == "About this workspace"
    ));
    // The second workspace exists but this machine is not allowed it.
    assert_eq!(
        listed(active),
        vec![m::ItemRef::Workspace(f.workspace.clone())]
    );
    assert_eq!(
        misplaced.result,
        m::WorkspaceResult::Refused(m::RefusalReason::InvalidInput {
            field: "workspaceId".to_owned()
        })
    );
    assert!(matches!(
        &archived.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceUpdated { workspace })
            if workspace.state == m::WorkspaceState::Archived
    ));
    assert_eq!(retried, archived);
    assert_eq!(
        after_archive.result,
        m::WorkspaceResult::Refused(m::RefusalReason::WorkspaceNotAllowed)
    );
    assert_eq!(listed(directory(None, 0, list(false)).await), vec![]);
    assert_eq!(
        listed(directory(None, 0, list(true)).await),
        vec![m::ItemRef::Workspace(f.workspace.clone())]
    );
    f.host.close().await;
}

#[tokio::test]
async fn new_lists_page_newest_first_and_filter_members_roles_issues_and_tags() {
    let f = Fixture::new().await;
    let area = f.area(2040).await;
    let thread = f.thread(2041, area.clone()).await;
    let other = f.thread(2042, area.clone()).await;
    for (request, body) in [(2043, "two"), (2044, "three"), (2045, "four")] {
        created(
            f.call(
                request,
                m::WorkspaceOperation::ThreadPost {
                    thread_id: thread.clone(),
                    body: post_text(body),
                    addressees: vec![],
                },
            )
            .await,
        );
    }
    f.call(
        2046,
        m::WorkspaceOperation::LeaveThread {
            thread_id: other.clone(),
        },
    )
    .await;
    let project = f.project(2047, area.clone()).await;
    let first = f.deliverable(2048, project.clone()).await;
    let second = f.deliverable(2049, project.clone()).await;
    for (request, deliverable) in [(2050, first), (2051, second)] {
        created(
            f.call(
                request,
                m::WorkspaceOperation::CreateIssue {
                    deliverable_id: deliverable,
                    title: "Issue".to_owned().try_into().unwrap(),
                },
            )
            .await,
        );
    }
    let tag = |value: &str| -> m::TrailTag { value.to_owned().try_into().unwrap() };
    for (request, tags) in [
        (2052, vec![tag("decision"), tag("api")]),
        (2053, vec![tag("decision")]),
    ] {
        created(
            f.call(
                request,
                m::WorkspaceOperation::WorkLogAdd {
                    project_id: project.clone(),
                    body: "Entry".to_owned().try_into().unwrap(),
                    tags,
                },
            )
            .await,
        );
    }
    let posts = m::ListQuery::Posts {
        scope: m::PostScope::Thread {
            thread_id: thread.clone(),
        },
    };

    let (newest, cursor) = page(
        &f.list(posts.clone(), start(m::ListOrder::NewestFirst), 2)
            .await,
    );
    let (older, end) = page(&f.list(posts, cursor.unwrap(), 2).await);
    let (followed, _) = page(
        &f.list(
            m::ListQuery::Threads {
                area_id: None,
                followed_only: true,
            },
            start(m::ListOrder::OldestFirst),
            10,
        )
        .await,
    );
    let members = |include_left| m::ListQuery::ThreadMembers {
        thread_id: other.clone(),
        include_left,
    };
    let (current_members, _) = page(
        &f.list(members(false), start(m::ListOrder::OldestFirst), 10)
            .await,
    );
    let (all_members, _) = page(
        &f.list(members(true), start(m::ListOrder::OldestFirst), 10)
            .await,
    );
    let (roles, _) = page(
        &f.list(
            m::ListQuery::ProjectRoles {
                project_id: project.clone(),
                include_ended: false,
            },
            start(m::ListOrder::OldestFirst),
            10,
        )
        .await,
    );
    let (issues, _) = page(
        &f.list(
            m::ListQuery::ProjectIssues {
                project_id: project.clone(),
            },
            start(m::ListOrder::OldestFirst),
            10,
        )
        .await,
    );
    let (both_tags, _) = page(
        &f.list(
            m::ListQuery::WorkLog {
                project_id: project.clone(),
                tags: vec![tag("api"), tag("decision")],
            },
            start(m::ListOrder::OldestFirst),
            10,
        )
        .await,
    );
    let (found, _) = page(
        &f.list(
            m::ListQuery::SearchItems {
                text: "PROJECT".to_owned().try_into().unwrap(),
                item_kind: m::SearchableKind::All,
                include_archived: false,
            },
            start(m::ListOrder::OldestFirst),
            10,
        )
        .await,
    );

    assert_eq!(bodies(&newest), vec!["four".to_owned(), "three".to_owned()]);
    assert_eq!(
        bodies(&older),
        vec!["two".to_owned(), "First post".to_owned()]
    );
    assert_eq!(end, None);
    assert_eq!(refs(&followed), vec![m::ItemRef::Thread(thread)]);
    assert!(current_members.is_empty());
    assert!(matches!(
        all_members.as_slice(),
        [m::ItemView::ThreadMember(member)] if matches!(member.state, m::MembershipState::Left { .. })
    ));
    assert!(matches!(
        roles.as_slice(),
        [m::ItemView::ProjectRole(role)] if role.role == m::ProjectRole::Lead
    ));
    assert_eq!(issues.len(), 2);
    assert_eq!(both_tags.len(), 1);
    assert_eq!(refs(&found), vec![m::ItemRef::Project(project)]);
    f.host.close().await;
}
