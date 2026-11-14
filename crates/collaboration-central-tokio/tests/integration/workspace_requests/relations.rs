use super::*;
#[tokio::test]
async fn reference_link_backlog_and_work_log_rows_are_real_and_removals_refuse_absence() {
    let f = Fixture::new().await;
    let area = f.area(100).await;
    let project = f.project(101, area.clone()).await;
    let thread = f.thread(102, area.clone()).await;
    let post = created(
        f.call(
            103,
            m::WorkspaceOperation::ThreadPost {
                thread_id: thread.clone(),
                body: "100%_ exact".to_owned().try_into().unwrap(),
                addressees: vec![],
            },
        )
        .await,
    );
    let m::ItemRef::Post(post_id) = post.clone() else {
        panic!("post");
    };
    let entry = created(
        f.call(
            104,
            m::WorkspaceOperation::WorkLogAdd {
                project_id: project.clone(),
                body: "original log".to_owned().try_into().unwrap(),
                tags: vec!["tag".to_owned().try_into().unwrap()],
            },
        )
        .await,
    );
    let m::ItemRef::WorkLogEntry(entry_id) = entry.clone() else {
        panic!("entry");
    };
    let source = m::ReferenceSource::Post(post_id);
    let target = m::ReferenceTarget::try_from(m::ItemRef::Project(project)).unwrap();
    let reference = created(
        f.call(
            105,
            m::WorkspaceOperation::AddReference {
                source: source.clone(),
                target,
                details: None,
            },
        )
        .await,
    );
    let m::ItemRef::Reference(reference_id) = reference.clone() else {
        panic!("reference");
    };
    let link = created(
        f.call(
            106,
            m::WorkspaceOperation::AddLink {
                source: source.clone(),
                url: "https://example.test/item".to_owned(),
                title: "External".to_owned(),
                details: None,
            },
        )
        .await,
    );
    let m::ItemRef::Link(link_id) = link.clone() else {
        panic!("link");
    };
    for (request, query, expected) in [
        (
            107,
            m::ListQuery::ReferencesFrom { item: post.clone() },
            reference.clone(),
        ),
        (
            108,
            m::ListQuery::Links { item: post.clone() },
            link.clone(),
        ),
    ] {
        let result = f
            .call(
                request,
                m::WorkspaceOperation::List {
                    query,
                    after: m::ChangePosition::ZERO,
                    limit: 10,
                },
            )
            .await;
        assert!(
            matches!(&result.result,m::WorkspaceResult::Done(m::WorkspaceAnswer::Page{items,..}) if items.len()==1 && items[0].item_ref()==expected),
            "{result:?}"
        );
    }
    let search = f
        .call(
            0,
            m::WorkspaceOperation::List {
                query: m::ListQuery::SearchPosts {
                    area_id: Some(area.clone()),
                    thread_id: Some(thread.clone()),
                    text: "%_".to_owned().try_into().unwrap(),
                },
                after: m::ChangePosition::ZERO,
                limit: 1,
            },
        )
        .await;
    assert!(
        matches!(&search.result,m::WorkspaceResult::Done(m::WorkspaceAnswer::Page{items,..}) if items.len()==1 && items[0].item_ref()==post),
        "{search:?}"
    );
    let redact = f
        .call(
            109,
            m::WorkspaceOperation::WorkLogRedact {
                entry_id: entry_id.clone(),
            },
        )
        .await;
    assert!(
        matches!(
            redact.result,
            m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
        ),
        "{redact:?}"
    );
    let shown = f.call(0, m::WorkspaceOperation::Show { item: entry }).await;
    assert!(
        matches!(
            shown.result,
            m::WorkspaceResult::Done(m::WorkspaceAnswer::Item {
                item: m::ItemView::WorkLogEntry(m::WorkLogEntry {
                    body: m::WorkLogBody::Redacted,
                    ..
                })
            })
        ),
        "{shown:?}"
    );
    let backlog = created(
        f.call(
            110,
            m::WorkspaceOperation::AddBacklogItem {
                area_id: area,
                title: name("Backlog"),
                description: "Description".to_owned().try_into().unwrap(),
            },
        )
        .await,
    );
    let m::ItemRef::BacklogItem(item_id) = backlog.clone() else {
        panic!("backlog");
    };
    for (request, operation, item) in [
        (
            111,
            m::WorkspaceOperation::RemoveReference { reference_id },
            reference,
        ),
        (113, m::WorkspaceOperation::RemoveLink { link_id }, link),
        (
            115,
            m::WorkspaceOperation::RemoveBacklogItem { item_id },
            backlog,
        ),
    ] {
        let removed = f.call(request, operation.clone()).await;
        assert!(
            matches!(&removed.result,m::WorkspaceResult::Done(m::WorkspaceAnswer::Removed{item:found,..}) if found==&item),
            "{removed:?}"
        );
        assert_eq!(
            removed,
            f.call(request, operation.clone()).await,
            "same-id remove replays success"
        );
        let absent = f.call(request + 1, operation.clone()).await;
        assert_eq!(
            absent.result,
            m::WorkspaceResult::NotFound {
                workspace_id: f.workspace.clone(),
                item
            }
        );
        assert_eq!(
            absent,
            f.call(request + 1, operation).await,
            "new-id missing removal caches refusal"
        );
    }
    let inbox = f
        .call(
            0,
            m::WorkspaceOperation::Inbox {
                after: None,
                limit: 100,
            },
        )
        .await;
    assert!(
        matches!(&inbox.result,m::WorkspaceResult::Done(m::WorkspaceAnswer::Inbox{entries,..}) if entries.iter().any(|entry|matches!(entry.item,m::ItemView::Removed{..}))),
        "{inbox:?}"
    );
    f.host.close().await;
}
