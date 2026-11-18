use crate::organization_rules::require_area;
use crate::statement_plans::{
    RulePlan, WriteContext, change_metadata, change_row, exact, not_found, plan, text, unchanged,
};
use central_storage::{AlarmUpdate, GuardedStatement, StatementBatch};
use workspace_model::{
    Addressee, Area, AssignmentState, DeliverableRoleAssignment, ItemRef, MemberRole,
    MembershipState, NotificationSetting, ProjectRoleAssignment, RefusalReason, RuleAnswer, Thread,
    ThreadCommand, ThreadId, ThreadMember, ThreadPostDraft, ThreadState, encode_json,
};

/// Role rows are needed only to validate role addressees, not to select recipients.
#[derive(Default)]
pub struct ThreadReads<'a> {
    pub area: Option<&'a Area>,
    pub thread: Option<&'a Thread>,
    pub caller_member: Option<&'a ThreadMember>,
    pub target_member: Option<&'a ThreadMember>,
    pub project_roles: &'a [ProjectRoleAssignment],
    pub deliverable_roles: &'a [DeliverableRoleAssignment],
}
pub fn change_thread(
    context: &WriteContext,
    command: &ThreadCommand,
    reads: ThreadReads<'_>,
) -> Result<RulePlan, RefusalReason> {
    let now = || text(context.now.to_rfc3339());
    if let ThreadCommand::CreateThread {
        thread_id,
        member_id,
        area_id,
        title,
        first_post,
    } = command
    {
        require_area(context, area_id, reads.area)?;
        require_held_addressees(&first_post.addressees, &reads)?;
        let item = ItemRef::Thread(thread_id.clone());
        let mut statements = vec![
            exact(
                "INSERT INTO threads (thread_id,area_id,title,creator,state,created_at,updated_at) VALUES (?,?,?,?,'open',?,?)",
                vec![
                    text(thread_id),
                    text(area_id),
                    text(title.as_str()),
                    text(&context.actor),
                    now(),
                    now(),
                ],
            ),
            exact(
                "INSERT INTO thread_members (thread_member_id,thread_id,actor_identity_id,member_role,notifications,left_at,created_at,updated_at) VALUES (?,?,?,'participant',1,NULL,?,?)",
                vec![
                    text(member_id),
                    text(thread_id),
                    text(&context.actor),
                    now(),
                    now(),
                ],
            ),
            change_row(context, &context.change_id, "created", &item),
        ];
        statements.extend(post_statements(context, thread_id, first_post)?);
        return Ok(RulePlan {
            batch: StatementBatch {
                statements,
                alarm: AlarmUpdate::Unchanged,
            },
            answer: RuleAnswer::Created { item },
        });
    }
    let thread_id = match command {
        ThreadCommand::CreateThread { thread_id, .. }
        | ThreadCommand::JoinThread { thread_id, .. }
        | ThreadCommand::AddThreadMember { thread_id, .. }
        | ThreadCommand::LeaveThread { thread_id }
        | ThreadCommand::SetNotifications { thread_id, .. }
        | ThreadCommand::ThreadPost { thread_id, .. }
        | ThreadCommand::ResolveThread { thread_id, .. }
        | ThreadCommand::ReopenThread { thread_id } => thread_id,
    };
    let thread = reads
        .thread
        .filter(|row| row.thread_id == *thread_id)
        .ok_or_else(|| not_found(ItemRef::Thread(thread_id.clone())))?;
    // Only a reopen acts on a resolved thread, and a reopen acts on nothing else.
    match (command, &thread.state) {
        (ThreadCommand::ReopenThread { .. }, ThreadState::Open) => {
            return Err(RefusalReason::ThreadOpen {
                thread_id: thread_id.clone(),
            });
        }
        (ThreadCommand::ReopenThread { .. }, ThreadState::Resolved) => {}
        (_, ThreadState::Resolved) => {
            return Err(RefusalReason::ThreadResolved {
                thread_id: thread_id.clone(),
            });
        }
        (_, ThreadState::Open) => {}
    }
    let current = reads.caller_member.filter(|row| {
        row.thread_id == *thread_id
            && row.actor_identity_id == context.actor
            && row.state == MembershipState::Joined
    });
    let (item, event, subject, statements) = match command {
        ThreadCommand::CreateThread { .. } => {
            return Err(RefusalReason::InvalidInput {
                field: "createThread".to_owned(),
            });
        }
        ThreadCommand::ResolveThread { closing_post, .. } => {
            if thread.creator != context.actor {
                return Err(RefusalReason::NotCreator {
                    creator: thread.creator.clone(),
                });
            }
            // The closing post lands while the thread is still open, in the same batch.
            let mut statements = match closing_post {
                Some(post) => {
                    require_participant(current, thread_id)?;
                    require_held_addressees(&post.addressees, &reads)?;
                    post_statements(context, thread_id, post)?
                }
                None => vec![],
            };
            statements.push(exact(
                "UPDATE threads SET state='resolved',updated_at=? WHERE thread_id=? AND creator=? AND state='open'",
                vec![now(), text(thread_id), text(&context.actor)],
            ));
            (
                ItemRef::Thread(thread_id.clone()),
                "resolved",
                None,
                statements,
            )
        }
        ThreadCommand::ReopenThread { .. } => {
            if thread.creator != context.actor {
                return Err(RefusalReason::NotCreator {
                    creator: thread.creator.clone(),
                });
            }
            (
                ItemRef::Thread(thread_id.clone()),
                "reopened",
                None,
                vec![exact(
                    "UPDATE threads SET state='open',updated_at=? WHERE thread_id=? AND creator=? AND state='resolved'",
                    vec![now(), text(thread_id), text(&context.actor)],
                )],
            )
        }
        ThreadCommand::JoinThread {
            member_id, role, ..
        } => {
            let statement = if let Some(member) = current {
                // Joining never demotes an existing participant or resets notifications.
                if member.role == *role || member.role == MemberRole::Participant {
                    return Ok(unchanged());
                }
                exact(
                    "UPDATE thread_members SET member_role='participant',updated_at=? WHERE thread_member_id=? AND left_at IS NULL",
                    vec![now(), text(&member.thread_member_id)],
                )
            } else {
                exact(
                    "INSERT INTO thread_members (thread_member_id,thread_id,actor_identity_id,member_role,notifications,left_at,created_at,updated_at) VALUES (?,?,?,?,1,NULL,?,?)",
                    vec![
                        text(member_id),
                        text(thread_id),
                        text(&context.actor),
                        text(member_role_name(*role)),
                        now(),
                        now(),
                    ],
                )
            };
            (
                ItemRef::Thread(thread_id.clone()),
                "joined",
                Some(&context.actor),
                vec![statement],
            )
        }
        ThreadCommand::AddThreadMember {
            member_id,
            actor,
            role,
            ..
        } => {
            if thread.creator != context.actor {
                return Err(RefusalReason::NotCreator {
                    creator: thread.creator.clone(),
                });
            }
            let target = reads.target_member.filter(|row| {
                row.thread_id == *thread_id
                    && row.actor_identity_id == *actor
                    && row.state == MembershipState::Joined
            });
            let statement = if let Some(target) = target {
                if target.role == *role {
                    return Ok(unchanged());
                }
                exact(
                    "UPDATE thread_members SET member_role=?,updated_at=? WHERE thread_member_id=? AND left_at IS NULL",
                    vec![
                        text(member_role_name(*role)),
                        now(),
                        text(&target.thread_member_id),
                    ],
                )
            } else {
                exact(
                    "INSERT INTO thread_members (thread_member_id,thread_id,actor_identity_id,member_role,notifications,left_at,created_at,updated_at) VALUES (?,?,?,?,1,NULL,?,?)",
                    vec![
                        text(member_id),
                        text(thread_id),
                        text(actor),
                        text(member_role_name(*role)),
                        now(),
                        now(),
                    ],
                )
            };
            (
                ItemRef::Thread(thread_id.clone()),
                "joined",
                Some(actor),
                vec![statement],
            )
        }
        ThreadCommand::LeaveThread { .. } => {
            let member = current.ok_or_else(|| RefusalReason::NotMember {
                thread_id: thread_id.clone(),
            })?;
            (
                ItemRef::Thread(thread_id.clone()),
                "left",
                Some(&context.actor),
                vec![exact(
                    "UPDATE thread_members SET left_at=?,updated_at=? WHERE thread_member_id=? AND left_at IS NULL",
                    vec![now(), now(), text(&member.thread_member_id)],
                )],
            )
        }
        ThreadCommand::SetNotifications { setting, .. } => {
            let member = current.ok_or_else(|| RefusalReason::NotMember {
                thread_id: thread_id.clone(),
            })?;
            if member.notifications == *setting {
                return Ok(unchanged());
            }
            (
                ItemRef::Thread(thread_id.clone()),
                if *setting == NotificationSetting::On {
                    "notificationsOn"
                } else {
                    "notificationsOff"
                },
                Some(&context.actor),
                vec![exact(
                    "UPDATE thread_members SET notifications=?,updated_at=? WHERE thread_member_id=? AND left_at IS NULL",
                    vec![
                        central_storage::SqlValue::Integer(
                            central_storage::SafeInteger::new(i64::from(
                                *setting == NotificationSetting::On,
                            ))
                            .map_err(|_| {
                                RefusalReason::InvalidInput {
                                    field: "notifications".to_owned(),
                                }
                            })?,
                        ),
                        now(),
                        text(&member.thread_member_id),
                    ],
                )],
            )
        }
        ThreadCommand::ThreadPost {
            post_id,
            body,
            addressees,
            ..
        } => {
            require_participant(current, thread_id)?;
            require_held_addressees(addressees, &reads)?;
            let encoded = encode_json(addressees).map_err(|error| RefusalReason::InvalidInput {
                field: error.field.to_owned(),
            })?;
            (
                ItemRef::Post(post_id.clone()),
                "posted",
                None,
                vec![exact(
                    "INSERT INTO posts (post_id,thread_id,author,body,addressees,created_at,updated_at) VALUES (?,?,?,?,?,?,?)",
                    vec![
                        text(post_id),
                        text(thread_id),
                        text(&context.actor),
                        text(body.as_str()),
                        text(encoded),
                        now(),
                        now(),
                    ],
                )],
            )
        }
    };
    let answer = if matches!(command, ThreadCommand::ThreadPost { .. }) {
        RuleAnswer::Created { item: item.clone() }
    } else {
        RuleAnswer::Updated { item: item.clone() }
    };
    Ok(change_metadata(
        plan(context, item, event, statements, answer),
        Some(thread_id),
        None,
        subject,
        None,
    ))
}
fn require_participant(
    current: Option<&ThreadMember>,
    thread_id: &ThreadId,
) -> Result<(), RefusalReason> {
    let member = current.ok_or_else(|| RefusalReason::NotMember {
        thread_id: thread_id.clone(),
    })?;
    if member.role != MemberRole::Participant {
        return Err(RefusalReason::NotParticipant {
            thread_id: thread_id.clone(),
        });
    }
    Ok(())
}
fn require_held_addressees(
    addressees: &[Addressee],
    reads: &ThreadReads<'_>,
) -> Result<(), RefusalReason> {
    for addressee in addressees {
        let held = match addressee {
            Addressee::Actor { .. } => true,
            Addressee::ProjectRole { project_id, role } => reads.project_roles.iter().any(|row| {
                row.project_id == *project_id
                    && row.role == *role
                    && row.state == AssignmentState::Active
            }),
            Addressee::DeliverableRole {
                deliverable_id,
                role,
            } => reads.deliverable_roles.iter().any(|row| {
                row.deliverable_id == *deliverable_id
                    && row.role == *role
                    && row.state == AssignmentState::Active
            }),
        };
        if !held {
            return Err(RefusalReason::UnheldAddressee);
        }
    }
    Ok(())
}
/// A post written inside another thread step: its row and its own `posted`
/// change, attributed to the thread, so readers see it as any other post.
fn post_statements(
    context: &WriteContext,
    thread_id: &ThreadId,
    post: &ThreadPostDraft,
) -> Result<Vec<GuardedStatement>, RefusalReason> {
    let encoded = encode_json(&post.addressees).map_err(|error| RefusalReason::InvalidInput {
        field: error.field.to_owned(),
    })?;
    let now = || text(context.now.to_rfc3339());
    let mut change = change_row(
        context,
        &post.change_id,
        "posted",
        &ItemRef::Post(post.post_id.clone()),
    );
    if let Some(thread) = change.statement.args.get_mut(4) {
        *thread = text(thread_id);
    }
    Ok(vec![
        exact(
            "INSERT INTO posts (post_id,thread_id,author,body,addressees,created_at,updated_at) VALUES (?,?,?,?,?,?,?)",
            vec![
                text(&post.post_id),
                text(thread_id),
                text(&context.actor),
                text(post.body.as_str()),
                text(encoded),
                now(),
                now(),
            ],
        ),
        change,
    ])
}
const fn member_role_name(role: MemberRole) -> &'static str {
    match role {
        MemberRole::Participant => "participant",
        MemberRole::Observer => "observer",
    }
}

#[cfg(test)]
#[path = "thread_rules_test.rs"]
mod tests;
