//! One workspace's inbox for one area or one project. Reading moves no read
//! mark; each item names the mark that acknowledges it.
use workspace_model::{
    ActorIdentityId, Area, Change, ChangeEvent, ChangePosition, InboxItem, InboxReadMode,
    InboxScope, ItemRef, ListOrder, MembershipState, NotificationSetting, PageLimit, Post, Project,
    ProjectId, ReadMark, ReadScope, RefusalReason, Thread, ThreadId, ThreadMember, ThreadState,
    WorkspaceAnswer,
};

/// A change with the thread or project its row was filed under.
pub struct ScopedChange {
    pub change: Change,
    pub thread_id: Option<ThreadId>,
    pub project_id: Option<ProjectId>,
}

pub struct InboxReads<'a> {
    pub reader: &'a ActorIdentityId,
    pub areas: &'a [Area],
    pub projects: &'a [Project],
    pub threads: &'a [Thread],
    pub memberships: &'a [ThreadMember],
    pub posts: &'a [Post],
    pub changes: &'a [ScopedChange],
    pub marks: &'a [ReadMark],
    /// The workspace's latest change position.
    pub high_water: ChangePosition,
}

#[must_use]
pub fn acknowledgement_scope(scope: &InboxScope) -> ReadScope {
    match scope {
        InboxScope::Area { area_id } => ReadScope::InboxArea {
            area_id: area_id.clone(),
        },
        InboxScope::Project { project_id } => ReadScope::InboxProject {
            project_id: project_id.clone(),
        },
    }
}

/// Unread: oldest first, strictly after the later of the reader's scope mark and
/// `after`. Latest: newest first, strictly before `after`, ignoring the mark.
pub fn inbox_answer(
    scope: &InboxScope,
    read_mode: InboxReadMode,
    after: Option<ChangePosition>,
    limit: PageLimit,
    reads: &InboxReads<'_>,
) -> Result<WorkspaceAnswer, RefusalReason> {
    let acknowledge = acknowledgement_scope(scope);
    let mark = reads
        .marks
        .iter()
        .find(|row| row.actor_identity_id == *reads.reader && row.scope == acknowledge)
        .map_or(ChangePosition::ZERO, |row| row.through_position);
    let (area_name, project_name) = scope_names(scope, reads)?;
    let mut items: Vec<InboxItem> = reads
        .changes
        .iter()
        .filter(|row| match read_mode {
            InboxReadMode::Unread => row.change.position > after.unwrap_or_default().max(mark),
            InboxReadMode::Latest => after.is_none_or(|before| row.change.position < before),
        })
        .filter_map(|row| inbox_item(scope, row, &acknowledge, &area_name, &project_name, reads))
        .collect();
    let order = match read_mode {
        InboxReadMode::Unread => {
            items.sort_by_key(InboxItem::position);
            ListOrder::OldestFirst
        }
        InboxReadMode::Latest => {
            items.sort_by_key(|item| std::cmp::Reverse(item.position()));
            ListOrder::NewestFirst
        }
    };
    let take = usize::from(limit.get());
    let next_after_position = (items.len() > take)
        .then(|| items.get(take.saturating_sub(1)).map(InboxItem::position))
        .flatten();
    items.truncate(take);
    Ok(WorkspaceAnswer::Inbox {
        scope: scope.clone(),
        read_mode,
        order,
        items,
        high_water: reads.high_water,
        unavailable_projects: Vec::new(),
        next_after_position,
    })
}

/// The area's name, and for a project scope the project's title.
fn scope_names(
    scope: &InboxScope,
    reads: &InboxReads<'_>,
) -> Result<(workspace_model::ResourceName, workspace_model::ResourceName), RefusalReason> {
    let area_name = |area_id: &workspace_model::AreaId| {
        reads
            .areas
            .iter()
            .find(|row| row.area_id == *area_id)
            .map(|row| row.name.clone())
            .ok_or_else(|| RefusalReason::NotFound {
                item: ItemRef::Area(area_id.clone()),
            })
    };
    match scope {
        InboxScope::Area { area_id } => {
            let name = area_name(area_id)?;
            Ok((name.clone(), name))
        }
        InboxScope::Project { project_id } => {
            let project = reads
                .projects
                .iter()
                .find(|row| row.project_id == *project_id)
                .ok_or_else(|| RefusalReason::NotFound {
                    item: ItemRef::Project(project_id.clone()),
                })?;
            Ok((area_name(&project.area_id)?, project.title.clone()))
        }
    }
}

/// A thread is followed by its creator and by members with notifications on.
fn followed(thread: &Thread, reads: &InboxReads<'_>) -> bool {
    thread.creator == *reads.reader
        || reads.memberships.iter().any(|row| {
            row.thread_id == thread.thread_id
                && row.actor_identity_id == *reads.reader
                && row.state == MembershipState::Joined
                && row.notifications == NotificationSetting::On
        })
}

fn inbox_item(
    scope: &InboxScope,
    row: &ScopedChange,
    acknowledge: &ReadScope,
    area_name: &workspace_model::ResourceName,
    project_name: &workspace_model::ResourceName,
    reads: &InboxReads<'_>,
) -> Option<InboxItem> {
    let position = row.change.position;
    match scope {
        InboxScope::Project { project_id } => {
            (row.project_id.as_ref() == Some(project_id)).then(|| InboxItem::ProjectChanged {
                position,
                acknowledge: acknowledge.clone(),
                change: row.change.clone(),
                project_name: project_name.clone(),
                area_name: area_name.clone(),
            })
        }
        InboxScope::Area { area_id } => {
            let thread = reads.threads.iter().find(|thread| {
                Some(&thread.thread_id) == row.thread_id.as_ref() && thread.area_id == *area_id
            })?;
            if !followed(thread, reads) {
                return None;
            }
            match (&row.change.item, &row.change.event) {
                (ItemRef::Post(post_id), ChangeEvent::Posted) => {
                    let post = reads.posts.iter().find(|post| post.post_id == *post_id)?;
                    Some(InboxItem::PostCreated {
                        position,
                        acknowledge: acknowledge.clone(),
                        post: post.clone(),
                        thread_title: thread.title.clone(),
                        area_name: area_name.clone(),
                        project_name: None,
                    })
                }
                (
                    ItemRef::Thread(thread_id),
                    event @ (ChangeEvent::Created | ChangeEvent::Resolved),
                ) => Some(InboxItem::ThreadStateChanged {
                    position,
                    acknowledge: acknowledge.clone(),
                    thread_id: thread_id.clone(),
                    actor: row.change.actor.clone(),
                    state: if matches!(event, ChangeEvent::Resolved) {
                        ThreadState::Resolved
                    } else {
                        ThreadState::Open
                    },
                    thread_title: thread.title.clone(),
                    area_name: area_name.clone(),
                    project_name: None,
                }),
                _ => None,
            }
        }
    }
}

#[cfg(test)]
#[path = "inbox_rules_test.rs"]
mod tests;
