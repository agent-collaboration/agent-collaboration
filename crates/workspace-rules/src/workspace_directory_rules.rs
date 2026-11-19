//! The organization's workspace directory: renames, archive, and the lists a
//! machine may read. Central owns the `workspaces` table; these decide.
use crate::list_rules::cursor_page;
use crate::read_rules::PositionedItem;
use crate::statement_plans::{exact, text};
use central_storage::GuardedStatement;
use workspace_model::{
    Description, ItemView, ListCursor, ListQuery, PageLimit, RefusalReason, ResourceName,
    Timestamp, Workspace, WorkspaceAnswer, WorkspaceId, WorkspaceState,
};

pub enum DirectoryChange {
    Update {
        name: ResourceName,
        description: Description,
    },
    Archive,
}

/// The guarded write and the workspace it leaves. `name_holder` is the
/// workspace that already holds the canonical name, if any.
pub fn change_workspace(
    now: Timestamp,
    current: &Workspace,
    change: &DirectoryChange,
    name_holder: Option<&WorkspaceId>,
) -> Result<(GuardedStatement, Workspace), RefusalReason> {
    if current.state != WorkspaceState::Active {
        return Err(RefusalReason::WorkspaceNotAllowed);
    }
    match change {
        DirectoryChange::Update { name, description } => {
            if name_holder.is_some_and(|holder| *holder != current.workspace_id) {
                return Err(RefusalReason::WorkspaceNameInUse { name: name.clone() });
            }
            Ok((
                exact(
                    "UPDATE workspaces SET name=?,description=?,updated_at=? WHERE workspace_id=? AND state='active'",
                    vec![
                        text(name.as_str()),
                        text(description.as_str()),
                        text(now.to_rfc3339()),
                        text(&current.workspace_id),
                    ],
                ),
                Workspace {
                    workspace_id: current.workspace_id.clone(),
                    name: name.clone(),
                    description: description.clone(),
                    state: WorkspaceState::Active,
                },
            ))
        }
        DirectoryChange::Archive => Ok((
            exact(
                "UPDATE workspaces SET state='archived',updated_at=? WHERE workspace_id=? AND state='active'",
                vec![text(now.to_rfc3339()), text(&current.workspace_id)],
            ),
            Workspace {
                state: WorkspaceState::Archived,
                ..current.clone()
            },
        )),
    }
}

/// Lists the workspaces this machine is allowed. A workspace's list position
/// is its ordinal among every organization workspace by UUIDv7 id, so a page
/// cursor stays stable as workspaces are added, archived or renamed.
pub fn workspace_list_answer(
    query: &ListQuery,
    cursor: ListCursor,
    limit: PageLimit,
    organization_workspaces: &[Workspace],
    allowed: &[WorkspaceId],
) -> Result<WorkspaceAnswer, RefusalReason> {
    let (text, include_archived) = match query {
        ListQuery::Workspaces { include_archived } => (None, *include_archived),
        ListQuery::SearchWorkspaces {
            text,
            include_archived,
        } => (Some(text), *include_archived),
        ListQuery::MessageStatus { .. }
        | ListQuery::Areas { .. }
        | ListQuery::Initiatives
        | ListQuery::Projects { .. }
        | ListQuery::Threads { .. }
        | ListQuery::ThreadMembers { .. }
        | ListQuery::Posts { .. }
        | ListQuery::Deliverables { .. }
        | ListQuery::Issues { .. }
        | ListQuery::ProjectIssues { .. }
        | ListQuery::ProjectRoles { .. }
        | ListQuery::DeliverableRoles { .. }
        | ListQuery::WorkLog { .. }
        | ListQuery::Backlog { .. }
        | ListQuery::ReferencesTo { .. }
        | ListQuery::ReferencesFrom { .. }
        | ListQuery::Links { .. }
        | ListQuery::SearchPosts { .. }
        | ListQuery::SearchItems { .. } => {
            return Err(RefusalReason::InvalidInput {
                field: "query".to_owned(),
            });
        }
    };
    let mut ordered: Vec<&Workspace> = organization_workspaces.iter().collect();
    ordered.sort_by(|left, right| left.workspace_id.cmp(&right.workspace_id));
    let selected = ordered
        .into_iter()
        .zip(1_u64..)
        .filter(|(workspace, _)| {
            allowed.contains(&workspace.workspace_id)
                && (include_archived || workspace.state == WorkspaceState::Active)
                && text.is_none_or(|text| {
                    crate::list_rules::contains_text(workspace.name.as_str(), text)
                        || crate::list_rules::contains_text(workspace.description.as_str(), text)
                })
        })
        .map(|(workspace, ordinal)| {
            Ok(PositionedItem {
                position: ordinal
                    .try_into()
                    .map_err(|_| RefusalReason::InvalidInput {
                        field: "position".to_owned(),
                    })?,
                item: ItemView::Workspace(workspace.clone()),
            })
        })
        .collect::<Result<Vec<_>, RefusalReason>>()?;
    let page = cursor_page(&selected, cursor, limit);
    Ok(WorkspaceAnswer::Page {
        items: page.items,
        next_cursor: page.next_cursor,
    })
}

#[cfg(test)]
#[path = "workspace_directory_rules_test.rs"]
mod tests;
