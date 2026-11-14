use crate::statement_plans::{
    RulePlan, WriteContext, change_metadata, exact, not_found, plan, text, unchanged,
};
use workspace_model::{
    Blocker, BlockerStatus, Deliverable, DeliverableStatus, DeliverableView, Initiative, Issue,
    ItemRef, Project, RefusalReason, RuleAnswer, StatusCommand, WorkStatus,
};

#[must_use]
pub fn deliverable_status(deliverable: &Deliverable, issues: &[Issue]) -> DeliverableStatus {
    let mut children = issues
        .iter()
        .filter(|issue| issue.deliverable_id == deliverable.deliverable_id)
        .peekable();
    if children.peek().is_some() && children.all(|issue| issue.status == WorkStatus::Closed) {
        DeliverableStatus::Closed
    } else {
        DeliverableStatus::Open
    }
}
#[must_use]
pub fn deliverable_view(deliverable: &Deliverable, issues: &[Issue]) -> DeliverableView {
    DeliverableView {
        deliverable: deliverable.clone(),
        status: deliverable_status(deliverable, issues),
    }
}
/// Central supplies complete child collections and the initiative parents of the project.
#[derive(Default)]
pub struct StatusReads<'a> {
    pub issue: Option<&'a Issue>,
    pub project: Option<&'a Project>,
    pub initiative: Option<&'a Initiative>,
    pub deliverables: &'a [Deliverable],
    pub issues: &'a [Issue],
    pub initiative_projects: &'a [Project],
    pub project_initiatives: &'a [Initiative],
}
fn project_blocker(project: &Project) -> Blocker {
    Blocker {
        item: ItemRef::Project(project.project_id.clone()),
        title: project.title.as_str().to_owned(),
        status: BlockerStatus::Work(project.status),
    }
}
pub(crate) fn require_open_project(project: &Project) -> Result<(), RefusalReason> {
    if project.status == WorkStatus::Closed {
        return Err(RefusalReason::ParentClosed {
            parent: project_blocker(project),
        });
    }
    Ok(())
}
pub fn set_work_status(
    context: &WriteContext,
    command: &StatusCommand,
    reads: StatusReads<'_>,
) -> Result<RulePlan, RefusalReason> {
    let (item, original, status, sql) = match command {
        StatusCommand::SetIssue { issue_id, status } => {
            let issue = reads
                .issue
                .filter(|row| row.issue_id == *issue_id)
                .ok_or_else(|| not_found(ItemRef::Issue(issue_id.clone())))?;
            let deliverable = reads
                .deliverables
                .iter()
                .find(|row| row.deliverable_id == issue.deliverable_id)
                .ok_or_else(|| not_found(ItemRef::Deliverable(issue.deliverable_id.clone())))?;
            let project = reads
                .project
                .filter(|row| row.project_id == deliverable.project_id)
                .ok_or_else(|| not_found(ItemRef::Project(deliverable.project_id.clone())))?;
            if *status != WorkStatus::Closed && project.status == WorkStatus::Closed {
                return Err(RefusalReason::ParentClosed {
                    parent: project_blocker(project),
                });
            }
            (
                ItemRef::Issue(issue_id.clone()),
                issue.status,
                *status,
                "UPDATE issues SET status=?,updated_at=? WHERE issue_id=? AND status=?",
            )
        }
        StatusCommand::SetProject { project_id, status } => {
            let project = reads
                .project
                .filter(|row| row.project_id == *project_id)
                .ok_or_else(|| not_found(ItemRef::Project(project_id.clone())))?;
            if *status == WorkStatus::Closed {
                let mut blockers = Vec::new();
                for row in reads
                    .deliverables
                    .iter()
                    .filter(|row| row.project_id == *project_id)
                {
                    if deliverable_status(row, reads.issues) == DeliverableStatus::Closed {
                        continue;
                    }
                    blockers.push(Blocker {
                        item: ItemRef::Deliverable(row.deliverable_id.clone()),
                        title: row.title.as_str().to_owned(),
                        status: BlockerStatus::Deliverable(DeliverableStatus::Open),
                    });
                    blockers.extend(
                        reads
                            .issues
                            .iter()
                            .filter(|issue| {
                                issue.deliverable_id == row.deliverable_id
                                    && issue.status != WorkStatus::Closed
                            })
                            .map(|issue| Blocker {
                                item: ItemRef::Issue(issue.issue_id.clone()),
                                title: issue.title.as_str().to_owned(),
                                status: BlockerStatus::Work(issue.status),
                            }),
                    );
                }
                if !blockers.is_empty() {
                    return Err(RefusalReason::ChildrenNotClosed { blockers });
                }
            }
            if *status != WorkStatus::Closed
                && let Some(parent) = reads
                    .project_initiatives
                    .iter()
                    .find(|row| row.status == WorkStatus::Closed)
            {
                return Err(RefusalReason::ParentClosed {
                    parent: Blocker {
                        item: ItemRef::Initiative(parent.initiative_id.clone()),
                        title: parent.name.as_str().to_owned(),
                        status: BlockerStatus::Work(parent.status),
                    },
                });
            }
            (
                ItemRef::Project(project_id.clone()),
                project.status,
                *status,
                "UPDATE projects SET status=?,updated_at=? WHERE project_id=? AND status=?",
            )
        }
        StatusCommand::SetInitiative {
            initiative_id,
            status,
        } => {
            let initiative = reads
                .initiative
                .filter(|row| row.initiative_id == *initiative_id)
                .ok_or_else(|| not_found(ItemRef::Initiative(initiative_id.clone())))?;
            if *status == WorkStatus::Closed {
                let blockers: Vec<_> = reads
                    .initiative_projects
                    .iter()
                    .filter(|row| row.status != WorkStatus::Closed)
                    .map(project_blocker)
                    .collect();
                if !blockers.is_empty() {
                    return Err(RefusalReason::ChildrenNotClosed { blockers });
                }
            }
            (
                ItemRef::Initiative(initiative_id.clone()),
                initiative.status,
                *status,
                "UPDATE initiatives SET status=?,updated_at=? WHERE initiative_id=? AND status=?",
            )
        }
    };
    if original == status {
        return Ok(unchanged());
    }
    let statements = vec![exact(
        sql,
        vec![
            text(status_name(status)),
            text(context.now.to_rfc3339()),
            text(item.kind_and_id().1),
            text(status_name(original)),
        ],
    )];
    let project = match command {
        StatusCommand::SetIssue { .. } | StatusCommand::SetProject { .. } => {
            reads.project.map(|row| &row.project_id)
        }
        StatusCommand::SetInitiative { .. } => None,
    };
    Ok(change_metadata(
        plan(
            context,
            item.clone(),
            "stateChanged",
            statements,
            RuleAnswer::Updated { item },
        ),
        None,
        project,
        None,
        None,
    ))
}
pub(crate) const fn status_name(status: WorkStatus) -> &'static str {
    match status {
        WorkStatus::Backlog => "backlog",
        WorkStatus::Open => "open",
        WorkStatus::OnHold => "onHold",
        WorkStatus::Closed => "closed",
    }
}
#[cfg(test)]
#[path = "status_rules_test.rs"]
mod tests;
