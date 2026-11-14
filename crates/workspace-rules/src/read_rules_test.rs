use super::{InboxReads, PositionedItem, ThreadChange, inbox_page, mark_read, ordered_page};
use crate::fixtures::{context, id, member, thread};
use workspace_model::{
    Change, ChangeEvent, ChangePosition, ItemRef, ItemView, MemberRole, ReadMark, ReadScope,
    RuleAnswer, ThreadState,
};
#[test]
fn r30_b_read_marks_never_go_back() {
    let context = context();
    let current = ReadMark {
        read_mark_id: id(10),
        actor_identity_id: id(2),
        scope: ReadScope::Inbox,
        through_position: ChangePosition::try_from(3).unwrap(),
    };
    let plan = mark_read(
        &context,
        &id(11),
        ReadScope::Inbox,
        ChangePosition::try_from(2).unwrap(),
        Some(&current),
    )
    .unwrap();
    assert!(matches!(plan.answer,RuleAnswer::Marked {through,..} if through.get() == 3));
    assert!(plan.batch.statements.is_empty());
}

#[test]
fn relation_pages_use_own_created_positions_not_source_or_id_order() {
    use workspace_model::{ExternalLink, Reference, ReferenceSource};
    let reference = |value| Reference {
        reference_id: id(value),
        source: ReferenceSource::Thread(id(8)),
        target: ItemRef::Project(id(5)).try_into().unwrap(),
        details: None,
    };
    let rows = [
        PositionedItem {
            position: ChangePosition::try_from(20).unwrap(),
            item: reference(100),
        },
        PositionedItem {
            position: ChangePosition::try_from(10).unwrap(),
            item: reference(200),
        },
    ];
    let first = super::references_to(&ItemRef::Project(id(5)), &rows, ChangePosition::ZERO, 1);
    assert_eq!(first.first().unwrap().item.reference_id, id(200));
    assert_eq!(first.first().unwrap().position.get(), 10);
    let second = super::references_to(
        &ItemRef::Project(id(5)),
        &rows,
        ChangePosition::try_from(10).unwrap(),
        1,
    );
    assert_eq!(second.first().unwrap().item.reference_id, id(100));
    assert_eq!(second.first().unwrap().position.get(), 20);
    assert!(
        super::references_to(
            &ItemRef::Project(id(5)),
            &rows,
            ChangePosition::try_from(20).unwrap(),
            1
        )
        .is_empty()
    );
    assert_eq!(
        super::references_from(&ItemRef::Thread(id(8)), &rows, ChangePosition::ZERO, 10)
            .iter()
            .map(|row| row.item.reference_id.clone())
            .collect::<Vec<_>>(),
        vec![id(200), id(100)]
    );
    let link = |value| ExternalLink {
        link_id: id(value),
        source: ReferenceSource::Thread(id(8)),
        url: "raw".to_owned(),
        title: "Link".to_owned(),
        details: None,
    };
    let rows = [
        PositionedItem {
            position: ChangePosition::try_from(20).unwrap(),
            item: link(100),
        },
        PositionedItem {
            position: ChangePosition::try_from(10).unwrap(),
            item: link(200),
        },
    ];
    let first = super::links_from(&ItemRef::Thread(id(8)), &rows, ChangePosition::ZERO, 1);
    assert_eq!(first.first().unwrap().item.link_id, id(200));
    let second = super::links_from(
        &ItemRef::Thread(id(8)),
        &rows,
        ChangePosition::try_from(10).unwrap(),
        1,
    );
    assert_eq!(second.first().unwrap().item.link_id, id(100));
    assert!(
        super::links_from(
            &ItemRef::Thread(id(8)),
            &rows,
            ChangePosition::try_from(20).unwrap(),
            1
        )
        .is_empty()
    );
}

#[test]
fn removed_link_on_a_followed_post_stays_in_the_inbox_without_a_snapshot() {
    let writer = context();
    let post = workspace_model::Post {
        post_id: id(30),
        thread_id: id(8),
        author: id(2),
        body: workspace_model::PostText::try_from("Post".to_owned()).unwrap(),
        addressees: vec![],
    };
    let link = workspace_model::ExternalLink {
        link_id: id(71),
        source: workspace_model::ReferenceSource::Post(id(30)),
        url: "raw".to_owned(),
        title: "Link".to_owned(),
        details: None,
    };
    let plan = crate::change_reference(
        &writer,
        &workspace_model::ReferenceCommand::RemoveLink { link_id: id(71) },
        crate::ReferenceReads {
            link: Some(&link),
            source_post: Some(&post),
            ..Default::default()
        },
    )
    .unwrap();
    let change = &plan.batch.statements.last().unwrap().statement;
    let Some(central_storage::SqlValue::Text(parent)) = change.args.get(4) else {
        panic!("removal must retain thread metadata");
    };
    let mut reader = context();
    reader.actor = id(99);
    let membership = member(id(99), MemberRole::Observer);
    let changes = [ThreadChange {
        thread_id: parent.parse().unwrap(),
        change: Change {
            change_id: id(3),
            position: ChangePosition::try_from(1).unwrap(),
            item: ItemRef::Link(id(71)),
            actor: id(2),
            event: ChangeEvent::Removed,
        },
    }];
    let inbox = inbox_page(
        &reader,
        InboxReads {
            threads: &[thread()],
            memberships: &[membership],
            changes: &changes,
            current_items: &[],
        },
        ChangePosition::ZERO,
        10,
    );
    assert_eq!(inbox.len(), 1);
    let entry = inbox.first().unwrap();
    assert_eq!(entry.change.actor, id(2));
    assert_eq!(entry.change.event, ChangeEvent::Removed);
    assert_eq!(
        entry.item,
        ItemView::Removed {
            item: ItemRef::Link(id(71))
        }
    );
}
#[test]
fn r8_order_comes_from_positions_not_input_or_ids() {
    let rows = [
        PositionedItem {
            position: ChangePosition::try_from(3).unwrap(),
            item: "third",
        },
        PositionedItem {
            position: ChangePosition::try_from(1).unwrap(),
            item: "first",
        },
        PositionedItem {
            position: ChangePosition::try_from(2).unwrap(),
            item: "redacted",
        },
    ];
    let output = ordered_page(&rows, ChangePosition::ZERO, 10);
    assert_eq!(
        output.into_iter().map(|row| row.item).collect::<Vec<_>>(),
        vec!["first", "redacted", "third"]
    );
}
#[test]
fn r30_a_c_follow_filter_and_resolution_uses_current_item() {
    let mut context = context();
    context.actor = id(99);
    let mut thread = thread();
    thread.state = ThreadState::Resolved;
    let member = member(id(99), MemberRole::Observer);
    let change = Change {
        change_id: id(10),
        position: ChangePosition::try_from(1).unwrap(),
        item: ItemRef::Thread(id(8)),
        actor: id(2),
        event: ChangeEvent::Resolved,
    };
    let other = ThreadChange {
        thread_id: id(999),
        change: Change {
            position: ChangePosition::try_from(2).unwrap(),
            ..change.clone()
        },
    };
    let changes = [
        other,
        ThreadChange {
            thread_id: id(8),
            change,
        },
    ];
    let items = [ItemView::Thread(thread.clone())];
    let output = inbox_page(
        &context,
        InboxReads {
            threads: &[thread],
            memberships: &[member],
            changes: &changes,
            current_items: &items,
        },
        ChangePosition::ZERO,
        10,
    );
    assert_eq!(output.len(), 1);
    assert!(
        matches!(output.first().map(|row|&row.item),Some(ItemView::Thread(thread)) if thread.state == ThreadState::Resolved)
    );
}

#[test]
fn r33_d_show_and_list_project_the_computed_deliverable_status() {
    let rows = [
        PositionedItem {
            position: ChangePosition::try_from(1).unwrap(),
            item: crate::fixtures::deliverable(10),
        },
        PositionedItem {
            position: ChangePosition::try_from(2).unwrap(),
            item: crate::fixtures::deliverable(20),
        },
        PositionedItem {
            position: ChangePosition::try_from(3).unwrap(),
            item: crate::fixtures::deliverable(30),
        },
    ];
    let issues = [
        crate::fixtures::issue(21, 20, workspace_model::WorkStatus::OnHold),
        crate::fixtures::issue(31, 30, workspace_model::WorkStatus::Closed),
    ];
    let reads = super::DeliverableReads {
        deliverables: &rows,
        issues: &issues,
        dependencies: &[],
    };
    for (value, expected) in [
        (10, workspace_model::DeliverableStatus::Open),
        (20, workspace_model::DeliverableStatus::Open),
        (30, workspace_model::DeliverableStatus::Closed),
    ] {
        assert!(
            matches!(super::show_item(&ItemRef::Deliverable(id(value)),&[],&reads).unwrap(),ItemView::Deliverable {view,..} if view.status == expected)
        );
    }
    let listed = super::list_deliverables(&id(5), &reads, ChangePosition::ZERO, 10).unwrap();
    let statuses: Vec<_> = listed
        .iter()
        .filter_map(|row| {
            if let ItemView::Deliverable { view, .. } = &row.item {
                Some(view.status)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        statuses,
        vec![
            workspace_model::DeliverableStatus::Open,
            workspace_model::DeliverableStatus::Open,
            workspace_model::DeliverableStatus::Closed
        ]
    );
}

#[test]
fn ported_search_keeps_literal_ascii_case_scope_and_position_order() {
    let mut other_thread = thread();
    other_thread.thread_id = id(40);
    other_thread.area_id = id(41);
    let post = |value, thread_id, body: &str| PositionedItem {
        position: ChangePosition::try_from(value).unwrap(),
        item: workspace_model::Post {
            post_id: id(u32::try_from(value).unwrap()),
            thread_id,
            author: id(2),
            body: workspace_model::PostText::try_from(body.to_owned()).unwrap(),
            addressees: vec![],
        },
    };
    let posts = [
        post(3, id(8), "NAVIGATION plain é"),
        post(2, id(8), "Navigation 100%_ É"),
        post(1, id(40), "Navigation outside"),
    ];
    let threads = [thread(), other_thread];
    let search = |text: &str, after| {
        super::search_posts(
            Some(&id(4)),
            Some(&id(8)),
            &workspace_model::SearchQuery::try_from(text.to_owned()).unwrap(),
            super::PostSearchReads {
                posts: &posts,
                threads: &threads,
            },
            after,
            10,
        )
        .unwrap()
    };
    let result = search(" navigation ", ChangePosition::ZERO);
    assert_eq!(
        result
            .items
            .iter()
            .map(|row| row.position.get())
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
    assert_eq!(
        search("%_", ChangePosition::ZERO)
            .items
            .first()
            .unwrap()
            .position
            .get(),
        2
    );
    assert!(
        search("%_", ChangePosition::ZERO)
            .statement
            .args
            .contains(&central_storage::SqlValue::Text("%\\%\\_%".to_owned()))
    );
    assert_eq!(
        search("É", ChangePosition::ZERO)
            .items
            .first()
            .unwrap()
            .position
            .get(),
        2
    );
    assert_eq!(
        search("é", ChangePosition::ZERO)
            .items
            .first()
            .unwrap()
            .position
            .get(),
        3
    );
    assert_eq!(
        search("navigation", ChangePosition::try_from(2).unwrap())
            .items
            .len(),
        1
    );
}
