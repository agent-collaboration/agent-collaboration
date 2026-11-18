//! Contract slice 2: a resolved thread reopens for its creator; area and
//! project updates are partial; a workspace's description is optional; search
//! matches area and project descriptions.
use super::*;

fn text_value(value: &str) -> m::Description {
    value.to_owned().try_into().unwrap()
}
fn workspace_text(value: &str) -> m::WorkspaceDescription {
    value.to_owned().try_into().unwrap()
}
fn required(value: &str) -> m::RequiredDescription {
    value.to_owned().try_into().unwrap()
}
fn invalid(field: &str) -> m::WorkspaceResult {
    m::WorkspaceResult::Refused(m::RefusalReason::InvalidInput {
        field: field.to_owned(),
    })
}
fn updated(reply: &m::WorkspaceReply) -> bool {
    matches!(
        reply.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    )
}
async fn shown(f: &Fixture, item: m::ItemRef) -> m::ItemView {
    let shown = f.call(0, m::WorkspaceOperation::Show { item }).await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Item { item }) = shown.result else {
        panic!("show: {shown:?}");
    };
    item
}
async fn listed(f: &Fixture, query: m::ListQuery) -> Vec<m::ItemView> {
    let page = f
        .call(
            0,
            m::WorkspaceOperation::List {
                query,
                cursor: m::ListCursor::start(m::ListOrder::OldestFirst),
                limit: m::PageLimit::try_from(50).unwrap(),
            },
        )
        .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Page { items, .. }) = page.result else {
        panic!("page: {page:?}");
    };
    items
}
async fn thread_state(f: &Fixture, thread: &m::ThreadId) -> m::ThreadState {
    match shown(f, m::ItemRef::Thread(thread.clone())).await {
        m::ItemView::Thread { thread, .. } => thread.state,
        other => panic!("thread: {other:?}"),
    }
}

/// A resolved thread reopens for its creator only; reopening an open thread is
/// refused; memberships and notification settings are untouched.
#[tokio::test]
async fn a_resolved_thread_reopens_for_its_creator_and_keeps_its_memberships() {
    // Arrange: a thread with a second member whose notifications are off.
    let f = Fixture::new().await;
    let area = f.area(2100).await;
    let thread = f.thread(2101, area).await;
    let observer = {
        let mut caller = f.caller.clone();
        let mut address = session(&f.caller);
        address.session_id = "reopen-observer".to_owned().try_into().unwrap();
        caller.caller = m::Caller::Session(address);
        caller
    };
    for (request, operation) in [
        (
            2102,
            m::WorkspaceOperation::JoinThread {
                thread_id: thread.clone(),
                role: m::MemberRole::Observer,
            },
        ),
        (
            2103,
            m::WorkspaceOperation::SetNotifications {
                thread_id: thread.clone(),
                setting: m::NotificationSetting::Off,
            },
        ),
    ] {
        let reply = call(
            &f.host,
            &fresh_auth(&f.host, &observer),
            Some(f.workspace.clone()),
            request,
            operation,
        )
        .await;
        assert!(
            matches!(reply.result, m::WorkspaceResult::Done(_)),
            "{reply:?}"
        );
    }
    let members = || {
        listed(
            &f,
            m::ListQuery::ThreadMembers {
                thread_id: thread.clone(),
                include_left: true,
            },
        )
    };
    let before = members().await;
    let reopen = || m::WorkspaceOperation::ReopenThread {
        thread_id: thread.clone(),
    };

    // Act and assert: an open thread does not reopen.
    assert_eq!(
        f.call(2104, reopen()).await.result,
        m::WorkspaceResult::Refused(m::RefusalReason::ThreadOpen {
            thread_id: thread.clone()
        })
    );
    assert!(updated(
        &f.call(
            2105,
            m::WorkspaceOperation::ResolveThread {
                thread_id: thread.clone(),
                closing_post: None,
            },
        )
        .await
    ));
    // Only the creator may reopen it, the same authority as resolving.
    let refused = call(
        &f.host,
        &fresh_auth(&f.host, &observer),
        Some(f.workspace.clone()),
        2106,
        reopen(),
    )
    .await;
    assert!(
        matches!(
            refused.result,
            m::WorkspaceResult::Refused(m::RefusalReason::NotCreator { .. })
        ),
        "{refused:?}"
    );
    assert_eq!(thread_state(&f, &thread).await, m::ThreadState::Resolved);
    assert!(updated(&f.call(2107, reopen()).await));
    assert_eq!(thread_state(&f, &thread).await, m::ThreadState::Open);
    assert_eq!(members().await, before);
    // An open thread takes posts again.
    let post = f
        .call(
            2108,
            m::WorkspaceOperation::ThreadPost {
                thread_id: thread.clone(),
                body: "after reopening".to_owned().try_into().unwrap(),
                addressees: vec![],
            },
        )
        .await;
    assert!(
        matches!(
            post.result,
            m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { .. })
        ),
        "{post:?}"
    );
    drop(observer);
    f.host.close().await;
}

/// Area and project updates are partial: at least one field, an absent one
/// keeps its stored value, and present values are never blank.
#[tokio::test]
async fn area_and_project_updates_are_partial_and_never_blank() {
    let f = Fixture::new().await;
    let area = f.area(2200).await;
    let project = f.project(2201, area.clone()).await;
    let area_update =
        |name: Option<&str>, description: Option<&str>| m::WorkspaceOperation::UpdateArea {
            area_id: area.clone(),
            name: name.map(super::name),
            description: description.map(text_value),
        };
    let project_update =
        |title: Option<&str>, description: Option<&str>| m::WorkspaceOperation::UpdateProject {
            project_id: project.clone(),
            title: title.map(super::name),
            description: description.map(text_value),
        };

    // Neither field, or a blank description, is refused.
    assert_eq!(
        f.call(2202, area_update(None, None)).await.result,
        invalid("nameOrDescription")
    );
    assert_eq!(
        f.call(2203, project_update(None, None)).await.result,
        invalid("titleOrDescription")
    );
    assert_eq!(
        f.call(2204, area_update(Some("Kept"), Some("  ")))
            .await
            .result,
        invalid("description")
    );
    assert_eq!(
        f.call(2205, project_update(None, Some(""))).await.result,
        invalid("description")
    );

    // A name alone keeps the description; a description alone keeps the name.
    assert!(updated(
        &f.call(2206, area_update(Some("Renamed area"), None)).await
    ));
    assert!(updated(
        &f.call(2207, area_update(None, Some("New area purpose")))
            .await
    ));
    assert!(updated(
        &f.call(2208, project_update(Some("Renamed project"), None))
            .await
    ));
    assert!(updated(
        &f.call(2209, project_update(None, Some("New project goal")))
            .await
    ));
    let m::ItemView::Area(area_view) = shown(&f, m::ItemRef::Area(area.clone())).await else {
        panic!("area");
    };
    assert_eq!(area_view.name, super::name("Renamed area"));
    assert_eq!(area_view.description, required("New area purpose"));
    let m::ItemView::Project(project_view) = shown(&f, m::ItemRef::Project(project.clone())).await
    else {
        panic!("project");
    };
    assert_eq!(project_view.title, super::name("Renamed project"));
    assert_eq!(project_view.description, required("New project goal"));

    // Both at once replace both.
    assert!(updated(
        &f.call(2210, project_update(Some("Final"), Some("Final goal")))
            .await
    ));
    let m::ItemView::Project(project_view) = shown(&f, m::ItemRef::Project(project)).await else {
        panic!("project");
    };
    assert_eq!(project_view.title, super::name("Final"));
    assert_eq!(project_view.description, required("Final goal"));
    f.host.close().await;
}

/// A workspace's description is optional and stored empty when absent; search
/// matches area and project descriptions as it matches their titles.
#[tokio::test]
async fn workspace_descriptions_default_to_empty_and_search_matches_item_descriptions() {
    let f = Fixture::new().await;
    for (request, workspace_name, description, expected) in [
        (
            2300,
            "Described workspace",
            Some("What this workspace holds"),
            "What this workspace holds",
        ),
        (2301, "Plain workspace", None, ""),
    ] {
        let reply = call(
            &f.host,
            &fresh_auth(&f.host, &f.caller),
            None,
            request,
            m::WorkspaceOperation::CreateWorkspace {
                name: super::name(workspace_name),
                description: description.map(workspace_text),
            },
        )
        .await;
        let m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceCreated { workspace }) =
            reply.result
        else {
            panic!("create workspace: {reply:?}");
        };
        assert_eq!(workspace.description.as_str(), expected);
    }

    let m::ItemRef::Area(area) = created(
        f.call(
            2302,
            m::WorkspaceOperation::CreateArea {
                name: super::name("Plain area"),
                description: text_value("Holds the zebra migration"),
            },
        )
        .await,
    ) else {
        panic!("area");
    };
    let m::ItemRef::Project(project) = created(
        f.call(
            2303,
            m::WorkspaceOperation::CreateProject {
                area_id: area.clone(),
                title: super::name("Plain project"),
                description: text_value("Ships the zebra importer"),
                lead: None,
            },
        )
        .await,
    ) else {
        panic!("project");
    };
    let found = listed(
        &f,
        m::ListQuery::SearchItems {
            text: "zebra".to_owned().try_into().unwrap(),
            item_kind: m::SearchableKind::All,
            include_archived: false,
        },
    )
    .await;
    let refs: Vec<_> = found.iter().map(m::ItemView::item_ref).collect();
    assert!(refs.contains(&m::ItemRef::Area(area)), "{refs:?}");
    assert!(refs.contains(&m::ItemRef::Project(project)), "{refs:?}");
    f.host.close().await;
}

/// A workspace description holds at most 2,048 UTF-8 bytes, the bound the
/// directory decoder enforces: 2,048 is stored and every directory read still
/// decodes; 2,049 never leaves the wire decode, so it can never be stored.
#[tokio::test]
async fn a_workspace_description_is_bounded_at_the_directory_decoders_2048_bytes() {
    let f = Fixture::new().await;
    let longest = format!(
        "boundary {}",
        "x".repeat(m::WORKSPACE_DESCRIPTION_MAX_BYTES - 9)
    );
    let request = m::WorkspaceRequest {
        request_id: Some(id(2400)),
        workspace_id: None,
        operation: m::WorkspaceOperation::CreateWorkspace {
            name: super::name("Longest description"),
            description: Some(workspace_text(&longest)),
        },
    };

    // Act: one byte over the bound, on the request's own wire form.
    let mut over = serde_json::to_value(&request).unwrap();
    over["operation"]["description"] = serde_json::json!(format!("{longest}x"));
    let decoded = serde_json::from_value::<m::WorkspaceRequest>(over);
    let reply = call(
        &f.host,
        &fresh_auth(&f.host, &f.caller),
        None,
        2400,
        request.operation.clone(),
    )
    .await;

    // Assert
    assert!(decoded.is_err(), "a 2,049-byte description decoded");
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceCreated { workspace }) = reply.result
    else {
        panic!("create workspace: {reply:?}");
    };
    assert_eq!(workspace.description.as_str(), longest);
    for (request, query) in [
        (
            2401,
            m::ListQuery::Workspaces {
                include_archived: true,
            },
        ),
        (
            2402,
            m::ListQuery::SearchWorkspaces {
                text: "boundary".to_owned().try_into().unwrap(),
                include_archived: true,
            },
        ),
    ] {
        let listed = call(
            &f.host,
            &fresh_auth(&f.host, &f.caller),
            None,
            request,
            m::WorkspaceOperation::List {
                query,
                cursor: m::ListCursor::start(m::ListOrder::OldestFirst),
                limit: 10.try_into().unwrap(),
            },
        )
        .await;
        // Every directory read decodes the whole organization directory before it
        // filters by the caller's allowance, so a page means the stored row decoded.
        assert!(
            matches!(
                listed.result,
                m::WorkspaceResult::Done(m::WorkspaceAnswer::Page { .. })
            ),
            "directory read after the longest description: {listed:?}"
        );
    }
    f.host.close().await;
}

/// A reopen reaches the area inbox once, latest and unread alike, for the creator
/// and for a joined observer with notifications on; a replayed reopen adds nothing.
#[tokio::test]
async fn a_reopen_reaches_the_area_inbox_once_for_the_creator_and_a_joined_observer() {
    // Arrange: a thread the creator resolves, with an observer whose notifications are on.
    let f = Fixture::new().await;
    let area = f.area(2500).await;
    let thread = f.thread(2501, area.clone()).await;
    let observer = {
        let mut caller = f.caller.clone();
        let mut address = session(&f.caller);
        address.session_id = "inbox-observer".to_owned().try_into().unwrap();
        caller.caller = m::Caller::Session(address);
        caller
    };
    let joined = call(
        &f.host,
        &fresh_auth(&f.host, &observer),
        Some(f.workspace.clone()),
        2502,
        m::WorkspaceOperation::JoinThread {
            thread_id: thread.clone(),
            role: m::MemberRole::Observer,
        },
    )
    .await;
    assert!(
        matches!(joined.result, m::WorkspaceResult::Done(_)),
        "{joined:?}"
    );
    assert!(updated(
        &f.call(
            2503,
            m::WorkspaceOperation::ResolveThread {
                thread_id: thread.clone(),
                closing_post: None,
            },
        )
        .await
    ));
    let reopen = m::WorkspaceOperation::ReopenThread {
        thread_id: thread.clone(),
    };

    // Act: reopen, then replay the same request.
    assert!(updated(&f.call(2504, reopen.clone()).await));
    assert!(updated(&f.call(2504, reopen).await));

    // Assert
    for (reader, who) in [(&f.caller, "creator"), (&observer, "observer")] {
        for read_mode in [m::InboxReadMode::Latest, m::InboxReadMode::Unread] {
            let caller = reader;
            let reply = call(
                &f.host,
                &fresh_auth(&f.host, caller),
                Some(f.workspace.clone()),
                0,
                m::WorkspaceOperation::Inbox {
                    scope: m::InboxScope::Area {
                        area_id: area.clone(),
                    },
                    read_mode,
                    after: None,
                    limit: 100.try_into().unwrap(),
                },
            )
            .await;
            let m::WorkspaceResult::Done(m::WorkspaceAnswer::Inbox { items, .. }) = reply.result
            else {
                panic!("{who} inbox: {reply:?}");
            };
            // The thread's own creation is also an Open state row; the reopen is
            // the Open row after the one Resolved row.
            let states: Vec<_> = items
                .iter()
                .filter_map(|item| match item {
                    m::InboxItem::ThreadStateChanged {
                        thread_id,
                        state,
                        position,
                        ..
                    } if *thread_id == thread => Some((*position, *state)),
                    _ => None,
                })
                .collect();
            let resolved: Vec<_> = states
                .iter()
                .filter(|(_, state)| *state == m::ThreadState::Resolved)
                .map(|(position, _)| *position)
                .collect();
            assert_eq!(resolved.len(), 1, "{who} {read_mode:?}: {items:?}");
            let reopened = states
                .iter()
                .filter(|(position, state)| {
                    *state == m::ThreadState::Open && *position > resolved[0]
                })
                .count();
            assert_eq!(reopened, 1, "{who} {read_mode:?}: {items:?}");
        }
    }
    f.host.close().await;
}
