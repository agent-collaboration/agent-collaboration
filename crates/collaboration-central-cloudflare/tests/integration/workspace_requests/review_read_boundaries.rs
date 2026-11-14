use super::*;
use central_storage::{CorruptReason, RowExpectation, StoreFailure};
#[tokio::test]
async fn inbox_cursor_never_precedes_the_stored_mark_and_an_ahead_cursor_pages_normally() {
    let f = Fixture::new().await;
    let area = f.area(1100).await;
    let thread = f.thread(1101, area).await;
    for request in 1102..1108 {
        created(
            f.call(
                request,
                m::WorkspaceOperation::ThreadPost {
                    thread_id: thread.clone(),
                    body: "Post".to_owned().try_into().unwrap(),
                    addressees: vec![],
                },
            )
            .await,
        );
    }
    let mark = 5_u64.try_into().unwrap();
    f.call(
        1108,
        m::WorkspaceOperation::MarkRead {
            scope: m::ReadScope::Inbox,
            through: mark,
        },
    )
    .await;
    let stale = f
        .call(
            0,
            m::WorkspaceOperation::Inbox {
                after: Some(2_u64.try_into().unwrap()),
                limit: 2.try_into().unwrap(),
            },
        )
        .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Inbox {
        entries,
        next_after_position,
        read_through,
    }) = stale.result
    else {
        panic!("inbox");
    };
    assert_eq!(read_through, mark);
    assert_eq!(
        entries
            .iter()
            .map(|row| row.position.get())
            .collect::<Vec<_>>(),
        vec![6, 7]
    );
    assert_eq!(next_after_position.map(m::ChangePosition::get), Some(7));
    let ahead = f
        .call(
            0,
            m::WorkspaceOperation::Inbox {
                after: Some(7_u64.try_into().unwrap()),
                limit: 2.try_into().unwrap(),
            },
        )
        .await;
    assert!(
        matches!(ahead.result,m::WorkspaceResult::Done(m::WorkspaceAnswer::Inbox{entries,next_after_position:None,..}) if entries.iter().map(|row|row.position.get()).collect::<Vec<_>>()==vec![8])
    );
    f.host.close().await;
}
#[tokio::test]
async fn malformed_stored_json_is_field_named_corruption_on_views_and_rule_snapshots() {
    let f = Fixture::new().await;
    let area = f.area(1120).await;
    let thread = f.thread(1121, area.clone()).await;
    let post = created(
        f.call(
            1122,
            m::WorkspaceOperation::ThreadPost {
                thread_id: thread.clone(),
                body: "Post".to_owned().try_into().unwrap(),
                addressees: vec![],
            },
        )
        .await,
    );
    let m::ItemRef::Post(post_id) = &post else {
        panic!("post");
    };
    let reference = created(
        f.call(
            1123,
            m::WorkspaceOperation::AddReference {
                source: m::ReferenceSource::Post(post_id.clone()),
                target: m::ReferenceTarget::try_from(m::ItemRef::Area(area)).unwrap(),
                details: Some(serde_json::json!({"valid":true})),
            },
        )
        .await,
    );
    let m::ItemRef::Reference(reference_id) = &reference else {
        panic!("reference");
    };
    let link = created(
        f.call(
            1124,
            m::WorkspaceOperation::AddLink {
                source: m::ReferenceSource::Thread(thread.clone()),
                url: "literal".to_owned(),
                title: "Link".to_owned(),
                details: None,
            },
        )
        .await,
    );
    let m::ItemRef::Link(link_id) = &link else {
        panic!("link");
    };
    f.call(
        1125,
        m::WorkspaceOperation::MarkRead {
            scope: m::ReadScope::Inbox,
            through: m::ChangePosition::ZERO,
        },
    )
    .await;
    for item in [&post, &reference, &link] {
        assert!(matches!(
            f.call(0, m::WorkspaceOperation::Show { item: item.clone() })
                .await
                .result,
            m::WorkspaceResult::Done(m::WorkspaceAnswer::Item { .. })
        ));
    }
    let scenarios = [
        (
            "UPDATE posts SET addressees='[' WHERE post_id=?",
            post_id.to_string(),
            "UPDATE posts SET addressees='[]' WHERE post_id=?",
            "posts.addressees",
            vec![
                m::WorkspaceOperation::Show { item: post.clone() },
                m::WorkspaceOperation::List {
                    query: m::ListQuery::Posts {
                        thread_id: thread.clone(),
                    },
                    after: m::ChangePosition::ZERO,
                    limit: 10.try_into().unwrap(),
                },
                m::WorkspaceOperation::AddLink {
                    source: m::ReferenceSource::Post(post_id.clone()),
                    url: "literal".to_owned(),
                    title: "No write".to_owned(),
                    details: None,
                },
            ],
        ),
        (
            "UPDATE item_references SET details='{' WHERE reference_id=?",
            reference_id.to_string(),
            "UPDATE item_references SET details='null' WHERE reference_id=?",
            "item_references.details",
            vec![
                m::WorkspaceOperation::Show {
                    item: reference.clone(),
                },
                m::WorkspaceOperation::RemoveReference {
                    reference_id: reference_id.clone(),
                },
            ],
        ),
        (
            "UPDATE links SET details='{' WHERE link_id=?",
            link_id.to_string(),
            "UPDATE links SET details='null' WHERE link_id=?",
            "links.details",
            vec![
                m::WorkspaceOperation::Show { item: link.clone() },
                m::WorkspaceOperation::RemoveLink {
                    link_id: link_id.clone(),
                },
            ],
        ),
    ];
    for (sql, key, restore, field, operations) in scenarios {
        f.workspace_batch(sql, vec![text(&key)]).await;
        for operation in operations {
            let result = f
                .host
                .request(
                    &f.caller,
                    m::WorkspaceRequest {
                        request_id: operation.is_write().then(|| id(1130)),
                        workspace_id: Some(f.workspace.clone()),
                        operation,
                    },
                )
                .await;
            assert_eq!(
                result,
                Err(StoreFailure::Corrupt {
                    reason: CorruptReason::BadValue {
                        column: field.to_owned()
                    }
                })
            );
        }
        f.workspace_batch(restore, vec![text(key)]).await;
    }
    // Create a genuine mark so both read selectors encounter its raw scope.
    f.call(
        1131,
        m::WorkspaceOperation::MarkRead {
            scope: m::ReadScope::Inbox,
            through: 1_u64.try_into().unwrap(),
        },
    )
    .await;
    f.workspace_execute(
        "UPDATE read_marks SET scope='{'",
        vec![],
        RowExpectation::Exactly(1),
    )
    .await;
    for operation in [
        m::WorkspaceOperation::Inbox {
            after: None,
            limit: 10.try_into().unwrap(),
        },
        m::WorkspaceOperation::MarkRead {
            scope: m::ReadScope::Inbox,
            through: 2_u64.try_into().unwrap(),
        },
    ] {
        let result = f
            .host
            .request(
                &f.caller,
                m::WorkspaceRequest {
                    request_id: operation.is_write().then(|| id(1132)),
                    workspace_id: Some(f.workspace.clone()),
                    operation,
                },
            )
            .await;
        assert_eq!(
            result,
            Err(StoreFailure::Corrupt {
                reason: CorruptReason::BadValue {
                    column: "read_marks.scope".to_owned()
                }
            })
        );
    }
    f.workspace_execute(
        "UPDATE read_marks SET scope='{\"kind\":\"inbox\"}'",
        vec![],
        RowExpectation::Exactly(1),
    )
    .await;
    assert!(matches!(
        f.call(
            0,
            m::WorkspaceOperation::Inbox {
                after: None,
                limit: 10.try_into().unwrap()
            }
        )
        .await
        .result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Inbox { .. })
    ));
    f.host.close().await;
}
