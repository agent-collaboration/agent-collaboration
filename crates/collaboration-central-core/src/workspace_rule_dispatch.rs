//! Translate admitted semantic inputs into the existing pure rule's declared rows.
use crate::workspace_decode_error::corrupt;
use crate::{ObjectHost, WorkspaceRuntime, WorkspaceStoreCalls};
use central_storage::StoreFailure;
use workspace_model as m;

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
            Op::JoinThread { thread_id, role } => {
                m::WorkspaceCommand::Thread(m::ThreadCommand::JoinThread {
                    thread_id: thread_id.clone(),
                    member_id: self.mint()?,
                    role: *role,
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
            Op::CloseBacklogItem {
                item_id,
                note,
                link,
            } => {
                let link = match link {
                    Some(target) => {
                        let target = match m::ReferenceTarget::try_from(target.clone()) {
                            Ok(target) => target,
                            Err(_) => {
                                return Ok(Err(m::RefusalReason::InvalidInput {
                                    field: "link".to_owned(),
                                }));
                            }
                        };
                        Some(m::BacklogClosingReference {
                            change_id: self.mint()?,
                            reference: m::Reference {
                                reference_id: self.mint()?,
                                source: m::ReferenceSource::BacklogItem(item_id.clone()),
                                target,
                                details: None,
                            },
                        })
                    }
                    None => None,
                };
                m::WorkspaceCommand::Backlog(m::BacklogCommand::Close {
                    item_id: item_id.clone(),
                    note: note.clone(),
                    link,
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
