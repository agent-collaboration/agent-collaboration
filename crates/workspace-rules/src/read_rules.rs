use crate::statement_plans::{RulePlan, WriteContext, exact, text};
use central_storage::{AlarmUpdate, SafeInteger, SqlValue, StatementBatch};
use workspace_model::{
    ActorIdentityId, ChangePosition, ItemRef, ItemView, MembershipState, ReadMark, ReadMarkId,
    ReadScope, RefusalReason, RuleAnswer, Thread, ThreadMember, ThreadReaderView,
};

/// Mark advancement ports the existing maximum rule, without writer leases/epochs.
pub fn mark_read(
    context: &WriteContext,
    read_mark_id: &ReadMarkId,
    scope: ReadScope,
    through: ChangePosition,
    current: Option<&ReadMark>,
) -> Result<RulePlan, RefusalReason> {
    let previous = current
        .filter(|row| row.actor_identity_id == context.actor && row.scope == scope)
        .map_or(ChangePosition::ZERO, |row| row.through_position);
    let through = through.max(previous);
    let scope_json =
        workspace_model::encode_json(&scope).map_err(|error| RefusalReason::InvalidInput {
            field: error.field.to_owned(),
        })?;
    let integer = SafeInteger::new(i64::try_from(through.get()).map_err(|_| {
        RefusalReason::InvalidInput {
            field: "throughPosition".to_owned(),
        }
    })?)
    .map_err(|_| RefusalReason::InvalidInput {
        field: "throughPosition".to_owned(),
    })?;
    let statements = if through == previous {
        vec![]
    } else {
        vec![exact(
            "INSERT INTO workspace_read_marks (read_mark_id,actor_identity_id,scope,through_position,created_at,updated_at) VALUES (?,?,?,?,?,?) ON CONFLICT(actor_identity_id,scope) DO UPDATE SET through_position=excluded.through_position,updated_at=excluded.updated_at WHERE workspace_read_marks.through_position < excluded.through_position",
            vec![
                text(read_mark_id),
                text(&context.actor),
                text(scope_json),
                SqlValue::Integer(integer),
                text(context.now.to_rfc3339()),
                text(context.now.to_rfc3339()),
            ],
        )]
    };
    Ok(RulePlan {
        batch: StatementBatch {
            statements,
            alarm: AlarmUpdate::Unchanged,
        },
        answer: RuleAnswer::Marked { scope, through },
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct PositionedItem<T> {
    pub position: ChangePosition,
    pub item: T,
}
pub fn show_item(
    item: &ItemRef,
    rows: &[ItemView],
    deliverables: &DeliverableReads<'_>,
) -> Result<ItemView, RefusalReason> {
    if let ItemRef::Deliverable(id) = item {
        return show_deliverable(id, deliverables);
    }
    rows.iter()
        .find(|row| row.item_ref() == *item)
        .cloned()
        .ok_or_else(|| RefusalReason::NotFound { item: item.clone() })
}

pub struct DeliverableReads<'a> {
    pub deliverables: &'a [PositionedItem<workspace_model::Deliverable>],
    pub issues: &'a [workspace_model::Issue],
    pub dependencies: &'a [workspace_model::DeliverableDependency],
}
fn show_deliverable(
    id: &workspace_model::DeliverableId,
    reads: &DeliverableReads<'_>,
) -> Result<ItemView, RefusalReason> {
    let row = reads
        .deliverables
        .iter()
        .find(|row| row.item.deliverable_id == *id)
        .ok_or_else(|| RefusalReason::NotFound {
            item: ItemRef::Deliverable(id.clone()),
        })?;
    Ok(ItemView::Deliverable {
        view: crate::status_rules::deliverable_view(&row.item, reads.issues),
        blocked_by: crate::dependency_rules::blocked_by(id, reads.dependencies),
    })
}
/// A thread as the reader sees it: their current membership and thread read
/// mark, when they are a member. Building it moves no mark.
#[must_use]
pub fn thread_view(
    thread: &Thread,
    reader: &ActorIdentityId,
    memberships: &[ThreadMember],
    marks: &[ReadMark],
) -> ItemView {
    let me = memberships
        .iter()
        .find(|row| {
            row.thread_id == thread.thread_id
                && row.actor_identity_id == *reader
                && row.state == MembershipState::Joined
        })
        .map(|member| ThreadReaderView {
            role: member.role,
            notifications: member.notifications,
            read_through: marks
                .iter()
                .find(|mark| {
                    mark.actor_identity_id == *reader
                        && mark.scope
                            == (ReadScope::Thread {
                                thread_id: thread.thread_id.clone(),
                            })
                })
                .map(|mark| mark.through_position),
        });
    ItemView::Thread {
        thread: thread.clone(),
        me,
    }
}

#[cfg(test)]
#[path = "read_rules_test.rs"]
mod tests;
