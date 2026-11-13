use crate::statement_plans::{RulePlan, WriteContext, exact, text};
use central_storage::{AlarmUpdate, SafeInteger, SqlValue, StatementBatch};
use workspace_model::{
    Change, ChangePosition, InboxEntry, ItemRef, ItemView, MembershipState, NotificationSetting,
    ReadMark, ReadMarkId, ReadScope, RefusalReason, RuleAnswer, Thread, ThreadMember, WorkLogEntry,
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
            "INSERT INTO read_marks (read_mark_id,actor_identity_id,scope,through_position,created_at,updated_at) VALUES (?,?,?,?,?,?) ON CONFLICT(actor_identity_id,scope) DO UPDATE SET through_position=excluded.through_position,updated_at=excluded.updated_at WHERE read_marks.through_position < excluded.through_position",
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

pub struct PositionedItem<T> {
    pub position: ChangePosition,
    pub item: T,
}
#[must_use]
pub fn ordered_page<T: Clone>(
    rows: &[PositionedItem<T>],
    after: ChangePosition,
    limit: u16,
) -> Vec<PositionedItem<T>> {
    let mut selected: Vec<_> = rows.iter().filter(|row| row.position > after).collect();
    selected.sort_by_key(|row| row.position);
    selected
        .into_iter()
        .take(usize::from(limit))
        .map(|row| PositionedItem {
            position: row.position,
            item: row.item.clone(),
        })
        .collect()
}
#[must_use]
pub fn work_log_page(
    rows: &[PositionedItem<WorkLogEntry>],
    project: &workspace_model::ProjectId,
    after: ChangePosition,
    limit: u16,
) -> Vec<PositionedItem<WorkLogEntry>> {
    let selected: Vec<_> = rows
        .iter()
        .filter(|row| row.item.project_id == *project)
        .map(|row| PositionedItem {
            position: row.position,
            item: row.item.clone(),
        })
        .collect();
    ordered_page(&selected, after, limit)
}

pub struct InboxReads<'a> {
    pub threads: &'a [Thread],
    pub memberships: &'a [ThreadMember],
    pub changes: &'a [ThreadChange],
    pub current_items: &'a [ItemView],
}
pub struct ThreadChange {
    pub thread_id: workspace_model::ThreadId,
    pub change: Change,
}
#[must_use]
pub fn inbox_page(
    context: &WriteContext,
    reads: InboxReads<'_>,
    after: ChangePosition,
    limit: u16,
) -> Vec<InboxEntry> {
    let followed = |thread_id: &workspace_model::ThreadId| {
        reads
            .threads
            .iter()
            .any(|row| row.thread_id == *thread_id && row.creator == context.actor)
            || reads.memberships.iter().any(|row| {
                row.thread_id == *thread_id
                    && row.actor_identity_id == context.actor
                    && row.state == MembershipState::Joined
                    && row.notifications == NotificationSetting::On
            })
    };
    let mut changes: Vec<_> = reads
        .changes
        .iter()
        .filter(|row| row.change.position > after && followed(&row.thread_id))
        .collect();
    changes.sort_by_key(|row| row.change.position);
    changes
        .into_iter()
        .filter_map(|row| {
            reads
                .current_items
                .iter()
                .find(|item| item.item_ref() == row.change.item)
                .map(|item| InboxEntry {
                    position: row.change.position,
                    change: row.change.clone(),
                    item: item.clone(),
                })
        })
        .take(usize::from(limit))
        .collect()
}

#[must_use]
pub fn show_item<'a>(item: &ItemRef, rows: &'a [ItemView]) -> Option<&'a ItemView> {
    rows.iter().find(|row| row.item_ref() == *item)
}

#[cfg(test)]
#[path = "read_rules_test.rs"]
mod tests;
