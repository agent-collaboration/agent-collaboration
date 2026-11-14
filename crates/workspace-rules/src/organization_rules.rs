use crate::statement_plans::{
    RulePlan, WriteContext, change_metadata, exact, not_found, plan, text,
};
use workspace_model::{
    Area, Deliverable, Initiative, ItemRef, OrganizationCommand, Project, RefusalReason, RuleAnswer,
};

/// Each variant needs only its named parent/current row; central scopes it first.
#[derive(Default)]
pub struct OrganizationReads<'a> {
    pub area: Option<&'a Area>,
    pub project: Option<&'a Project>,
    pub deliverable: Option<&'a Deliverable>,
    pub initiative: Option<&'a Initiative>,
}
pub fn organize_work(
    context: &WriteContext,
    command: &OrganizationCommand,
    reads: OrganizationReads<'_>,
) -> Result<RulePlan, RefusalReason> {
    let now = || text(context.now.to_rfc3339());
    let (item, statements, created) = match command {
        OrganizationCommand::CreateArea { area_id, name } => (
            ItemRef::Area(area_id.clone()),
            vec![exact(
                "INSERT INTO areas (area_id,name,state,created_at,updated_at) VALUES (?,?,'open',?,?)",
                vec![text(area_id), text(name.as_str()), now(), now()],
            )],
            true,
        ),
        OrganizationCommand::ArchiveArea { area_id } => {
            require_area(context, area_id, reads.area)?;
            (
                ItemRef::Area(area_id.clone()),
                vec![exact(
                    "UPDATE areas SET state='archived',updated_at=? WHERE area_id=?",
                    vec![now(), text(area_id)],
                )],
                false,
            )
        }
        OrganizationCommand::CreateProject {
            project_id,
            area_id,
            title,
            lead,
            role_id,
        } => {
            require_area(context, area_id, reads.area)?;
            (
                ItemRef::Project(project_id.clone()),
                vec![
                    exact(
                        "INSERT INTO projects (project_id,area_id,title,status,created_at,updated_at) VALUES (?,?,?,'backlog',?,?)",
                        vec![
                            text(project_id),
                            text(area_id),
                            text(title.as_str()),
                            now(),
                            now(),
                        ],
                    ),
                    exact(
                        "INSERT INTO project_roles (project_role_id,project_id,role,holder,granted_at,ended_at,created_at,updated_at) VALUES (?,?,'lead',?,?,NULL,?,?)",
                        vec![
                            text(role_id),
                            text(project_id),
                            text(lead),
                            now(),
                            now(),
                            now(),
                        ],
                    ),
                ],
                true,
            )
        }
        OrganizationCommand::CreateInitiative {
            initiative_id,
            name,
            description,
        } => (
            ItemRef::Initiative(initiative_id.clone()),
            vec![exact(
                "INSERT INTO initiatives (initiative_id,name,description,status,created_at,updated_at) VALUES (?,?,?,'backlog',?,?)",
                vec![
                    text(initiative_id),
                    text(name.as_str()),
                    text(description.as_str()),
                    now(),
                    now(),
                ],
            )],
            true,
        ),
        OrganizationCommand::UpdateInitiative {
            initiative_id,
            name,
            description,
        } => {
            reads
                .initiative
                .filter(|row| row.initiative_id == *initiative_id)
                .ok_or_else(|| not_found(ItemRef::Initiative(initiative_id.clone())))?;
            (
                ItemRef::Initiative(initiative_id.clone()),
                vec![exact(
                    "UPDATE initiatives SET name=?,description=?,updated_at=? WHERE initiative_id=?",
                    vec![
                        text(name.as_str()),
                        text(description.as_str()),
                        now(),
                        text(initiative_id),
                    ],
                )],
                false,
            )
        }
        OrganizationCommand::CreateDeliverable {
            deliverable_id,
            project_id,
            title,
        } => {
            let project = reads
                .project
                .filter(|row| row.project_id == *project_id)
                .ok_or_else(|| not_found(ItemRef::Project(project_id.clone())))?;
            crate::status_rules::require_open_project(project)?;
            (
                ItemRef::Deliverable(deliverable_id.clone()),
                vec![exact(
                    "INSERT INTO deliverables (deliverable_id,project_id,title,created_at,updated_at) VALUES (?,?,?,?,?)",
                    vec![
                        text(deliverable_id),
                        text(project_id),
                        text(title.as_str()),
                        now(),
                        now(),
                    ],
                )],
                true,
            )
        }
        OrganizationCommand::CreateIssue {
            issue_id,
            deliverable_id,
            title,
        } => {
            let deliverable = reads
                .deliverable
                .filter(|row| row.deliverable_id == *deliverable_id)
                .ok_or_else(|| not_found(ItemRef::Deliverable(deliverable_id.clone())))?;
            let project = reads
                .project
                .filter(|row| row.project_id == deliverable.project_id)
                .ok_or_else(|| not_found(ItemRef::Project(deliverable.project_id.clone())))?;
            crate::status_rules::require_open_project(project)?;
            (
                ItemRef::Issue(issue_id.clone()),
                vec![exact(
                    "INSERT INTO issues (issue_id,deliverable_id,title,status,created_at,updated_at) VALUES (?,?,?,'backlog',?,?)",
                    vec![
                        text(issue_id),
                        text(deliverable_id),
                        text(title.as_str()),
                        now(),
                        now(),
                    ],
                )],
                true,
            )
        }
    };
    let answer = if created {
        RuleAnswer::Created { item: item.clone() }
    } else {
        RuleAnswer::Updated { item: item.clone() }
    };
    let project = match command {
        OrganizationCommand::CreateProject { project_id, .. }
        | OrganizationCommand::CreateDeliverable { project_id, .. } => Some(project_id),
        OrganizationCommand::CreateIssue { .. } => reads.deliverable.map(|row| &row.project_id),
        _ => None,
    };
    Ok(change_metadata(
        plan(
            context,
            item,
            if created { "created" } else { "updated" },
            statements,
            answer,
        ),
        None,
        project,
        None,
        None,
    ))
}
pub(crate) fn require_area(
    context: &WriteContext,
    area_id: &workspace_model::AreaId,
    row: Option<&Area>,
) -> Result<(), RefusalReason> {
    row.filter(|area| area.area_id == *area_id && area.workspace_id == context.workspace_id)
        .ok_or_else(|| not_found(ItemRef::Area(area_id.clone())))?;
    Ok(())
}

#[cfg(test)]
#[path = "organization_rules_test.rs"]
mod tests;
