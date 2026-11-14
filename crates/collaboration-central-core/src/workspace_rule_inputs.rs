//! Supply each pure rule with the admitted rows its command needs.
use crate::workspace_row_codec::WorkspaceRows;
use crate::workspace_rule_dispatch::ResolvedOperation;
use workspace_model as m;
use workspace_rules as rules;

pub(crate) fn run_rule(
    context: &rules::WriteContext,
    resolved: &ResolvedOperation,
    rows: &WorkspaceRows,
) -> Result<rules::RulePlan, m::RefusalReason> {
    match &resolved.command {
        m::WorkspaceCommand::Organization(command) => rules::organize_work(
            context,
            command,
            rules::OrganizationReads {
                area: rows.areas.first(),
                project: rows.projects.first(),
                deliverable: match command {
                    m::OrganizationCommand::CreateIssue { deliverable_id, .. } => rows
                        .deliverables
                        .iter()
                        .find(|row| row.deliverable_id == *deliverable_id),
                    _ => rows.deliverables.first(),
                },
                initiative: rows.initiatives.first(),
            },
        ),
        m::WorkspaceCommand::Role(command) => {
            let (project_id, deliverable_id) = match command {
                m::RoleCommand::GrantProject { project_id, .. }
                | m::RoleCommand::ReleaseProject { project_id, .. } => (Some(project_id), None),
                m::RoleCommand::GrantDeliverable { deliverable_id, .. }
                | m::RoleCommand::ReleaseDeliverable { deliverable_id, .. } => {
                    (None, Some(deliverable_id))
                }
            };
            let deliverable = deliverable_id.and_then(|id| {
                rows.deliverables
                    .iter()
                    .find(|row| row.deliverable_id == *id)
            });
            if let Some(id) = deliverable_id
                && deliverable.is_none()
            {
                return Err(m::RefusalReason::NotFound {
                    item: m::ItemRef::Deliverable(id.clone()),
                });
            }
            let project_id = project_id
                .or_else(|| deliverable.map(|row| &row.project_id))
                .ok_or_else(|| m::RefusalReason::InvalidInput {
                    field: "role project".to_owned(),
                })?;
            let project = rows
                .projects
                .iter()
                .find(|row| &row.project_id == project_id)
                .ok_or_else(|| m::RefusalReason::NotFound {
                    item: m::ItemRef::Project(project_id.clone()),
                })?;
            let project_assignment = rows.project_roles.iter().find(|row| {
                row.state == m::AssignmentState::Active
                    && match command {
                        m::RoleCommand::GrantProject { role, .. }
                        | m::RoleCommand::ReleaseProject { role, .. } => row.role == *role,
                        _ => false,
                    }
            });
            let deliverable_assignment = rows.deliverable_roles.iter().find(|row| {
                row.state == m::AssignmentState::Active
                    && Some(&row.deliverable_id) == deliverable_id
                    && match command {
                        m::RoleCommand::GrantDeliverable { role, .. }
                        | m::RoleCommand::ReleaseDeliverable { role, .. } => row.role == *role,
                        _ => false,
                    }
            });
            rules::change_role(
                context,
                command,
                rules::RoleReads {
                    project,
                    deliverable,
                    project_assignment,
                    deliverable_assignment,
                },
            )
        }
        m::WorkspaceCommand::Thread(command) => {
            let target = match command {
                m::ThreadCommand::AddThreadMember { actor, .. } => actor,
                _ => &context.actor,
            };
            rules::change_thread(
                context,
                command,
                rules::ThreadReads {
                    area: rows.areas.first(),
                    thread: rows.threads.first(),
                    caller_member: rows.members.iter().find(|row| {
                        row.actor_identity_id == context.actor
                            && row.state == m::MembershipState::Joined
                    }),
                    target_member: rows.members.iter().find(|row| {
                        row.actor_identity_id == *target && row.state == m::MembershipState::Joined
                    }),
                    project_roles: &rows.project_roles,
                    deliverable_roles: &rows.deliverable_roles,
                },
            )
        }
        m::WorkspaceCommand::Dependency(command) => rules::change_dependency(
            context,
            command,
            rules::DependencyReads {
                deliverables: &rows.deliverables,
                dependencies: &rows.dependencies,
            },
        ),
        m::WorkspaceCommand::Status(command) => {
            let issue = match command {
                m::StatusCommand::SetIssue { issue_id, .. } => {
                    rows.issues.iter().find(|row| row.issue_id == *issue_id)
                }
                _ => None,
            };
            let project = match command {
                m::StatusCommand::SetProject { project_id, .. } => rows
                    .projects
                    .iter()
                    .find(|row| row.project_id == *project_id),
                _ => rows.projects.first(),
            };
            rules::set_work_status(
                context,
                command,
                rules::StatusReads {
                    issue,
                    project,
                    initiative: match command {
                        m::StatusCommand::SetInitiative { initiative_id, .. } => rows
                            .initiatives
                            .iter()
                            .find(|row| row.initiative_id == *initiative_id),
                        _ => None,
                    },
                    deliverables: &rows.deliverables,
                    issues: &rows.issues,
                    initiative_projects: &rows.projects,
                    project_initiatives: &rows.initiatives,
                },
            )
        }
        m::WorkspaceCommand::Backlog(command) => {
            let item_id = match command {
                m::BacklogCommand::Add { item_id, .. }
                | m::BacklogCommand::Update { item_id, .. }
                | m::BacklogCommand::Close { item_id, .. }
                | m::BacklogCommand::Remove { item_id } => item_id,
            };
            let target = match command {
                m::BacklogCommand::Close {
                    link: Some(link), ..
                } => {
                    let item = link.reference.target.as_item_ref();
                    crate::workspace_item_reads::contains_item(rows, item).then(|| {
                        rules::LocatedItem {
                            workspace_id: context.workspace_id.clone(),
                            item: item.clone(),
                        }
                    })
                }
                _ => None,
            };
            rules::change_backlog(
                context,
                command,
                rules::BacklogReads {
                    area: rows.areas.first(),
                    item: rows.backlog.iter().find(|row| row.item_id == *item_id),
                    target: target.as_ref(),
                },
            )
        }
        m::WorkspaceCommand::Reference(command) => {
            let (source, target) = match command {
                m::ReferenceCommand::AddReference { reference } => (
                    Some(&reference.source),
                    Some(reference.target.as_item_ref()),
                ),
                m::ReferenceCommand::AddLink { link } => (Some(&link.source), None),
                m::ReferenceCommand::RemoveReference { .. } => {
                    (rows.references.first().map(|row| &row.source), None)
                }
                m::ReferenceCommand::RemoveLink { .. } => {
                    (rows.links.first().map(|row| &row.source), None)
                }
            };
            let located = |item: m::ItemRef| {
                crate::workspace_item_reads::contains_item(rows, &item).then_some(
                    rules::LocatedItem {
                        workspace_id: context.workspace_id.clone(),
                        item,
                    },
                )
            };
            let source_ref = source.map(|source| m::ItemRef::from(source.clone()));
            let source = source_ref.clone().and_then(located);
            let target = target.and_then(|item| located(item.clone()));
            rules::change_reference(
                context,
                command,
                rules::ReferenceReads {
                    source: source.as_ref(),
                    target: target.as_ref(),
                    reference: rows.references.first(),
                    link: rows.links.first(),
                    initiative: rows.initiatives.iter().find(|row| {
                        source_ref == Some(m::ItemRef::Initiative(row.initiative_id.clone()))
                    }),
                    project: rows.projects.iter().find(|row| {
                        target.as_ref().is_some_and(|target| {
                            target.item == m::ItemRef::Project(row.project_id.clone())
                        })
                    }),
                    source_post: rows
                        .posts
                        .iter()
                        .find(|row| source_ref == Some(m::ItemRef::Post(row.post_id.clone()))),
                    source_work_log: rows.entries.iter().find(|row| {
                        source_ref == Some(m::ItemRef::WorkLogEntry(row.entry_id.clone()))
                    }),
                    source_deliverable: rows.deliverables.iter().find(|row| {
                        source_ref == Some(m::ItemRef::Deliverable(row.deliverable_id.clone()))
                    }),
                },
            )
        }
        m::WorkspaceCommand::WorkLog(command) => rules::change_work_log(
            context,
            command,
            rules::WorkLogReads {
                project: rows.projects.first(),
                entry: rows.entries.first(),
                tag_ids: &resolved.tag_ids,
            },
        ),
        m::WorkspaceCommand::MarkRead { scope, through } => rules::mark_read(
            context,
            &resolved.read_mark_id,
            scope.clone(),
            *through,
            rows.marks.iter().find(|row| row.scope == *scope),
        ),
        m::WorkspaceCommand::Show(_) | m::WorkspaceCommand::List(_) => {
            Err(m::RefusalReason::InvalidInput {
                field: "write command".to_owned(),
            })
        }
    }
}
