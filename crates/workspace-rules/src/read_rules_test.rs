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
