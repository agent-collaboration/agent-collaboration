use crate::statement_plans::{
    RulePlan, WriteContext, any, change_metadata, exact, not_found, plan, text, unchanged,
};
use workspace_model::{
    AssignmentState, Deliverable, DeliverableRole, DeliverableRoleAssignment, ItemRef, Project,
    ProjectRole, ProjectRoleAssignment, RefusalReason, RoleCommand, RuleAnswer, named_project,
};

pub struct RoleReads<'a> {
    pub project: &'a Project,
    pub deliverable: Option<&'a Deliverable>,
    pub project_assignment: Option<&'a ProjectRoleAssignment>,
    pub deliverable_assignment: Option<&'a DeliverableRoleAssignment>,
}
pub fn change_role(
    context: &WriteContext,
    command: &RoleCommand,
    reads: RoleReads<'_>,
) -> Result<RulePlan, RefusalReason> {
    // Ported equality check from message-board::work_rules::require_role_grant.
    if reads.project.lead != context.actor {
        return Err(RefusalReason::NotLead {
            project: named_project(reads.project),
            lead: reads.project.lead.clone(),
        });
    }
    let now = || text(context.now.to_rfc3339());
    let (item, role, holder, statements, event) = match command {
        RoleCommand::GrantProject {
            assignment_id,
            project_id,
            role,
            holder,
        } => {
            if reads.project.project_id != *project_id {
                return Err(not_found(ItemRef::Project(project_id.clone())));
            }
            if reads.project_assignment.is_some_and(|row| {
                row.project_id == *project_id
                    && row.role == *role
                    && row.holder == *holder
                    && row.state == AssignmentState::Active
            }) {
                return Ok(unchanged());
            }
            (
                ItemRef::Project(project_id.clone()),
                project_role_name(*role),
                holder.clone(),
                vec![
                    any(
                        "UPDATE project_roles SET ended_at=?,updated_at=? WHERE project_id=? AND role=? AND ended_at IS NULL",
                        vec![
                            now(),
                            now(),
                            text(project_id),
                            text(project_role_name(*role)),
                        ],
                    ),
                    exact(
                        "INSERT INTO project_roles (project_role_id,project_id,role,holder,granted_at,ended_at,created_at,updated_at) VALUES (?,?,?,?,?,NULL,?,?)",
                        vec![
                            text(assignment_id),
                            text(project_id),
                            text(project_role_name(*role)),
                            text(holder),
                            now(),
                            now(),
                            now(),
                        ],
                    ),
                ],
                "roleGranted",
            )
        }
        RoleCommand::ReleaseProject { project_id, role } => {
            if reads.project.project_id != *project_id {
                return Err(not_found(ItemRef::Project(project_id.clone())));
            }
            if *role == ProjectRole::Lead {
                return Err(RefusalReason::LeadRequired {
                    project: named_project(reads.project),
                });
            }
            let Some(assignment) = reads.project_assignment.filter(|row| {
                row.project_id == *project_id
                    && row.role == *role
                    && row.state == AssignmentState::Active
            }) else {
                return Ok(unchanged());
            };
            (
                ItemRef::Project(project_id.clone()),
                project_role_name(*role),
                assignment.holder.clone(),
                vec![exact(
                    "UPDATE project_roles SET ended_at=?,updated_at=? WHERE project_role_id=? AND ended_at IS NULL",
                    vec![now(), now(), text(&assignment.project_role_id)],
                )],
                "roleEnded",
            )
        }
        RoleCommand::GrantDeliverable {
            assignment_id,
            deliverable_id,
            role,
            holder,
        } => {
            reads
                .deliverable
                .filter(|row| {
                    row.deliverable_id == *deliverable_id
                        && row.project_id == reads.project.project_id
                })
                .ok_or_else(|| not_found(ItemRef::Deliverable(deliverable_id.clone())))?;
            if reads.deliverable_assignment.is_some_and(|row| {
                row.deliverable_id == *deliverable_id
                    && row.role == *role
                    && row.holder == *holder
                    && row.state == AssignmentState::Active
            }) {
                return Ok(unchanged());
            }
            (
                ItemRef::Deliverable(deliverable_id.clone()),
                deliverable_role_name(*role),
                holder.clone(),
                vec![
                    any(
                        "UPDATE deliverable_roles SET ended_at=?,updated_at=? WHERE deliverable_id=? AND role=? AND ended_at IS NULL",
                        vec![
                            now(),
                            now(),
                            text(deliverable_id),
                            text(deliverable_role_name(*role)),
                        ],
                    ),
                    exact(
                        "INSERT INTO deliverable_roles (deliverable_role_id,deliverable_id,role,holder,granted_at,ended_at,created_at,updated_at) VALUES (?,?,?,?,?,NULL,?,?)",
                        vec![
                            text(assignment_id),
                            text(deliverable_id),
                            text(deliverable_role_name(*role)),
                            text(holder),
                            now(),
                            now(),
                            now(),
                        ],
                    ),
                ],
                "roleGranted",
            )
        }
        RoleCommand::ReleaseDeliverable {
            deliverable_id,
            role,
        } => {
            reads
                .deliverable
                .filter(|row| {
                    row.deliverable_id == *deliverable_id
                        && row.project_id == reads.project.project_id
                })
                .ok_or_else(|| not_found(ItemRef::Deliverable(deliverable_id.clone())))?;
            let Some(assignment) = reads.deliverable_assignment.filter(|row| {
                row.deliverable_id == *deliverable_id
                    && row.role == *role
                    && row.state == AssignmentState::Active
            }) else {
                return Ok(unchanged());
            };
            (
                ItemRef::Deliverable(deliverable_id.clone()),
                deliverable_role_name(*role),
                assignment.holder.clone(),
                vec![exact(
                    "UPDATE deliverable_roles SET ended_at=?,updated_at=? WHERE deliverable_role_id=? AND ended_at IS NULL",
                    vec![now(), now(), text(&assignment.deliverable_role_id)],
                )],
                "roleEnded",
            )
        }
    };
    Ok(change_metadata(
        plan(
            context,
            item.clone(),
            event,
            statements,
            RuleAnswer::Updated { item },
        ),
        None,
        Some(&reads.project.project_id),
        Some(&holder),
        Some(role),
    ))
}
pub(crate) const fn project_role_name(role: ProjectRole) -> &'static str {
    match role {
        ProjectRole::Lead => "lead",
        ProjectRole::Advisor => "advisor",
    }
}
pub(crate) const fn deliverable_role_name(role: DeliverableRole) -> &'static str {
    match role {
        DeliverableRole::Implementer => "implementer",
        DeliverableRole::Verifier => "verifier",
        DeliverableRole::Reviewer => "reviewer",
    }
}

#[cfg(test)]
#[path = "role_rules_test.rs"]
mod tests;
