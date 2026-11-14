use super::*;
#[tokio::test]
async fn relation_pages_use_created_positions_after_clock_behind_reopen_and_keep_source_metadata() {
    let mut f = Fixture::new().await;
    let first_area = f.area(940).await;
    let second_area = f.area(941).await;
    let thread = f.thread(942, first_area.clone()).await;
    let source = m::ReferenceSource::Thread(thread.clone());
    let reference = |area| m::WorkspaceOperation::AddReference {
        source: source.clone(),
        target: m::ReferenceTarget::try_from(m::ItemRef::Area(area)).unwrap(),
        details: None,
    };
    let link = |title: &str| m::WorkspaceOperation::AddLink {
        source: source.clone(),
        url: title.to_owned(),
        title: title.to_owned(),
        details: None,
    };
    let first_reference = created(f.call(943, reference(first_area)).await);
    let first_link = created(f.call(944, link("First literal")).await);
    f.clock.0.fetch_sub(100_000, Ordering::SeqCst);
    f.reopen().await;
    let second_reference = created(f.call(945, reference(second_area)).await);
    let second_link = created(f.call(946, link("Second literal")).await);
    assert!(
        second_reference.kind_and_id().1 < first_reference.kind_and_id().1,
        "ids intentionally oppose Created positions"
    );
    assert!(second_link.kind_and_id().1 < first_link.kind_and_id().1);
    for (query, first, second) in [
        (
            m::ListQuery::ReferencesFrom {
                item: m::ItemRef::Thread(thread.clone()),
            },
            first_reference.clone(),
            second_reference.clone(),
        ),
        (
            m::ListQuery::Links {
                item: m::ItemRef::Thread(thread.clone()),
            },
            first_link.clone(),
            second_link.clone(),
        ),
    ] {
        let page = f
            .call(
                0,
                m::WorkspaceOperation::List {
                    query: query.clone(),
                    after: m::ChangePosition::ZERO,
                    limit: 1,
                },
            )
            .await;
        let m::WorkspaceResult::Done(m::WorkspaceAnswer::Page {
            items,
            next_after_position: Some(after),
        }) = page.result
        else {
            panic!("first page");
        };
        assert_eq!(
            items.iter().map(m::ItemView::item_ref).collect::<Vec<_>>(),
            vec![first]
        );
        let page = f
            .call(
                0,
                m::WorkspaceOperation::List {
                    query,
                    after,
                    limit: 1,
                },
            )
            .await;
        assert!(
            matches!(page.result,m::WorkspaceResult::Done(m::WorkspaceAnswer::Page{items,next_after_position:None}) if items.iter().map(m::ItemView::item_ref).collect::<Vec<_>>()==vec![second])
        );
    }
    let runtime = f.host.central(&f.caller.organization_id).await;
    for item in [
        &first_reference,
        &second_reference,
        &first_link,
        &second_link,
    ] {
        let (kind, id) = item.kind_and_id();
        let rows = runtime
            .inspect_workspace(
                &f.workspace,
                Statement {
                    sql: "SELECT item_kind,thread_id FROM changes WHERE item_id=?",
                    args: vec![text(id)],
                },
            )
            .await
            .unwrap();
        assert_eq!(rows.rows, vec![vec![text(kind), text(&thread)]]);
    }
    let m::ItemRef::Reference(reference_id) = first_reference.clone() else {
        panic!("reference");
    };
    f.call(947, m::WorkspaceOperation::RemoveReference { reference_id })
        .await;
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
        matches!(inbox.result,m::WorkspaceResult::Done(m::WorkspaceAnswer::Inbox{entries,..}) if entries.iter().any(|row|row.item==m::ItemView::Removed{item:first_reference.clone()}&&row.change.event==m::ChangeEvent::Removed))
    );
    f.host.close().await;
}
