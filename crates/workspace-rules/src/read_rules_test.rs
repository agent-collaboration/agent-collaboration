use super::{PositionedItem, mark_read, thread_view};
use crate::fixtures::{context, id, member, thread};
use workspace_model::{
    ChangePosition, ItemRef, ItemView, MemberRole, NotificationSetting, ReadMark, ReadScope,
    RuleAnswer,
};
#[test]
fn r30_b_read_marks_never_go_back() {
    let context = context();
    let scope = ReadScope::InboxArea { area_id: id(4) };
    let current = ReadMark {
        read_mark_id: id(10),
        actor_identity_id: id(2),
        scope: scope.clone(),
        through_position: ChangePosition::try_from(3).unwrap(),
    };
    let plan = mark_read(
        &context,
        &id(11),
        scope,
        ChangePosition::try_from(2).unwrap(),
        Some(&current),
    )
    .unwrap();
    assert!(matches!(plan.answer,RuleAnswer::Marked {through,..} if through.get() == 3));
    assert!(plan.batch.statements.is_empty());
}

#[test]
fn a_scope_mark_never_advances_another_scopes_mark() {
    let context = context();
    let other_area = ReadMark {
        read_mark_id: id(10),
        actor_identity_id: id(2),
        scope: ReadScope::InboxArea { area_id: id(40) },
        through_position: ChangePosition::try_from(9).unwrap(),
    };
    let plan = mark_read(
        &context,
        &id(11),
        ReadScope::InboxArea { area_id: id(4) },
        ChangePosition::try_from(2).unwrap(),
        Some(&other_area),
    )
    .unwrap();
    assert!(matches!(plan.answer, RuleAnswer::Marked { through, .. } if through.get() == 2));
    assert_eq!(plan.batch.statements.len(), 1);
}

#[test]
fn removing_a_link_on_a_post_files_the_change_under_the_posts_thread() {
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
    let thread_id: workspace_model::ThreadId = id(8);
    assert_eq!(
        change.args.get(4),
        Some(&central_storage::SqlValue::Text(thread_id.to_string()))
    );
}

#[test]
fn r33_d_show_projects_the_computed_deliverable_status() {
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
}

#[test]
fn a_thread_shows_the_readers_membership_and_thread_mark_only_to_members() {
    let thread = thread();
    let mut reader_member = member(id(99), MemberRole::Observer);
    reader_member.notifications = NotificationSetting::Off;
    let marks = [
        ReadMark {
            read_mark_id: id(10),
            actor_identity_id: id(99),
            scope: ReadScope::Thread { thread_id: id(8) },
            through_position: ChangePosition::try_from(7).unwrap(),
        },
        ReadMark {
            read_mark_id: id(11),
            actor_identity_id: id(99),
            scope: ReadScope::InboxArea { area_id: id(4) },
            through_position: ChangePosition::try_from(9).unwrap(),
        },
    ];

    let member_view = thread_view(&thread, &id(99), &[reader_member.clone()], &marks);
    let stranger_view = thread_view(&thread, &id(98), &[reader_member], &marks);

    let ItemView::Thread { me: Some(me), .. } = member_view else {
        panic!("a member sees their own standing");
    };
    assert_eq!(me.role, MemberRole::Observer);
    assert_eq!(me.notifications, NotificationSetting::Off);
    assert_eq!(me.read_through, Some(ChangePosition::try_from(7).unwrap()));
    assert!(matches!(stranger_view, ItemView::Thread { me: None, .. }));
}
