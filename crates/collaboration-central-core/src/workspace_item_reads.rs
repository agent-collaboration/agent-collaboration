use crate::workspace_decode_error::corrupt;
use crate::workspace_row_codec::WorkspaceRows;
use central_storage::StoreFailure;
use workspace_model as m;
use workspace_rules::{DeliverableReads, PositionedItem};

pub(crate) fn contains_item(rows: &WorkspaceRows, item: &m::ItemRef) -> bool {
    match item {
        m::ItemRef::Area(id) => rows.areas.iter().any(|row| row.area_id == *id),
        m::ItemRef::Project(id) => rows.projects.iter().any(|row| row.project_id == *id),
        m::ItemRef::Initiative(id) => rows.initiatives.iter().any(|row| row.initiative_id == *id),
        m::ItemRef::Deliverable(id) => rows
            .deliverables
            .iter()
            .any(|row| row.deliverable_id == *id),
        m::ItemRef::Issue(id) => rows.issues.iter().any(|row| row.issue_id == *id),
        m::ItemRef::Thread(id) => rows.threads.iter().any(|row| row.thread_id == *id),
        m::ItemRef::Post(id) => rows.posts.iter().any(|row| row.post_id == *id),
        m::ItemRef::WorkLogEntry(id) => rows.entries.iter().any(|row| row.entry_id == *id),
        m::ItemRef::BacklogItem(id) => rows.backlog.iter().any(|row| row.item_id == *id),
        m::ItemRef::Reference(id) => rows.references.iter().any(|row| row.reference_id == *id),
        m::ItemRef::Link(id) => rows.links.iter().any(|row| row.link_id == *id),
    }
}
fn current_items(rows: &WorkspaceRows) -> Vec<m::ItemView> {
    rows.areas
        .iter()
        .cloned()
        .map(m::ItemView::Area)
        .chain(rows.projects.iter().cloned().map(m::ItemView::Project))
        .chain(
            rows.initiatives
                .iter()
                .cloned()
                .map(m::ItemView::Initiative),
        )
        .chain(
            rows.deliverables
                .iter()
                .map(|row| m::ItemView::Deliverable {
                    view: workspace_rules::deliverable_view(row, &rows.issues),
                    blocked_by: workspace_rules::blocked_by(
                        &row.deliverable_id,
                        &rows.dependencies,
                    ),
                }),
        )
        .chain(rows.issues.iter().cloned().map(m::ItemView::Issue))
        .chain(rows.threads.iter().cloned().map(m::ItemView::Thread))
        .chain(rows.posts.iter().cloned().map(m::ItemView::Post))
        .chain(rows.entries.iter().cloned().map(m::ItemView::WorkLogEntry))
        .chain(rows.backlog.iter().cloned().map(m::ItemView::BacklogItem))
        .chain(rows.references.iter().cloned().map(m::ItemView::Reference))
        .chain(rows.links.iter().cloned().map(m::ItemView::Link))
        .collect()
}
pub(crate) fn read_answer(
    context: &workspace_rules::WriteContext,
    operation: &m::WorkspaceOperation,
    rows: &WorkspaceRows,
) -> Result<m::WorkspaceResult, StoreFailure> {
    let items = current_items(rows);
    let result = match operation {
        m::WorkspaceOperation::Show { item } => {
            let deliverables = rows
                .deliverables
                .iter()
                .map(|row| {
                    Ok(PositionedItem {
                        position: rows.created_position(&m::ItemRef::Deliverable(
                            row.deliverable_id.clone(),
                        ))?,
                        item: row.clone(),
                    })
                })
                .collect::<Result<Vec<_>, StoreFailure>>()?;
            workspace_rules::show_item(
                item,
                &items,
                &DeliverableReads {
                    deliverables: &deliverables,
                    issues: &rows.issues,
                    dependencies: &rows.dependencies,
                },
            )
            .map(|item| m::WorkspaceAnswer::Item { item })
        }
        m::WorkspaceOperation::List {
            query,
            after,
            limit,
        } => {
            let matches = |item: &m::ItemView| match (query, item) {
                (m::ListQuery::Areas, m::ItemView::Area(_)) => true,
                (m::ListQuery::Projects { area_id }, m::ItemView::Project(row)) => {
                    row.area_id == *area_id
                }
                (m::ListQuery::Threads { area_id }, m::ItemView::Thread(row)) => {
                    row.area_id == *area_id
                }
                (m::ListQuery::Posts { thread_id }, m::ItemView::Post(row)) => {
                    row.thread_id == *thread_id
                }
                (
                    m::ListQuery::Deliverables { project_id },
                    m::ItemView::Deliverable { view, .. },
                ) => view.deliverable.project_id == *project_id,
                (m::ListQuery::Issues { deliverable_id }, m::ItemView::Issue(row)) => {
                    row.deliverable_id == *deliverable_id
                }
                (m::ListQuery::WorkLog { project_id }, m::ItemView::WorkLogEntry(row)) => {
                    row.project_id == *project_id
                }
                (m::ListQuery::Backlog { area_id }, m::ItemView::BacklogItem(row)) => {
                    row.area_id == *area_id
                }
                (m::ListQuery::ReferencesTo { item }, m::ItemView::Reference(row)) => {
                    row.target.as_item_ref() == item
                }
                (m::ListQuery::ReferencesFrom { item }, m::ItemView::Reference(row)) => {
                    m::ItemRef::from(row.source.clone()) == *item
                }
                (m::ListQuery::Links { item }, m::ItemView::Link(row)) => {
                    m::ItemRef::from(row.source.clone()) == *item
                }
                (
                    m::ListQuery::SearchPosts {
                        area_id,
                        thread_id,
                        text,
                    },
                    m::ItemView::Post(row),
                ) => {
                    thread_id.as_ref().is_none_or(|id| row.thread_id == *id)
                        && rows.threads.iter().any(|parent| {
                            parent.thread_id == row.thread_id
                                && area_id.as_ref().is_none_or(|id| parent.area_id == *id)
                        })
                        && row
                            .body
                            .as_str()
                            .to_ascii_lowercase()
                            .contains(&text.as_str().to_ascii_lowercase())
                }
                _ => false,
            };
            let positioned = items
                .into_iter()
                .filter(matches)
                .map(|item| {
                    Ok(PositionedItem {
                        position: rows.created_position(&item.item_ref())?,
                        item,
                    })
                })
                .collect::<Result<Vec<_>, StoreFailure>>()?;
            let output =
                workspace_rules::ordered_page(&positioned, *after, limit.saturating_add(1));
            let next = if output.len() > usize::from(*limit) {
                output
                    .get(usize::from(*limit).saturating_sub(1))
                    .map(|row| row.position)
            } else {
                None
            };
            Ok(m::WorkspaceAnswer::Page {
                items: output
                    .into_iter()
                    .take(usize::from(*limit))
                    .map(|row| row.item)
                    .collect(),
                next_after_position: next,
            })
        }
        m::WorkspaceOperation::Inbox { after, limit } => {
            let mark = rows
                .marks
                .iter()
                .find(|row| {
                    row.actor_identity_id == context.actor && row.scope == m::ReadScope::Inbox
                })
                .map_or(m::ChangePosition::ZERO, |row| row.through_position);
            let changes = rows
                .changes
                .iter()
                .filter_map(|row| {
                    row.thread_id.as_ref().map(|thread| {
                        row.change().map(|change| workspace_rules::ThreadChange {
                            thread_id: thread.clone(),
                            change,
                        })
                    })
                })
                .collect::<Result<Vec<_>, StoreFailure>>()?;
            let entries = workspace_rules::inbox_page(
                context,
                workspace_rules::InboxReads {
                    threads: &rows.threads,
                    memberships: &rows.members,
                    changes: &changes,
                    current_items: &items,
                },
                after.unwrap_or(mark),
                limit.saturating_add(1),
            );
            let next = if entries.len() > usize::from(*limit) {
                entries
                    .get(usize::from(*limit).saturating_sub(1))
                    .map(|row| row.position)
            } else {
                None
            };
            Ok(m::WorkspaceAnswer::Inbox {
                entries: entries.into_iter().take(usize::from(*limit)).collect(),
                next_after_position: next,
                read_through: mark,
            })
        }
        _ => return Err(corrupt("read operation")),
    };
    Ok(match result {
        Ok(answer) => m::WorkspaceResult::Done(answer),
        Err(m::RefusalReason::NotFound { item }) => m::WorkspaceResult::NotFound {
            workspace_id: context.workspace_id.clone(),
            item,
        },
        Err(refusal) => m::WorkspaceResult::Refused(refusal),
    })
}
pub(crate) fn actor_ids(result: &m::WorkspaceResult) -> Vec<m::ActorIdentityId> {
    fn add(ids: &mut Vec<m::ActorIdentityId>, actor: &m::ActorIdentityId) {
        if !ids.contains(actor) {
            ids.push(actor.clone());
        }
    }
    fn item(ids: &mut Vec<m::ActorIdentityId>, view: &m::ItemView) {
        match view {
            m::ItemView::Project(row) => add(ids, &row.lead),
            m::ItemView::Thread(row) => add(ids, &row.creator),
            m::ItemView::Post(row) => {
                add(ids, &row.author);
                for addressee in &row.addressees {
                    if let m::Addressee::Actor { actor_identity_id } = addressee {
                        add(ids, actor_identity_id);
                    }
                }
            }
            m::ItemView::WorkLogEntry(row) => add(ids, &row.author),
            m::ItemView::BacklogItem(row) => add(ids, &row.creator),
            m::ItemView::Area(_)
            | m::ItemView::Initiative(_)
            | m::ItemView::Deliverable { .. }
            | m::ItemView::Issue(_)
            | m::ItemView::Reference(_)
            | m::ItemView::Link(_)
            | m::ItemView::Removed { .. } => {}
        }
    }
    let mut ids = Vec::new();
    match result {
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Item { item: view }) => item(&mut ids, view),
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Page { items, .. }) => {
            for view in items {
                item(&mut ids, view);
            }
        }
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Inbox { entries, .. }) => {
            for entry in entries {
                add(&mut ids, &entry.change.actor);
                item(&mut ids, &entry.item);
                match &entry.change.event {
                    m::ChangeEvent::Joined { member }
                    | m::ChangeEvent::Left { member }
                    | m::ChangeEvent::NotificationsOff { member }
                    | m::ChangeEvent::NotificationsOn { member } => add(&mut ids, member),
                    m::ChangeEvent::ProjectRoleGranted { holder, .. }
                    | m::ChangeEvent::ProjectRoleEnded { holder, .. }
                    | m::ChangeEvent::DeliverableRoleGranted { holder, .. }
                    | m::ChangeEvent::DeliverableRoleEnded { holder, .. } => add(&mut ids, holder),
                    m::ChangeEvent::Created
                    | m::ChangeEvent::Updated
                    | m::ChangeEvent::Removed
                    | m::ChangeEvent::Posted
                    | m::ChangeEvent::Resolved
                    | m::ChangeEvent::Redacted
                    | m::ChangeEvent::StateChanged => {}
                }
            }
        }
        m::WorkspaceResult::Refused(m::RefusalReason::NotCreator { creator }) => {
            add(&mut ids, creator)
        }
        m::WorkspaceResult::Refused(m::RefusalReason::NotLead { lead, .. }) => add(&mut ids, lead),
        _ => {}
    }
    ids
}
