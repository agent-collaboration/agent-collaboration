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
pub fn references_to(
    target: &ItemRef,
    rows: &[PositionedItem<workspace_model::Reference>],
    after: ChangePosition,
    limit: u16,
) -> Vec<PositionedItem<workspace_model::Reference>> {
    let selected: Vec<_> = rows
        .iter()
        .filter(|row| row.item.target.as_item_ref() == target)
        .map(|row| PositionedItem {
            position: row.position,
            item: row.item.clone(),
        })
        .collect();
    ordered_page(&selected, after, limit)
}
#[must_use]
pub fn references_from(
    source: &ItemRef,
    rows: &[PositionedItem<workspace_model::Reference>],
    after: ChangePosition,
    limit: u16,
) -> Vec<PositionedItem<workspace_model::Reference>> {
    let selected: Vec<_> = rows
        .iter()
        .filter(|row| ItemRef::from(row.item.source.clone()) == *source)
        .map(|row| PositionedItem {
            position: row.position,
            item: row.item.clone(),
        })
        .collect();
    ordered_page(&selected, after, limit)
}
#[must_use]
pub fn links_from(
    source: &ItemRef,
    rows: &[PositionedItem<workspace_model::ExternalLink>],
    after: ChangePosition,
    limit: u16,
) -> Vec<PositionedItem<workspace_model::ExternalLink>> {
    let selected: Vec<_> = rows
        .iter()
        .filter(|row| ItemRef::from(row.item.source.clone()) == *source)
        .map(|row| PositionedItem {
            position: row.position,
            item: row.item.clone(),
        })
        .collect();
    ordered_page(&selected, after, limit)
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
        .map(|row| InboxEntry {
            position: row.change.position,
            change: row.change.clone(),
            item: reads
                .current_items
                .iter()
                .find(|item| item.item_ref() == row.change.item)
                .cloned()
                .unwrap_or_else(|| ItemView::Removed {
                    item: row.change.item.clone(),
                }),
        })
        .take(usize::from(limit))
        .collect()
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
pub fn list_deliverables(
    project: &workspace_model::ProjectId,
    reads: &DeliverableReads<'_>,
    after: ChangePosition,
    limit: u16,
) -> Result<Vec<PositionedItem<ItemView>>, RefusalReason> {
    let rows = reads
        .deliverables
        .iter()
        .filter(|row| row.item.project_id == *project)
        .map(|row| {
            Ok(PositionedItem {
                position: row.position,
                item: show_deliverable(&row.item.deliverable_id, reads)?,
            })
        })
        .collect::<Result<Vec<_>, RefusalReason>>()?;
    Ok(ordered_page(&rows, after, limit))
}

pub struct PostSearchReads<'a> {
    pub posts: &'a [PositionedItem<workspace_model::Post>],
    pub threads: &'a [Thread],
}
pub struct PostSearchPlan {
    pub statement: central_storage::Statement,
    pub items: Vec<PositionedItem<workspace_model::Post>>,
}
/// Ported literal, ASCII-case-insensitive matching; escaped LIKE preserves % and _.
pub fn search_posts(
    area: Option<&workspace_model::AreaId>,
    thread: Option<&workspace_model::ThreadId>,
    query: &workspace_model::SearchQuery,
    reads: PostSearchReads<'_>,
    after: ChangePosition,
    limit: u16,
) -> Result<PostSearchPlan, RefusalReason> {
    let pattern = format!(
        "%{}%",
        query
            .as_str()
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let position =
        SafeInteger::new(
            i64::try_from(after.get()).map_err(|_| RefusalReason::InvalidInput {
                field: "afterPosition".to_owned(),
            })?,
        )
        .map_err(|_| RefusalReason::InvalidInput {
            field: "afterPosition".to_owned(),
        })?;
    let page_limit =
        SafeInteger::new(i64::from(limit)).map_err(|_| RefusalReason::InvalidInput {
            field: "limit".to_owned(),
        })?;
    let statement = central_storage::Statement {
        sql: "SELECT posts.post_id,posts.thread_id,posts.author,posts.body,posts.addressees,changes.position FROM posts JOIN threads ON threads.thread_id=posts.thread_id JOIN changes ON changes.item_kind='post' AND changes.item_id=posts.post_id AND changes.change_kind='posted' WHERE (? IS NULL OR threads.area_id=?) AND (? IS NULL OR posts.thread_id=?) AND posts.body LIKE ? ESCAPE '\\' AND changes.position>? ORDER BY changes.position LIMIT ?",
        args: vec![
            area.map_or(SqlValue::Null, text),
            area.map_or(SqlValue::Null, text),
            thread.map_or(SqlValue::Null, text),
            thread.map_or(SqlValue::Null, text),
            text(pattern),
            SqlValue::Integer(position),
            SqlValue::Integer(page_limit),
        ],
    };
    let needle = query.as_str().to_ascii_lowercase();
    let matching: Vec<_> = reads
        .posts
        .iter()
        .filter(|row| {
            thread.is_none_or(|id| row.item.thread_id == *id)
                && reads.threads.iter().any(|parent| {
                    parent.thread_id == row.item.thread_id
                        && area.is_none_or(|id| parent.area_id == *id)
                })
                && row
                    .item
                    .body
                    .as_str()
                    .to_ascii_lowercase()
                    .contains(&needle)
        })
        .map(|row| PositionedItem {
            position: row.position,
            item: row.item.clone(),
        })
        .collect();
    Ok(PostSearchPlan {
        statement,
        items: ordered_page(&matching, after, limit),
    })
}

#[cfg(test)]
#[path = "read_rules_test.rs"]
mod tests;
