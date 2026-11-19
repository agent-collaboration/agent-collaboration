use crate::statement_plans::{
    RulePlan, WriteContext, change_metadata, exact, not_found, plan, text,
};
use workspace_model::{
    Area, Deliverable, Initiative, Issue, ItemRef, OrganizationCommand, Project, RefusalReason,
    RuleAnswer,
};

/// Each variant needs only its named parent/current row; central scopes it first.
#[derive(Default)]
pub struct OrganizationReads<'a> {
    pub area: Option<&'a Area>,
    pub project: Option<&'a Project>,
    pub deliverable: Option<&'a Deliverable>,
    pub initiative: Option<&'a Initiative>,
    pub issue: Option<&'a Issue>,
}
pub fn organize_work(
    context: &WriteContext,
    command: &OrganizationCommand,
    reads: OrganizationReads<'_>,
) -> Result<RulePlan, RefusalReason> {
    let now = || text(context.now.to_rfc3339());
    let (item, statements, created) = match command {
        OrganizationCommand::CreateArea {
            area_id,
            name,
            description,
        } => (
            ItemRef::Area(area_id.clone()),
            vec![exact(
                "INSERT INTO workspace_areas (area_id,name,description,state,created_at,updated_at) VALUES (?,?,?,'open',?,?)",
                vec![
                    text(area_id),
                    text(name.as_str()),
                    text(description.as_str()),
                    now(),
                    now(),
                ],
            )],
            true,
        ),
        OrganizationCommand::ArchiveArea { area_id } => {
            require_area(context, area_id, reads.area)?;
            (
                ItemRef::Area(area_id.clone()),
                vec![exact(
                    "UPDATE workspace_areas SET state='archived',updated_at=? WHERE area_id=?",
                    vec![now(), text(area_id)],
                )],
                false,
            )
        }
        OrganizationCommand::UpdateArea {
            area_id,
            name,
            description,
        } => {
            require_area(context, area_id, reads.area)?;
            let statement = match description {
                Some(description) => exact(
                    "UPDATE workspace_areas SET name=?,description=?,updated_at=? WHERE area_id=?",
                    vec![
                        text(name.as_str()),
                        text(description.as_str()),
                        now(),
                        text(area_id),
                    ],
                ),
                None => exact(
                    "UPDATE workspace_areas SET name=?,updated_at=? WHERE area_id=?",
                    vec![text(name.as_str()), now(), text(area_id)],
                ),
            };
            (ItemRef::Area(area_id.clone()), vec![statement], false)
        }
        OrganizationCommand::UpdateProject {
            project_id,
            title,
            description,
        } => {
            reads
                .project
                .filter(|row| row.project_id == *project_id)
                .ok_or_else(|| not_found(ItemRef::Project(project_id.clone())))?;
            let statement = match description {
                Some(description) => exact(
                    "UPDATE workspace_projects SET title=?,description=?,updated_at=? WHERE project_id=?",
                    vec![
                        text(title.as_str()),
                        text(description.as_str()),
                        now(),
                        text(project_id),
                    ],
                ),
                None => exact(
                    "UPDATE workspace_projects SET title=?,updated_at=? WHERE project_id=?",
                    vec![text(title.as_str()), now(), text(project_id)],
                ),
            };
            (ItemRef::Project(project_id.clone()), vec![statement], false)
        }
        OrganizationCommand::UpdateIssue { issue_id, title } => {
            reads
                .issue
                .filter(|row| row.issue_id == *issue_id)
                .ok_or_else(|| not_found(ItemRef::Issue(issue_id.clone())))?;
            (
                ItemRef::Issue(issue_id.clone()),
                vec![exact(
                    "UPDATE workspace_issues SET title=?,updated_at=? WHERE issue_id=?",
                    vec![text(title.as_str()), now(), text(issue_id)],
                )],
                false,
            )
        }
        OrganizationCommand::CreateProject {
            project_id,
            area_id,
            title,
            description,
            lead,
            role_id,
        } => {
            require_area(context, area_id, reads.area)?;
            (
                ItemRef::Project(project_id.clone()),
                vec![
                    exact(
                        "INSERT INTO workspace_projects (project_id,area_id,title,description,status,created_at,updated_at) VALUES (?,?,?,?,'backlog',?,?)",
                        vec![
                            text(project_id),
                            text(area_id),
                            text(title.as_str()),
                            text(description.as_str()),
                            now(),
                            now(),
                        ],
                    ),
                    exact(
                        "INSERT INTO workspace_project_roles (project_role_id,project_id,role,holder,granted_at,ended_at,created_at,updated_at) VALUES (?,?,'lead',?,?,NULL,?,?)",
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
                "INSERT INTO workspace_initiatives (initiative_id,name,description,status,created_at,updated_at) VALUES (?,?,?,'backlog',?,?)",
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
                    "UPDATE workspace_initiatives SET name=?,description=?,updated_at=? WHERE initiative_id=?",
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
                    "INSERT INTO workspace_deliverables (deliverable_id,project_id,title,created_at,updated_at) VALUES (?,?,?,?,?)",
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
                    "INSERT INTO workspace_issues (issue_id,deliverable_id,title,status,created_at,updated_at) VALUES (?,?,?,'backlog',?,?)",
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
        OrganizationCommand::UpdateProject { project_id, .. } => Some(project_id),
        OrganizationCommand::CreateIssue { .. } | OrganizationCommand::UpdateIssue { .. } => {
            reads.deliverable.map(|row| &row.project_id)
        }
        OrganizationCommand::CreateArea { .. }
        | OrganizationCommand::ArchiveArea { .. }
        | OrganizationCommand::UpdateArea { .. }
        | OrganizationCommand::CreateInitiative { .. }
        | OrganizationCommand::UpdateInitiative { .. } => None,
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
