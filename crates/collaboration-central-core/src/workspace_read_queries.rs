use crate::workspace_decode_error::corrupt;
use crate::workspace_row_codec::WorkspaceRows;
use central_storage::{SqlValue, Statement, StoreFailure};
use workspace_model::{ActorIdentityId, ChangePosition, ItemRef, WorkspaceOperation};
fn text(value: impl ToString) -> SqlValue {
    SqlValue::Text(value.to_string())
}
pub(crate) fn read_statement(
    workspace: &workspace_model::WorkspaceId,
    actor: &ActorIdentityId,
    operation: &WorkspaceOperation,
) -> Result<Statement, StoreFailure> {
    use WorkspaceOperation as Op;
    let (sql, first, second) = match operation {
        Op::CreateArea { .. } => (
            include_str!("workspace_sql/organization.sql"),
            SqlValue::Null,
            SqlValue::Null,
        ),
        Op::ArchiveArea { area_id }
        | Op::CreateProject { area_id, .. }
        | Op::CreateThread { area_id, .. }
        | Op::AddBacklogItem { area_id, .. } => (
            match operation {
                Op::CreateThread { .. } => include_str!("workspace_sql/threads.sql"),
                Op::AddBacklogItem { .. } => include_str!("workspace_sql/backlog.sql"),
                _ => include_str!("workspace_sql/organization.sql"),
            },
            text(area_id),
            SqlValue::Null,
        ),
        Op::CreateInitiative { .. } => (
            include_str!("workspace_sql/organization.sql"),
            SqlValue::Null,
            SqlValue::Null,
        ),
        Op::UpdateInitiative { initiative_id, .. } => (
            include_str!("workspace_sql/organization.sql"),
            text(initiative_id),
            SqlValue::Null,
        ),
        Op::CreateDeliverable { project_id, .. }
        | Op::GrantProjectRole { project_id, .. }
        | Op::ReleaseProjectRole { project_id, .. } => (
            if matches!(operation, Op::CreateDeliverable { .. }) {
                include_str!("workspace_sql/organization.sql")
            } else {
                include_str!("workspace_sql/roles.sql")
            },
            text(project_id),
            SqlValue::Null,
        ),
        Op::CreateIssue { deliverable_id, .. }
        | Op::GrantDeliverableRole { deliverable_id, .. }
        | Op::ReleaseDeliverableRole { deliverable_id, .. } => (
            if matches!(operation, Op::CreateIssue { .. }) {
                include_str!("workspace_sql/organization.sql")
            } else {
                include_str!("workspace_sql/roles.sql")
            },
            text(deliverable_id),
            SqlValue::Null,
        ),
        Op::JoinThread { thread_id, .. }
        | Op::AddThreadMember { thread_id, .. }
        | Op::LeaveThread { thread_id }
        | Op::SetNotifications { thread_id, .. }
        | Op::ThreadPost { thread_id, .. }
        | Op::ResolveThread { thread_id } => (
            include_str!("workspace_sql/threads.sql"),
            text(thread_id),
            SqlValue::Null,
        ),
        Op::AddDependency { blocking, blocked } => (
            include_str!("workspace_sql/dependencies.sql"),
            text(blocking),
            text(blocked),
        ),
        Op::RemoveDependency { dependency_id } => (
            include_str!("workspace_sql/dependencies.sql"),
            text(dependency_id),
            SqlValue::Null,
        ),
        Op::SetIssueStatus { issue_id, .. } => (
            include_str!("workspace_sql/statuses.sql"),
            text(issue_id),
            SqlValue::Null,
        ),
        Op::SetProjectStatus { project_id, .. } => (
            include_str!("workspace_sql/statuses.sql"),
            text(project_id),
            SqlValue::Null,
        ),
        Op::SetInitiativeStatus { initiative_id, .. } => (
            include_str!("workspace_sql/statuses.sql"),
            text(initiative_id),
            SqlValue::Null,
        ),
        Op::CloseBacklogItem { item_id, link, .. } => (
            include_str!("workspace_sql/relations.sql"),
            text(item_id),
            link.as_ref()
                .map_or(SqlValue::Null, |target| text(target.kind_and_id().1)),
        ),
        Op::UpdateBacklogItem { item_id, .. } | Op::RemoveBacklogItem { item_id } => (
            include_str!("workspace_sql/backlog.sql"),
            text(item_id),
            SqlValue::Null,
        ),
        Op::AddReference { source, target, .. } => (
            include_str!("workspace_sql/relations.sql"),
            text(ItemRef::from(source.clone()).kind_and_id().1),
            text(target.as_item_ref().kind_and_id().1),
        ),
        Op::AddLink { source, .. } => (
            include_str!("workspace_sql/relations.sql"),
            text(ItemRef::from(source.clone()).kind_and_id().1),
            SqlValue::Null,
        ),
        Op::RemoveReference { reference_id } => (
            include_str!("workspace_sql/relations.sql"),
            text(reference_id),
            SqlValue::Null,
        ),
        Op::RemoveLink { link_id } => (
            include_str!("workspace_sql/relations.sql"),
            text(link_id),
            SqlValue::Null,
        ),
        Op::WorkLogAdd { project_id, .. } => (
            include_str!("workspace_sql/work_log.sql"),
            text(project_id),
            SqlValue::Null,
        ),
        Op::WorkLogRedact { entry_id } => (
            include_str!("workspace_sql/work_log.sql"),
            text(entry_id),
            SqlValue::Null,
        ),
        Op::MarkRead { .. } => (
            include_str!("workspace_sql/read_marks.sql"),
            SqlValue::Null,
            SqlValue::Null,
        ),
        Op::Show { .. } | Op::List { .. } | Op::Inbox { .. } | Op::CreateWorkspace { .. } => (
            include_str!("workspace_sql/views.sql"),
            SqlValue::Null,
            SqlValue::Null,
        ),
    };
    let second = if let Op::ThreadPost { addressees, .. } = operation {
        text(serde_json::to_string(addressees).map_err(|_| corrupt("addressees"))?)
    } else {
        second
    };
    let first = if matches!(
        operation,
        Op::Show { .. } | Op::List { .. } | Op::Inbox { .. }
    ) {
        text(serde_json::to_string(operation).map_err(|_| corrupt("read operation"))?)
    } else {
        first
    };
    Ok(Statement {
        sql,
        args: vec![text(workspace), first, second, text(actor)],
    })
}
impl WorkspaceRows {
    pub fn created_position(&self, item: &ItemRef) -> Result<ChangePosition, StoreFailure> {
        self.created_positions
            .get(item)
            .copied()
            .ok_or_else(|| corrupt("created position"))
    }
}
