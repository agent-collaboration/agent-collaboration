//! Translate admitted semantic inputs into the existing pure rule's declared rows.
use crate::workspace_decode_error::corrupt;
use crate::workspace_row_codec::WorkspaceRows;
use crate::{ObjectHost, WorkspaceRuntime, WorkspaceStoreCalls};
use central_storage::StoreFailure;
use workspace_model as m;
use workspace_rules as rules;

pub(crate) struct ResolvedOperation {
    pub command: m::WorkspaceCommand,
    pub tag_ids: Vec<m::WorkLogTagId>,
    pub read_mark_id: m::ReadMarkId,
}
impl<Host: ObjectHost, Stores: WorkspaceStoreCalls> WorkspaceRuntime<Host, Stores> {
    pub(crate) async fn resolve_command(
        &self,
        op: &m::WorkspaceOperation,
        actor: &m::ActorIdentityId,
    ) -> Result<Result<ResolvedOperation, m::RefusalReason>, StoreFailure> {
        use m::WorkspaceOperation as Op;
        let mut tags = Vec::new();
        let command = match op {
            Op::CreateArea { name } => {
                m::WorkspaceCommand::Organization(m::OrganizationCommand::CreateArea {
                    area_id: self.mint()?,
                    name: name.clone(),
                })
            }
            Op::ArchiveArea { area_id } => {
                m::WorkspaceCommand::Organization(m::OrganizationCommand::ArchiveArea {
                    area_id: area_id.clone(),
                })
            }
            Op::CreateProject {
                area_id,
                title,
                lead,
            } => {
                let holder = match lead {
                    Some(reference) => match self.resolve_actor_ref(reference, actor).await? {
                        Ok(holder) => holder,
                        Err(refusal) => return Ok(Err(refusal)),
                    },
                    None => actor.clone(),
                };
                m::WorkspaceCommand::Organization(m::OrganizationCommand::CreateProject {
                    project_id: self.mint()?,
                    area_id: area_id.clone(),
                    title: title.clone(),
                    lead: holder,
                    role_id: self.mint()?,
                })
            }
            Op::CreateInitiative { name, description } => {
                m::WorkspaceCommand::Organization(m::OrganizationCommand::CreateInitiative {
                    initiative_id: self.mint()?,
                    name: name.clone(),
                    description: description.clone(),
                })
            }
            Op::UpdateInitiative {
                initiative_id,
                name,
                description,
            } => m::WorkspaceCommand::Organization(m::OrganizationCommand::UpdateInitiative {
                initiative_id: initiative_id.clone(),
                name: name.clone(),
                description: description.clone(),
            }),
            Op::CreateDeliverable { project_id, title } => {
                m::WorkspaceCommand::Organization(m::OrganizationCommand::CreateDeliverable {
                    deliverable_id: self.mint()?,
                    project_id: project_id.clone(),
                    title: title.clone(),
                })
            }
            Op::CreateIssue {
                deliverable_id,
                title,
            } => m::WorkspaceCommand::Organization(m::OrganizationCommand::CreateIssue {
                issue_id: self.mint()?,
                deliverable_id: deliverable_id.clone(),
                title: title.clone(),
            }),
            Op::GrantProjectRole {
                project_id,
                role,
                holder,
            } => {
                let holder = match self.resolve_actor_ref(holder, actor).await? {
                    Ok(holder) => holder,
                    Err(refusal) => return Ok(Err(refusal)),
                };
                m::WorkspaceCommand::Role(m::RoleCommand::GrantProject {
                    assignment_id: self.mint()?,
                    project_id: project_id.clone(),
                    role: *role,
                    holder,
                })
            }
            Op::ReleaseProjectRole { project_id, role } => {
                m::WorkspaceCommand::Role(m::RoleCommand::ReleaseProject {
                    project_id: project_id.clone(),
                    role: *role,
                })
            }
            Op::GrantDeliverableRole {
                deliverable_id,
                role,
                holder,
            } => {
                let holder = match self.resolve_actor_ref(holder, actor).await? {
                    Ok(holder) => holder,
                    Err(refusal) => return Ok(Err(refusal)),
                };
                m::WorkspaceCommand::Role(m::RoleCommand::GrantDeliverable {
                    assignment_id: self.mint()?,
                    deliverable_id: deliverable_id.clone(),
                    role: *role,
                    holder,
                })
            }
            Op::ReleaseDeliverableRole {
                deliverable_id,
                role,
            } => m::WorkspaceCommand::Role(m::RoleCommand::ReleaseDeliverable {
                deliverable_id: deliverable_id.clone(),
                role: *role,
            }),
            Op::CreateThread { area_id, title } => {
                m::WorkspaceCommand::Thread(m::ThreadCommand::CreateThread {
                    thread_id: self.mint()?,
                    member_id: self.mint()?,
                    area_id: area_id.clone(),
                    title: title.clone(),
                })
            }
            Op::JoinThread { thread_id } => {
                m::WorkspaceCommand::Thread(m::ThreadCommand::JoinThread {
                    thread_id: thread_id.clone(),
                    member_id: self.mint()?,
                })
            }
            Op::AddThreadMember {
                thread_id,
                actor: target,
                role,
            } => {
                if !self.named_actor(target).await? {
                    return Ok(Err(m::RefusalReason::UnknownActor {
                        actor_identity_id: target.clone(),
                    }));
                }
                m::WorkspaceCommand::Thread(m::ThreadCommand::AddThreadMember {
                    thread_id: thread_id.clone(),
                    member_id: self.mint()?,
                    actor: target.clone(),
                    role: *role,
                })
            }
            Op::LeaveThread { thread_id } => {
                m::WorkspaceCommand::Thread(m::ThreadCommand::LeaveThread {
                    thread_id: thread_id.clone(),
                })
            }
            Op::SetNotifications { thread_id, setting } => {
                m::WorkspaceCommand::Thread(m::ThreadCommand::SetNotifications {
                    thread_id: thread_id.clone(),
                    setting: *setting,
                })
            }
            Op::ThreadPost {
                thread_id,
                body,
                addressees,
            } => {
                for addressee in addressees {
                    if let m::Addressee::Actor { actor_identity_id } = addressee
                        && !self.named_actor(actor_identity_id).await?
                    {
                        return Ok(Err(m::RefusalReason::UnknownActor {
                            actor_identity_id: actor_identity_id.clone(),
                        }));
                    }
                }
                m::WorkspaceCommand::Thread(m::ThreadCommand::ThreadPost {
                    post_id: self.mint()?,
                    thread_id: thread_id.clone(),
                    body: body.clone(),
                    addressees: addressees.clone(),
                })
            }
            Op::ResolveThread { thread_id } => {
                m::WorkspaceCommand::Thread(m::ThreadCommand::ResolveThread {
                    thread_id: thread_id.clone(),
                })
            }
            Op::AddDependency { blocking, blocked } => {
                m::WorkspaceCommand::Dependency(m::DependencyCommand::Add {
                    dependency_id: self.mint()?,
                    blocking: blocking.clone(),
                    blocked: blocked.clone(),
                })
            }
            Op::RemoveDependency { dependency_id } => {
                m::WorkspaceCommand::Dependency(m::DependencyCommand::Remove {
                    dependency_id: dependency_id.clone(),
                })
            }
            Op::SetIssueStatus { issue_id, status } => {
                m::WorkspaceCommand::Status(m::StatusCommand::SetIssue {
                    issue_id: issue_id.clone(),
                    status: *status,
                })
            }
            Op::SetProjectStatus { project_id, status } => {
                m::WorkspaceCommand::Status(m::StatusCommand::SetProject {
                    project_id: project_id.clone(),
                    status: *status,
                })
            }
            Op::SetInitiativeStatus {
                initiative_id,
                status,
            } => m::WorkspaceCommand::Status(m::StatusCommand::SetInitiative {
                initiative_id: initiative_id.clone(),
                status: *status,
            }),
            Op::AddBacklogItem {
                area_id,
                title,
                description,
            } => m::WorkspaceCommand::Backlog(m::BacklogCommand::Add {
                item_id: self.mint()?,
                area_id: area_id.clone(),
                title: title.clone(),
                description: description.clone(),
            }),
            Op::UpdateBacklogItem {
                item_id,
                title,
                description,
            } => m::WorkspaceCommand::Backlog(m::BacklogCommand::Update {
                item_id: item_id.clone(),
                title: title.clone(),
                description: description.clone(),
            }),
            Op::RemoveBacklogItem { item_id } => {
                m::WorkspaceCommand::Backlog(m::BacklogCommand::Remove {
                    item_id: item_id.clone(),
                })
            }
            Op::AddReference {
                source,
                target,
                details,
            } => m::WorkspaceCommand::Reference(m::ReferenceCommand::AddReference {
                reference: m::Reference {
                    reference_id: self.mint()?,
                    source: source.clone(),
                    target: target.clone(),
                    details: details.clone(),
                },
            }),
            Op::RemoveReference { reference_id } => {
                m::WorkspaceCommand::Reference(m::ReferenceCommand::RemoveReference {
                    reference_id: reference_id.clone(),
                })
            }
            Op::AddLink {
                source,
                url,
                title,
                details,
            } => m::WorkspaceCommand::Reference(m::ReferenceCommand::AddLink {
                link: m::ExternalLink {
                    link_id: self.mint()?,
                    source: source.clone(),
                    url: url.clone(),
                    title: title.clone(),
                    details: details.clone(),
                },
            }),
            Op::RemoveLink { link_id } => {
                m::WorkspaceCommand::Reference(m::ReferenceCommand::RemoveLink {
                    link_id: link_id.clone(),
                })
            }
            Op::WorkLogAdd {
                project_id,
                body,
                tags: labels,
            } => {
                for _ in labels {
                    tags.push(self.mint()?);
                }
                m::WorkspaceCommand::WorkLog(m::WorkLogCommand::Add {
                    entry_id: self.mint()?,
                    project_id: project_id.clone(),
                    body: body.clone(),
                    tags: labels.clone(),
                })
            }
            Op::WorkLogRedact { entry_id } => {
                m::WorkspaceCommand::WorkLog(m::WorkLogCommand::Redact {
                    entry_id: entry_id.clone(),
                })
            }
            Op::MarkRead { scope, through } => m::WorkspaceCommand::MarkRead {
                scope: scope.clone(),
                through: *through,
            },
            Op::Show { .. } | Op::List { .. } | Op::Inbox { .. } | Op::CreateWorkspace { .. } => {
                return Err(corrupt("operation dispatch"));
            }
        };
        Ok(Ok(ResolvedOperation {
            command,
            tag_ids: tags,
            read_mark_id: self.mint()?,
        }))
    }
}

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
        m::WorkspaceCommand::Backlog(command) => rules::change_backlog(
            context,
            command,
            rules::BacklogReads {
                area: rows.areas.first(),
                item: rows.backlog.first(),
            },
        ),
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
