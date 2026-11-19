//! One workspace's lists: which current items a query selects, and the page a
//! cursor and limit cut from them. Reads move no read marks.
use crate::read_rules::PositionedItem;
use workspace_model::{
    ActorIdentityId, AreaState, AssignmentState, ItemRef, ItemView, ListCursor, ListOrder,
    ListQuery, MembershipState, PageLimit, PostScope, RefusalReason, SearchableKind, Thread,
    ThreadMember, WorkspaceAnswer,
};

/// Every current item of the workspace with the position of its creation.
pub struct ListReads<'a> {
    pub reader: &'a ActorIdentityId,
    pub items: &'a [PositionedItem<ItemView>],
    pub threads: &'a [Thread],
    pub memberships: &'a [ThreadMember],
    /// Deliverables that place issues in projects, whether or not they are listed.
    pub deliverables: &'a [workspace_model::Deliverable],
}

/// The selected items in the cursor's direction, at most `limit` of them, and
/// the cursor that continues after the last one when more remain.
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<ListCursor>,
}

#[must_use]
pub fn cursor_page<T: Clone>(
    rows: &[PositionedItem<T>],
    cursor: ListCursor,
    limit: PageLimit,
) -> Page<T> {
    let mut selected: Vec<_> = rows
        .iter()
        .filter(|row| match (cursor.order(), cursor.position()) {
            (_, None) => true,
            (ListOrder::OldestFirst, Some(position)) => row.position > position,
            (ListOrder::NewestFirst, Some(position)) => row.position < position,
        })
        .collect();
    match cursor.order() {
        ListOrder::OldestFirst => selected.sort_by_key(|row| row.position),
        ListOrder::NewestFirst => selected.sort_by_key(|row| std::cmp::Reverse(row.position)),
    }
    let take = usize::from(limit.get());
    let next_cursor = (selected.len() > take)
        .then(|| selected.get(take.saturating_sub(1)))
        .flatten()
        .map(|row| cursor.continue_past(row.position));
    Page {
        items: selected
            .into_iter()
            .take(take)
            .map(|row| row.item.clone())
            .collect(),
        next_cursor,
    }
}

/// Workspace-store lists only; the organization's workspace lists are central's.
pub fn list_answer(
    query: &ListQuery,
    cursor: ListCursor,
    limit: PageLimit,
    reads: &ListReads<'_>,
) -> Result<WorkspaceAnswer, RefusalReason> {
    if matches!(
        query,
        ListQuery::Workspaces { .. }
            | ListQuery::SearchWorkspaces { .. }
            | ListQuery::MessageStatus { .. }
    ) {
        return Err(RefusalReason::InvalidInput {
            field: "query".to_owned(),
        });
    }
    let selected: Vec<_> = reads
        .items
        .iter()
        .filter(|row| selects(query, &row.item, reads))
        .cloned()
        .collect();
    let page = cursor_page(&selected, cursor, limit);
    Ok(WorkspaceAnswer::Page {
        items: page.items,
        next_cursor: page.next_cursor,
    })
}

/// Literal, ASCII-case-insensitive containment, as the ported post search did.
pub(crate) fn contains_text(haystack: &str, needle: &workspace_model::SearchQuery) -> bool {
    haystack
        .to_ascii_lowercase()
        .contains(&needle.as_str().to_ascii_lowercase())
}

fn post_in_scope(
    thread_id: &workspace_model::ThreadId,
    scope: &PostScope,
    reads: &ListReads<'_>,
) -> bool {
    match scope {
        PostScope::Workspace => true,
        PostScope::Thread { thread_id: wanted } => thread_id == wanted,
        PostScope::Area { area_id } => reads
            .threads
            .iter()
            .any(|thread| thread.thread_id == *thread_id && thread.area_id == *area_id),
    }
}

fn deliverable_in_project(
    deliverable_id: &workspace_model::DeliverableId,
    project_id: &workspace_model::ProjectId,
    reads: &ListReads<'_>,
) -> bool {
    reads
        .deliverables
        .iter()
        .any(|row| row.deliverable_id == *deliverable_id && row.project_id == *project_id)
}

fn selects(query: &ListQuery, item: &ItemView, reads: &ListReads<'_>) -> bool {
    match query {
        ListQuery::Workspaces { .. }
        | ListQuery::SearchWorkspaces { .. }
        | ListQuery::MessageStatus { .. } => false,
        ListQuery::Areas { include_archived } => {
            matches!(item, ItemView::Area(row) if *include_archived || row.state == AreaState::Open)
        }
        ListQuery::Initiatives => matches!(item, ItemView::Initiative(_)),
        ListQuery::Projects { area_id } => {
            matches!(item, ItemView::Project(row) if row.area_id == *area_id)
        }
        ListQuery::Threads {
            area_id,
            followed_only,
        } => match item {
            ItemView::Thread { thread, .. } => {
                area_id.as_ref().is_none_or(|id| thread.area_id == *id)
                    && (!followed_only
                        || reads.memberships.iter().any(|member| {
                            member.thread_id == thread.thread_id
                                && member.actor_identity_id == *reads.reader
                                && member.state == MembershipState::Joined
                        }))
            }
            _ => false,
        },
        ListQuery::ThreadMembers {
            thread_id,
            include_left,
        } => matches!(item, ItemView::ThreadMember(row)
            if row.thread_id == *thread_id
                && (*include_left || row.state == MembershipState::Joined)),
        ListQuery::Posts { scope } => {
            matches!(item, ItemView::Post(row) if post_in_scope(&row.thread_id, scope, reads))
        }
        ListQuery::SearchPosts { scope, text } => matches!(item, ItemView::Post(row)
            if post_in_scope(&row.thread_id, scope, reads) && contains_text(row.body.as_str(), text)),
        ListQuery::Deliverables { project_id } => matches!(item, ItemView::Deliverable { view, .. }
            if view.deliverable.project_id == *project_id),
        ListQuery::Issues { deliverable_id } => {
            matches!(item, ItemView::Issue(row) if row.deliverable_id == *deliverable_id)
        }
        ListQuery::ProjectIssues { project_id } => matches!(item, ItemView::Issue(row)
            if deliverable_in_project(&row.deliverable_id, project_id, reads)),
        ListQuery::ProjectRoles {
            project_id,
            include_ended,
        } => matches!(item, ItemView::ProjectRole(row)
            if row.project_id == *project_id
                && (*include_ended || row.state == AssignmentState::Active)),
        ListQuery::DeliverableRoles {
            deliverable_id,
            include_ended,
        } => matches!(item, ItemView::DeliverableRole(row)
            if row.deliverable_id == *deliverable_id
                && (*include_ended || row.state == AssignmentState::Active)),
        ListQuery::WorkLog { project_id, tags } => matches!(item, ItemView::WorkLogEntry(row)
            if row.project_id == *project_id && tags.iter().all(|tag| row.tags.contains(tag))),
        ListQuery::Backlog { area_id } => {
            matches!(item, ItemView::BacklogItem(row) if row.area_id == *area_id)
        }
        ListQuery::ReferencesTo { item: target } => {
            matches!(item, ItemView::Reference(row) if row.target.as_item_ref() == target)
        }
        ListQuery::ReferencesFrom { item: source } => matches!(item, ItemView::Reference(row)
            if ItemRef::from(row.source.clone()) == *source),
        ListQuery::Links { item: source } => matches!(item, ItemView::Link(row)
            if ItemRef::from(row.source.clone()) == *source),
        ListQuery::SearchItems {
            text,
            item_kind,
            include_archived,
        } => match item {
            ItemView::Area(row) => {
                matches!(item_kind, SearchableKind::All | SearchableKind::Area)
                    && (*include_archived || row.state == AreaState::Open)
                    && contains_text(row.name.as_str(), text)
            }
            ItemView::Initiative(row) => {
                matches!(item_kind, SearchableKind::All | SearchableKind::Initiative)
                    && (contains_text(row.name.as_str(), text)
                        || contains_text(row.description.as_str(), text))
            }
            ItemView::Project(row) => {
                matches!(item_kind, SearchableKind::All | SearchableKind::Project)
                    && contains_text(row.title.as_str(), text)
            }
            _ => false,
        },
    }
}

#[cfg(test)]
#[path = "list_rules_test.rs"]
mod tests;
